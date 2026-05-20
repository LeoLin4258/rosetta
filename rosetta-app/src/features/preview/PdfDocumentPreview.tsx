import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  countRosettaPdfPages,
  getRosettaPdfPageStatus,
  renderRosettaPdfTranslatedPageAsPng,
  type PdfPageTranslationState,
} from "@/lib/rosettaJobs";
import type {
  RosettaDocument,
  RosettaTranslationFile,
} from "../../types/rosetta";

import { PdfPane } from "./PdfPane";

type PreviewSide = "source" | "translation";

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
  /// Live phase+percent from the pdf2zh progress event, owned by WorkspacePage.
  pdfProgress?: { phase: string; percent: number | null } | null;
  /// Error message from the last failed PDF generation, owned by WorkspacePage.
  pdfError?: string | null;
  selectedPages: number[];
  forceRetranslate: boolean;
  onForceRetranslateChange: (force: boolean) => void;
  onPageCountChange: (count: number) => void;
  onSelectedPagesChange: (pages: number[]) => void;
};

/// Default rasterize width per pane. Backed by pdfium on the Rust side at
/// ~1.5x the source page width so retina screens stay crisp.
const DEFAULT_RASTER_WIDTH = 1200;

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
  forceRetranslate,
  onForceRetranslateChange,
  onPageCountChange,
  onSelectedPagesChange,
}: PdfDocumentPreviewProps) {
  const sourceScrollRef = useRef<HTMLDivElement | null>(null);
  const translationScrollRef = useRef<HTMLDivElement | null>(null);
  const scrollDriverRef = useRef<PreviewSide | null>(null);
  const scrollDriverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [sourcePageCount, setSourcePageCount] = useState<number | null>(null);
  const [paneWidth, setPaneWidth] = useState<number>(DEFAULT_RASTER_WIDTH);
  // Bumping forces PdfPane to re-fetch all pages — used after regeneration so
  // the user immediately sees the new translated PDF.
  const [translatedCacheKey, setTranslatedCacheKey] = useState(0);
  const [pdfPageState, setPdfPageState] = useState<PdfPageTranslationState | null>(null);
  const paneContainerRef = useRef<HTMLDivElement | null>(null);

  // Measure the pane container width so rasterization matches display.
  useLayoutEffect(() => {
    const el = paneContainerRef.current;
    if (!el) return;
    const measure = () => {
      const halfWidth = el.clientWidth / 2;
      const dpr = window.devicePixelRatio || 1;
      const want = Math.round(Math.max(400, halfWidth * dpr));
      setPaneWidth((prev) =>
        Math.abs(prev - want) > 32 ? want : prev,
      );
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const refreshPageState = useCallback(async () => {
    try {
      const state = await getRosettaPdfPageStatus(jobId, translationFile?.targetLang);
      setPdfPageState(state);
    } catch (error) {
      console.error("[pdf] failed to load page translation state", error);
    }
  }, [jobId, translationFile?.targetLang]);

  // Probe source pages whenever the job changes. The translated pane uses the
  // same page count because page-level translation can render mixed states
  // before a complete translated PDF exists.
  useEffect(() => {
    let cancelled = false;
    setSourcePageCount(null);

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
  }, [jobId, onPageCountChange, onSelectedPagesChange, refreshPageState]);

  useEffect(() => {
    if (!jobId) return;
    let unlisten: (() => void) | null = null;
    let unmounted = false;
    listen<{ jobId: string; pageNumber: number; status: string }>(
      "rosetta-pdf-page-progress",
      (event) => {
        if (event.payload.jobId !== jobId) return;
        void refreshPageState();
        setTranslatedCacheKey((key) => key + 1);
      },
    ).then((fn) => {
      if (unmounted) fn();
      else unlisten = fn;
    }).catch(() => {});
    return () => {
      unmounted = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jobId, refreshPageState]);

  useEffect(() => {
    return () => {
      if (scrollDriverTimeoutRef.current) clearTimeout(scrollDriverTimeoutRef.current);
    };
  }, []);

  function syncScroll(side: PreviewSide) {
    if (scrollDriverRef.current !== null && scrollDriverRef.current !== side) return;
    const from =
      side === "source" ? sourceScrollRef.current : translationScrollRef.current;
    const to =
      side === "source" ? translationScrollRef.current : sourceScrollRef.current;
    if (!from || !to) return;

    const maxFrom = from.scrollHeight - from.clientHeight;
    const maxTo = to.scrollHeight - to.clientHeight;
    const ratio = maxFrom > 0 ? from.scrollTop / maxFrom : 0;
    const targetScrollTop = ratio * Math.max(maxTo, 0);
    if (Math.abs(to.scrollTop - targetScrollTop) < 2) return;

    scrollDriverRef.current = side;
    if (scrollDriverTimeoutRef.current) clearTimeout(scrollDriverTimeoutRef.current);
    scrollDriverTimeoutRef.current = setTimeout(() => {
      scrollDriverRef.current = null;
    }, 150);
    to.scrollTop = targetScrollTop;
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

  function togglePage(pageNumber: number, checked: boolean) {
    const next = checked
      ? [...selectedPages, pageNumber]
      : selectedPages.filter((page) => page !== pageNumber);
    const normalized = [...new Set(next)].sort((a, b) => a - b);
    onSelectedPagesChange(normalized);
  }

  function pageStatus(pageIndex: number) {
    const pageNumber = pageIndex + 1;
    return pdfPageState?.pages.find((page) => page.pageNumber === pageNumber) ?? null;
  }

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden py-0">
      <div className="flex flex-wrap items-center gap-3 border-b bg-background px-4 py-2 text-xs">
        <span className="font-medium text-foreground">页面</span>
        <span className="text-muted-foreground">
          已选 {selectedPages.length} / {sourcePageCount ?? 0} 页
        </span>
        <Button
          size="sm"
          variant="ghost"
          className="h-7 px-2 text-xs"
          onClick={() => {
            if (!sourcePageCount) return;
            onSelectedPagesChange(Array.from({ length: sourcePageCount }, (_, index) => index + 1));
          }}
          disabled={isTranslating}
        >
          全选
        </Button>
        <Button
          size="sm"
          variant="ghost"
          className="h-7 px-2 text-xs"
          onClick={() => onSelectedPagesChange([])}
          disabled={isTranslating}
        >
          取消选择
        </Button>
        <label className="flex items-center gap-2 text-muted-foreground">
          <input
            type="checkbox"
            checked={forceRetranslate}
            onChange={(event) => onForceRetranslateChange(event.target.checked)}
            disabled={isTranslating}
          />
          强制重翻
        </label>
      </div>
      <div ref={paneContainerRef} className="grid min-h-0 flex-1 grid-cols-2">
        <div className="min-h-0 border-r">
          <PdfPane
            jobId={jobId}
            kind="source"
            pageCount={sourcePageCount}
            targetWidth={paneWidth}
            placeholder="加载源 PDF…"
            scrollRef={sourceScrollRef}
            onScroll={() => syncScroll("source")}
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
            jobId={jobId}
            kind="translated"
            cacheKey={translatedCacheKey}
            pageCount={sourcePageCount}
            targetWidth={paneWidth}
            placeholder={translationPlaceholder}
            scrollRef={translationScrollRef}
            onScroll={() => syncScroll("translation")}
            canRenderPage={(pageIndex) => pageStatus(pageIndex)?.status === "translated"}
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
