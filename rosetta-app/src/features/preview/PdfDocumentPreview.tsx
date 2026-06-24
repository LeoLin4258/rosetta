import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Card } from "@/components/ui/card";
import {
  countRosettaPdfPages,
  getRosettaPdfPageStatus,
  renderRosettaPdfTranslatedPageAsPng,
  type PdfPageTranslation,
  type PdfPageTranslationState,
} from "@/lib/rosettaJobs";
import type {
  RosettaDocument,
  RosettaTranslationFile,
} from "../../types/rosetta";

import { PdfPane, type PdfPaneHandle } from "./PdfPane";

type PreviewSide = "source" | "translation";

type ScrollAnchor = {
  pageIndex: number;
  localOffsetRatio: number;
};

type PdfPreviewCacheEntry = {
  sourcePageCount: number | null;
  pdfPageState: PdfPageTranslationState | null;
  sourceAnchor: ScrollAnchor | null;
  translationAnchor: ScrollAnchor | null;
  lastUsed: number;
};

const PDF_PREVIEW_CACHE = new Map<string, PdfPreviewCacheEntry>();
const PDF_PREVIEW_CACHE_LIMIT = 2;

function pdfPreviewCacheKey(jobId: string, targetLang: string | null | undefined) {
  return `${jobId}:${targetLang ?? "default"}`;
}

function getPdfPreviewCache(key: string) {
  const cached = PDF_PREVIEW_CACHE.get(key);
  if (!cached) return null;
  cached.lastUsed = Date.now();
  return cached;
}

function putPdfPreviewCache(key: string, entry: Omit<PdfPreviewCacheEntry, "lastUsed">) {
  PDF_PREVIEW_CACHE.set(key, { ...entry, lastUsed: Date.now() });
  while (PDF_PREVIEW_CACHE.size > PDF_PREVIEW_CACHE_LIMIT) {
    let oldestKey: string | null = null;
    let oldestSeen = Number.POSITIVE_INFINITY;
    for (const [candidateKey, candidate] of PDF_PREVIEW_CACHE) {
      if (candidate.lastUsed < oldestSeen) {
        oldestSeen = candidate.lastUsed;
        oldestKey = candidateKey;
      }
    }
    if (!oldestKey) break;
    PDF_PREVIEW_CACHE.delete(oldestKey);
  }
}

type PdfDocumentPreviewProps = {
  jobId: string;
  document: RosettaDocument;
  translationFile: RosettaTranslationFile | null;
  /// Translation progress fields lifted from WorkspacePage so we can render a
  /// live "Translating… X / N" placeholder without subscribing the whole
  /// store here.
  segmentCount: number;
  completedSegments: number;
  failedSegments: number;
  /// True while a translation run is actively writing back to segments.
  isTranslating: boolean;
  /// Live phase+percent (+per-page progress) from the pdf2zh progress event,
  /// owned by WorkspacePage. `currentPage` / `totalPages` are 1-based and
  /// scoped to the filtered list of pages this run will translate (i.e.
  /// "3rd of 5 pages I asked for", not "page 7 of a 100-page document").
  pdfProgress?: {
    phase: string;
    percent: number | null;
    currentPage: number | null;
    totalPages: number | null;
  } | null;
  /// Error message from the last failed PDF generation, owned by WorkspacePage.
  pdfError?: string | null;
  selectedPages: number[];
  onPageCountChange: (count: number) => void;
  onSelectedPagesChange: (pages: number[]) => void;
};

/// Single rasterize width, used for every page regardless of pane size. We
/// used to track the container width with a ResizeObserver and re-rasterize
/// on > 10% changes, but that made every sidebar toggle / window resize
/// flash the entire stack as PNGs got re-fetched. The backend clamps to
/// MAX_TARGET_WIDTH (rasterize.rs) anyway, so just asking for that width
/// once gives the sharpest result the renderer will produce and lets CSS
/// scale it down to whatever the pane currently is.
const RASTER_WIDTH = 1800;

/// Side-by-side PDF preview. Both panes are rasterized server-side via
/// pdfium and shipped as PNG bytes per page. This component is a pure
/// display component — all PDF generation logic lives in WorkspacePage.
export function PdfDocumentPreview({
  jobId,
  document,
  translationFile,
  segmentCount,
  completedSegments,
  failedSegments,
  isTranslating,
  pdfProgress,
  pdfError,
  selectedPages,
  onPageCountChange,
  onSelectedPagesChange,
}: PdfDocumentPreviewProps) {
  const sourcePaneRef = useRef<PdfPaneHandle | null>(null);
  const translationPaneRef = useRef<PdfPaneHandle | null>(null);
  const scrollDriverRef = useRef<PreviewSide | null>(null);
  const scrollDriverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const targetLang = translationFile?.targetLang ?? document.targetLang;
  const cacheKey = pdfPreviewCacheKey(jobId, targetLang);
  const cachedPreview = getPdfPreviewCache(cacheKey);

  const [sourcePageCount, setSourcePageCount] = useState<number | null>(
    cachedPreview?.sourcePageCount ?? null,
  );
  const [pdfPageState, setPdfPageState] = useState<PdfPageTranslationState | null>(
    cachedPreview?.pdfPageState ?? null,
  );
  const sourcePageCountRef = useRef(sourcePageCount);
  const pdfPageStateRef = useRef(pdfPageState);

  useEffect(() => {
    sourcePageCountRef.current = sourcePageCount;
  }, [sourcePageCount]);

  useEffect(() => {
    pdfPageStateRef.current = pdfPageState;
  }, [pdfPageState]);

  const pagesByNumber = useMemo(() => {
    const pages = new Map<number, PdfPageTranslation>();
    for (const page of pdfPageState?.pages ?? []) {
      pages.set(page.pageNumber, page);
    }
    return pages;
  }, [pdfPageState?.pages]);

  const refreshPageState = useCallback(async () => {
    try {
      const state = await getRosettaPdfPageStatus(jobId, targetLang);
      setPdfPageState(state);
    } catch (error) {
      console.error("[pdf] failed to load page translation state", error);
    }
  }, [jobId, targetLang]);

  // Probe source pages whenever the job changes. The translated pane uses the
  // same page count because page-level translation can render mixed states
  // before a complete translated PDF exists.
  useEffect(() => {
    let cancelled = false;
    const cached = getPdfPreviewCache(cacheKey);
    setSourcePageCount(cached?.sourcePageCount ?? null);
    setPdfPageState(cached?.pdfPageState ?? null);

    (async () => {
      try {
        const srcPages = await countRosettaPdfPages(jobId, "source");
        if (cancelled) return;
        setSourcePageCount(srcPages);
        onPageCountChange(srcPages);
        onSelectedPagesChange(Array.from({ length: srcPages }, (_, index) => index + 1));
        void refreshPageState();
      } catch (error) {
        if (cancelled) return;
        console.error("[pdf] failed to probe PDF page counts for job", jobId, error);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [cacheKey, jobId, onPageCountChange, onSelectedPagesChange, refreshPageState]);

  useEffect(() => {
    return () => {
      putPdfPreviewCache(cacheKey, {
        sourcePageCount: sourcePageCountRef.current,
        pdfPageState: pdfPageStateRef.current,
        sourceAnchor: sourcePaneRef.current?.getScrollAnchor() ?? null,
        translationAnchor: translationPaneRef.current?.getScrollAnchor() ?? null,
      });
    };
  }, [cacheKey]);

  useEffect(() => {
    const cached = getPdfPreviewCache(cacheKey);
    if (!cached || sourcePageCount == null) return;
    const id = window.setTimeout(() => {
      if (cached.sourceAnchor) {
        sourcePaneRef.current?.scrollToPageAnchor(
          cached.sourceAnchor.pageIndex,
          cached.sourceAnchor.localOffsetRatio,
        );
      }
      if (cached.translationAnchor) {
        translationPaneRef.current?.scrollToPageAnchor(
          cached.translationAnchor.pageIndex,
          cached.translationAnchor.localOffsetRatio,
        );
      }
    }, 0);
    return () => window.clearTimeout(id);
  }, [cacheKey, sourcePageCount]);

  useEffect(() => {
    if (!jobId) return;
    let unlisten: (() => void) | null = null;
    let unmounted = false;
    listen<{ jobId: string; pageNumber: number; status: string }>(
      "rosetta-pdf-page-progress",
      (event) => {
        if (event.payload.jobId !== jobId) return;
        setPdfPageState((current) =>
          patchPdfPageState(current, {
            pageNumber: event.payload.pageNumber,
            sourcePageCount: sourcePageCountRef.current,
            status: event.payload.status,
            targetLang,
          }),
        );
      },
    ).then((fn) => {
      if (unmounted) fn();
      else unlisten = fn;
    }).catch(() => {});
    return () => {
      unmounted = true;
      unlisten?.();
    };
  }, [jobId, targetLang]);

  useEffect(() => {
    if (isTranslating) return;
    void refreshPageState();
  }, [isTranslating, refreshPageState]);

  useEffect(() => {
    return () => {
      if (scrollDriverTimeoutRef.current) clearTimeout(scrollDriverTimeoutRef.current);
    };
  }, []);

  // Page-anchor scroll sync: find the page row under the driving pane's
  // viewport top, then scroll the other pane so the SAME page number sits at
  // the same page-local offset. Whole-pane ratio sync (the previous approach)
  // drifts whenever the two panes' total heights differ — which is the normal
  // case here, since untranslated pages render as fixed-height placeholders
  // and PNGs load at different times.
  function syncScroll(side: PreviewSide) {
    if (scrollDriverRef.current !== null && scrollDriverRef.current !== side) return;
    const from =
      side === "source" ? sourcePaneRef.current : translationPaneRef.current;
    const to =
      side === "source" ? translationPaneRef.current : sourcePaneRef.current;
    const anchor = from?.getScrollAnchor();
    if (!anchor || !to) return;

    scrollDriverRef.current = side;
    if (scrollDriverTimeoutRef.current) clearTimeout(scrollDriverTimeoutRef.current);
    scrollDriverTimeoutRef.current = setTimeout(() => {
      scrollDriverRef.current = null;
    }, 150);
    to.scrollToPageAnchor(anchor.pageIndex, anchor.localOffsetRatio);
  }

  const extractionStatus = document.extractionStatus ?? "done";
  const pdfAlreadyTranslated = translationFile?.status === "translated";
  const translationComplete =
    segmentCount > 0 && completedSegments === segmentCount && failedSegments === 0;

  const pdf2zhProgressText = pdfProgress
    ? `${phaseLabel(pdfProgress.phase)}${
        pdfProgress.percent == null ? "" : ` ${pdfProgress.percent}%`
      }`
    : null;

  const translationPlaceholder = (() => {
    if (extractionStatus === "pending") return "PDF 正在解析，请稍候…";
    if (extractionStatus === "failed") return "PDF 解析失败，请重新导入。";
    if (isTranslating) return pdf2zhProgressText ?? "正在生成翻译后 PDF…";
    if (pdfError) return `生成失败：${pdfError}`;
    if (pdfAlreadyTranslated) return "正在加载译文 PDF…";
    if (segmentCount === 0) return "等待翻译。Rosetta 将保留 PDF 版面并生成译文 PDF。";
    if (translationComplete) return "等待生成翻译后 PDF…";
    if (completedSegments === 0)
      return `等待翻译。共 ${segmentCount} 段，点击「翻译全部」开始。`;
    return `翻译部分完成 (${completedSegments} / ${segmentCount})，继续翻译以生成完整译文 PDF。`;
  })();

  const translationPlaceholderLoading =
    extractionStatus === "pending" ||
    isTranslating ||
    pdfAlreadyTranslated;

  function togglePage(pageNumber: number, checked: boolean) {
    const next = checked
      ? [...selectedPages, pageNumber]
      : selectedPages.filter((page) => page !== pageNumber);
    const normalized = [...new Set(next)].sort((a, b) => a - b);
    onSelectedPagesChange(normalized);
  }

  function pageStatus(pageIndex: number) {
    const pageNumber = pageIndex + 1;
    return pagesByNumber.get(pageNumber) ?? null;
  }

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden py-0">
      <div className="grid min-h-0 flex-1 grid-cols-2">
        <div className="min-h-0 border-r">
          <PdfPane
            ref={sourcePaneRef}
            jobId={jobId}
            kind="source"
            pageCount={sourcePageCount}
            targetWidth={RASTER_WIDTH}
            placeholder="加载源 PDF…"
            onScroll={() => syncScroll("source")}
            pageActivity={(pageIndex) => pageStatus(pageIndex)?.status ?? null}
            pageControls={(pageIndex) => {
              const pageNumber = pageIndex + 1;
              return (
                <input
                  type="checkbox"
                  aria-label={`选择第 ${pageNumber} 页`}
                  checked={selectedPages.includes(pageNumber)}
                  disabled={isTranslating}
                  onChange={(event) => togglePage(pageNumber, event.target.checked)}
                />
              );
            }}
          />
        </div>
        <div className="min-h-0">
          <PdfPane
            ref={translationPaneRef}
            jobId={jobId}
            kind="translated"
            pageCount={sourcePageCount}
            pageRenderVersion={(pageIndex) =>
              translatedPageRenderVersion(pageIndex + 1, pageStatus(pageIndex))
            }
            targetWidth={RASTER_WIDTH}
            placeholder={translationPlaceholder}
            placeholderLoading={translationPlaceholderLoading}
            onScroll={() => syncScroll("translation")}
            canRenderPage={(pageIndex) => pageStatus(pageIndex)?.status === "translated"}
            pageActivity={(pageIndex) => pageStatus(pageIndex)?.status ?? null}
            renderPage={(pageIndex, width) =>
              renderRosettaPdfTranslatedPageAsPng(jobId, pageIndex + 1, width)
            }
            pageStatus={(pageIndex) => translatedPageLabel(pageIndex + 1, pageStatus(pageIndex))}
          />
        </div>
      </div>
    </Card>
  );
}

function patchPdfPageState(
  current: PdfPageTranslationState | null,
  update: {
    pageNumber: number;
    sourcePageCount: number | null;
    status: string;
    targetLang: string;
  },
): PdfPageTranslationState {
  const now = Date.now().toString();
  const existingPages = current?.pages ?? [];
  const pages = [...existingPages];
  const index = pages.findIndex((page) => page.pageNumber === update.pageNumber);
  const existing = index >= 0 ? pages[index] : null;
  const status = normalizePdfPageStatus(update.status);
  const nextPage: PdfPageTranslation = {
    pageNumber: update.pageNumber,
    status,
    translatedPdfPath:
      status === "translated"
        ? existing?.translatedPdfPath ??
          `pdf-pages/page-${String(update.pageNumber).padStart(4, "0")}.pdf`
        : existing?.translatedPdfPath ?? null,
    error: status === "failed" ? existing?.error ?? "可重试" : null,
    updatedAt: now,
  };

  if (index >= 0) {
    pages[index] = nextPage;
  } else {
    pages.push(nextPage);
  }
  pages.sort((left, right) => left.pageNumber - right.pageNumber);

  return {
    schemaVersion: current?.schemaVersion ?? 1,
    sourcePageCount:
      current?.sourcePageCount ?? update.sourcePageCount ?? Math.max(update.pageNumber, 1),
    targetLang: current?.targetLang ?? update.targetLang,
    pages,
  };
}

function normalizePdfPageStatus(status: string): PdfPageTranslation["status"] {
  if (
    status === "pending" ||
    status === "queued" ||
    status === "translating" ||
    status === "translated" ||
    status === "failed"
  ) {
    return status;
  }
  return "pending";
}

function translatedPageRenderVersion(
  pageNumber: number,
  page: { status: string; translatedPdfPath?: string | null; updatedAt?: string | null } | null,
) {
  if (page?.status !== "translated") return "pending";
  return `${pageNumber}:${page.translatedPdfPath ?? ""}:${page.updatedAt ?? "translated"}`;
}

function translatedPageLabel(
  pageNumber: number,
  page: { status: string; error?: string | null } | null,
) {
  if (!page) return `第 ${pageNumber} 页未翻译，导出时保留原文`;
  if (page.status === "translated") return `加载第 ${pageNumber} 页译文…`;
  if (page.status === "translating") return `第 ${pageNumber} 页翻译中…`;
  if (page.status === "queued") return `第 ${pageNumber} 页排队中…`;
  if (page.status === "failed") return `第 ${pageNumber} 页失败：${page.error ?? "可重试"}`;
  return `第 ${pageNumber} 页未翻译，导出时保留原文`;
}

function phaseLabel(phase: string) {
  switch (phase) {
    case "parse":
      return "正在分析版面...";
    case "translate":
      return "正在翻译...";
    case "render":
      return "正在生成 PDF...";
    default:
      return phase;
  }
}
