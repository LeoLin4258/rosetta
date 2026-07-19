import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useVirtualizer } from "@tanstack/react-virtual";

import { Card } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  countRosettaPdfPages,
  getRosettaPdfSnapshot,
  renderRosettaPdfTranslatedPageAsPng,
  renderRosettaPdfV3TranslatedPageAsPng,
  type PdfPageTranslation,
  type PdfPageTranslationState,
} from "@/lib/rosettaJobs";
import {
  defaultPdfSelectedPages,
  PDF_AUTO_SELECT_ALL_PAGE_LIMIT,
} from "@/lib/pdfPageSelectionPolicy";
import { cn } from "@/lib/utils";
import type {
  PdfV3PageControlStatus,
  PdfV3RunState,
  RosettaDocument,
  RosettaTranslationFile,
} from "../../types/rosetta";

import { pdfPreviewPaneWidth, pdfRasterTargetWidth } from "./pdfRasterSizing";
import { PdfPageImage } from "./PdfPane";
import { usePdfV3Preview } from "./usePdfV3Preview";

const PAGE_ASPECT_RATIO = 1.4142;
const PDF_PREVIEW_OVERSCAN_ROWS = 1;

type PdfProgress = {
  phase: string;
  percent: number | null;
  currentPage: number | null;
  totalPages: number | null;
  completedPages?: number | null;
  translatedChars?: number | null;
};

type PdfDocumentPreviewProps = {
  jobId: string;
  document: RosettaDocument;
  translationFile: RosettaTranslationFile | null;
  segmentCount: number;
  completedSegments: number;
  failedSegments: number;
  isTranslating: boolean;
  pdfProgress?: PdfProgress | null;
  pdfError?: string | null;
  activePages?: number[];
  selectedPages: number[];
  onPageCountChange: (count: number) => void;
  onSelectedPagesChange: (pages: number[]) => void;
};

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
  activePages = [],
  selectedPages,
  onPageCountChange,
  onSelectedPagesChange,
}: PdfDocumentPreviewProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [viewportWidth, setViewportWidth] = useState(0);
  const targetLang = translationFile?.targetLang ?? document.targetLang;

  const [sourcePageCount, setSourcePageCount] = useState<number | null>(null);
  const [pdfPageState, setPdfPageState] = useState<PdfPageTranslationState | null>(null);
  const [sourcePageImages, setSourcePageImages] = useState<Record<number, string>>({});
  const sourcePageCountRef = useRef(sourcePageCount);

  useLayoutEffect(() => {
    const node = scrollRef.current;
    if (!node) return;

    function updateWidth() {
      setViewportWidth(node?.clientWidth ?? 0);
    }

    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    sourcePageCountRef.current = sourcePageCount;
  }, [sourcePageCount]);

  const pages = useMemo(
    () =>
      sourcePageCount && sourcePageCount > 0
        ? Array.from({ length: sourcePageCount }, (_, i) => i)
        : [],
    [sourcePageCount],
  );

  const pagesByNumber = useMemo(() => {
    const pages = new Map<number, PdfPageTranslation>();
    for (const page of pdfPageState?.pages ?? []) {
      pages.set(page.pageNumber, page);
    }
    return pages;
  }, [pdfPageState?.pages]);

  const activePagesInRunOrder = useMemo(
    () => [...new Set(activePages)].sort((a, b) => a - b),
    [activePages],
  );

  const selectedPagesInRunOrder = useMemo(
    () => [...new Set(selectedPages)].sort((a, b) => a - b),
    [selectedPages],
  );

  const runPagesInOrder = useMemo(
    () =>
      activePagesInRunOrder.length > 0
        ? activePagesInRunOrder
        : selectedPagesInRunOrder,
    [activePagesInRunOrder, selectedPagesInRunOrder],
  );

  const currentTranslatingPageNumber = useMemo(() => {
    if (!isTranslating) return null;

    const explicitTranslatingPage = runPagesInOrder.find(
      (pageNumber) => pagesByNumber.get(pageNumber)?.status === "translating",
    );
    if (explicitTranslatingPage != null) return explicitTranslatingPage;

    const firstIncompletePage = runPagesInOrder.find((pageNumber) => {
      const status = pagesByNumber.get(pageNumber)?.status ?? null;
      return status !== "translated" && status !== "failed";
    });
    if (firstIncompletePage != null) return firstIncompletePage;

    if (!pdfProgress?.currentPage) return null;
    return runPagesInOrder[pdfProgress.currentPage - 1] ?? null;
  }, [
    isTranslating,
    pagesByNumber,
    pdfProgress?.currentPage,
    runPagesInOrder,
  ]);

  const activePageNumberSet = useMemo(
    () => new Set(runPagesInOrder),
    [runPagesInOrder],
  );
  const activeTranslationPageCount =
    runPagesInOrder.length;
  const stablePreviewMode =
    isTranslating &&
    activeTranslationPageCount > PDF_AUTO_SELECT_ALL_PAGE_LIMIT;

  const estimatedRowSize = useMemo(() => {
    const pageWidth = pdfPreviewPaneWidth(viewportWidth) || 240;
    return Math.ceil(pageWidth * PAGE_ASPECT_RATIO + 24);
  }, [viewportWidth]);

  const rasterTargetWidth = useMemo(() => {
    const devicePixelRatio =
      typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;
    return pdfRasterTargetWidth(viewportWidth, devicePixelRatio);
  }, [viewportWidth]);

  const renderTranslatedPdfPage = useCallback(
    (index: number, width: number) =>
      renderRosettaPdfTranslatedPageAsPng(jobId, index + 1, width, targetLang),
    [jobId, targetLang],
  );

  const virtualizer = useVirtualizer({
    count: pages.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => estimatedRowSize,
    overscan: PDF_PREVIEW_OVERSCAN_ROWS,
  });

  const virtualItems = virtualizer.getVirtualItems();
  const firstVisiblePageNumber =
    virtualItems[0]?.index != null ? virtualItems[0].index + 1 : 1;
  const pdfV3Preview = usePdfV3Preview({
    jobId,
    targetLanguage: targetLang,
    visiblePageNumber: firstVisiblePageNumber,
    isTranslating,
  });
  const pdfV3RunId = pdfV3Preview.run?.runId ?? null;

  const renderPdfV3TranslatedPage = useCallback(
    (index: number, width: number) => {
      if (!pdfV3RunId) {
        return Promise.reject(new Error("PDF v3 运行不可用。"));
      }
      return renderRosettaPdfV3TranslatedPageAsPng(
        jobId,
        pdfV3RunId,
        index + 1,
        width,
      );
    },
    [jobId, pdfV3RunId],
  );

  useEffect(() => {
    const pageCount = pdfV3Preview.run?.sourcePageCount ?? null;
    if (!pageCount) return;
    setSourcePageCount(pageCount);
    onPageCountChange(pageCount);
  }, [onPageCountChange, pdfV3Preview.run?.sourcePageCount]);

  const refreshPageState = useCallback(async () => {
    try {
      const snapshot = await getRosettaPdfSnapshot(jobId, targetLang);
      setPdfPageState(snapshot.pages);
      const totalPages = snapshot.summary.totalPages || snapshot.pages.sourcePageCount;
      if (totalPages > 0) {
        setSourcePageCount(totalPages);
        onPageCountChange(totalPages);
      }
    } catch (error) {
      console.error("[pdf] failed to load page translation state", error);
    }
  }, [jobId, onPageCountChange, targetLang]);

  useEffect(() => {
    let cancelled = false;
    setSourcePageCount(null);
    setPdfPageState(null);
    setSourcePageImages({});

    (async () => {
      try {
        const snapshot = await getRosettaPdfSnapshot(jobId, targetLang);
        if (cancelled) return;
        const srcPages = snapshot.summary.totalPages || snapshot.pages.sourcePageCount;
        setSourcePageCount(srcPages);
        setPdfPageState(snapshot.pages);
        onPageCountChange(srcPages);
        onSelectedPagesChange(defaultPdfSelectedPages(srcPages));
      } catch (error) {
        try {
          const srcPages = await countRosettaPdfPages(jobId, "source");
          if (cancelled) return;
          setSourcePageCount(srcPages);
          onPageCountChange(srcPages);
          onSelectedPagesChange(defaultPdfSelectedPages(srcPages));
          void refreshPageState();
          return;
        } catch {
          // Fall through to the visible console diagnostic below.
        }
        if (cancelled) return;
        console.error("[pdf] failed to probe PDF page counts for job", jobId, error);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [jobId, onPageCountChange, onSelectedPagesChange, refreshPageState, targetLang]);

  useEffect(() => {
    if (!jobId) return;
    let unlisten: (() => void) | null = null;
    let unmounted = false;

    listen<{
      jobId: string;
      targetLang?: string | null;
      runId?: string | null;
      pageNumber: number;
      status: string;
      resultKind?: PdfPageTranslation["resultKind"];
    }>(
      "rosetta-pdf-page-progress",
      (event) => {
        if (event.payload.jobId !== jobId) return;
        if (event.payload.targetLang && event.payload.targetLang !== targetLang) return;
        setPdfPageState((current) =>
          patchPdfPageState(current, {
            pageNumber: event.payload.pageNumber,
            sourcePageCount: sourcePageCountRef.current,
            status: event.payload.status,
            resultKind: event.payload.resultKind ?? null,
            targetLang,
            runId: event.payload.runId ?? null,
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
    if (extractionStatus === "pending") return "PDF 正在解析，请稍候...";
    if (extractionStatus === "failed") return "PDF 解析失败，请重新导入。";
    if (isTranslating) return pdf2zhProgressText ?? "正在生成翻译后 PDF...";
    if (pdfError) return `生成失败：${pdfError}`;
    if (pdfAlreadyTranslated) return "正在加载译文 PDF...";
    if (segmentCount === 0) return "等待翻译。Rosetta 将保留 PDF 版面并生成译文 PDF。";
    if (translationComplete) return "等待生成翻译后 PDF...";
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

  const handleSourcePageRendered = useCallback((pageIndex: number, src: string | null) => {
    setSourcePageImages((current) => {
      if (src && current[pageIndex] === src) return current;
      if (!src && !current[pageIndex]) return current;
      const next = { ...current };
      if (src) next[pageIndex] = src;
      else delete next[pageIndex];
      return next;
    });
  }, []);

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden rounded-none border-0 py-0">
      <ScrollArea className="h-full min-h-0 bg-muted/30" viewportRef={scrollRef}>
        {pages.length === 0 ? (
          <div className="flex min-h-full flex-col items-center justify-center gap-2 px-8 text-center text-sm text-muted-foreground">
            {translationPlaceholderLoading ? (
              <span className="rosetta-pdf-inline-progress-hide" aria-hidden="true" />
            ) : null}
            {sourcePageCount == null ? 
             <div className="h-40 flex items-center justify-center ">
              加载源 PDF...
             </div>
            : 
            translationPlaceholder}
          </div>
        ) : (
          <div
            className="relative w-full"
            style={{ height: `${virtualizer.getTotalSize()}px` }}
          >
            {virtualItems.map((item) => {
              const pageIndex = pages[item.index];
              const pageNumber = pageIndex + 1;
              const status = pageStatus(pageIndex);
              const pdfV3Page = pdfV3Preview.pagesByNumber.get(pageNumber) ?? null;
              const usePdfV3 = pdfV3Preview.run != null;
              const pdfV3PageRequested = usePdfV3
                ? pdfV3Preview.isPageRequested(pageNumber)
                : false;
              const legacyPreviewReady =
                !pdfV3Preview.isDiscovering && !pdfV3Preview.discoveryError;
              const sourcePageSrc = sourcePageImages[pageIndex] ?? null;
              const showSourceAsTranslation = usePdfV3
                ? pdfV3Page?.state.kind === "preserved"
                : status?.resultKind === "no_text";
              const activity = usePdfV3
                ? pdfV3PageActivity(
                    pdfV3Page,
                    pdfV3Preview.runState,
                    pdfV3PageRequested,
                  )
                : !legacyPreviewReady
                  ? "pending"
                  : displayPageActivity(
                      status?.status ?? null,
                      pageNumber,
                      currentTranslatingPageNumber,
                      activePageNumberSet,
                      isTranslating,
                    );
              const canRenderTranslation = usePdfV3
                ? showSourceAsTranslation
                  ? !!sourcePageSrc
                  : pdfV3Page?.state.kind === "completed"
                : legacyPreviewReady &&
                  (showSourceAsTranslation
                    ? !!sourcePageSrc
                    : status?.status === "translated" &&
                      !!status.translatedPdfPath &&
                      !stablePreviewMode);
              const translationStatus = usePdfV3
                ? pdfV3TranslatedPageLabel({
                    pageNumber,
                    page: pdfV3Page,
                    runState: pdfV3Preview.runState,
                    requested: pdfV3PageRequested,
                    error: pdfV3Preview.error,
                  })
                : legacyPreviewReady
                  ? translatedPageLabel(
                      pageNumber,
                      status,
                      activity,
                      stablePreviewMode,
                    )
                  : pdfV3Preview.discoveryError ??
                    "正在读取 PDF 翻译状态...";

              return (
                <div
                  key={`${jobId}-pdf-row-${pageIndex}`}
                  className="absolute left-0 top-0 w-full"
                  data-index={item.index}
                  data-pdf-page-row="true"
                  ref={virtualizer.measureElement}
                  style={{
                    transform: `translateY(${item.start}px)`,
                  }}
                >
                  <div
                    className={cn(
                      "grid min-w-0 grid-cols-[2rem_minmax(0,1fr)_minmax(0,1fr)] items-stretch gap-4 px-4 py-3",
                      pageIndex === 0 && "pt-4",
                      pageIndex === pages.length - 1 && "pb-4",
                    )}
                  >
                    <div className="flex items-center justify-center">
                      <input
                        type="checkbox"
                        aria-label={`选择第 ${pageNumber} 页`}
                        checked={selectedPages.includes(pageNumber)}
                        disabled={isTranslating}
                        onChange={(event) => togglePage(pageNumber, event.target.checked)}
                        className="size-3.5 rounded border-border accent-primary"
                      />
                    </div>

                    <div className="min-w-0">
                      <PdfPageImage
                        jobId={jobId}
                        kind="source"
                        pageIndex={pageIndex}
                        renderVersion={0}
                        targetWidth={rasterTargetWidth}
                        canRender
                        activity={
                          currentTranslatingPageNumber === pageNumber
                            ? "translating"
                            : null
                        }
                        onRendered={handleSourcePageRendered}
                      />
                    </div>

                    <div className="min-w-0">
                      <PdfPageImage
                        jobId={jobId}
                        kind="translated"
                        pageIndex={pageIndex}
                        renderVersion={
                          usePdfV3
                            ? pdfV3TranslatedPageRenderVersion(
                                pdfV3RunId,
                                pdfV3Page,
                              )
                            : translatedPageRenderVersion(pageNumber, status)
                        }
                        targetWidth={rasterTargetWidth}
                        canRender={canRenderTranslation}
                        activity={activity}
                        backdropSrc={showSourceAsTranslation ? null : sourcePageSrc}
                        staticSrc={showSourceAsTranslation ? sourcePageSrc : null}
                        imageAlt={
                          showSourceAsTranslation
                            ? `第 ${pageNumber} 页译文：保留原页`
                            : `第 ${pageNumber} 页译文`
                        }
                        renderPage={
                          usePdfV3
                            ? renderPdfV3TranslatedPage
                            : renderTranslatedPdfPage
                        }
                        status={translationStatus}
                      />
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </ScrollArea>
    </Card>
  );
}

function patchPdfPageState(
  current: PdfPageTranslationState | null,
  update: {
    pageNumber: number;
    sourcePageCount: number | null;
    status: string;
    resultKind: PdfPageTranslation["resultKind"] | null;
    targetLang: string;
    runId: string | null;
  },
): PdfPageTranslationState {
  const now = Date.now().toString();
  const existingPages = current?.pages ?? [];
  const pages = [...existingPages];
  const index = pages.findIndex((page) => page.pageNumber === update.pageNumber);
  const existing = index >= 0 ? pages[index] : null;
  const status = normalizePdfPageStatus(update.status);
  const resultKind =
    update.resultKind ??
    existing?.resultKind ??
    (status === "translated" ? "translated" : status === "failed" ? "failed" : null);
  const hasTranslatedArtifact = status === "translated" && resultKind !== "no_text";
  const nextPage: PdfPageTranslation = {
    pageNumber: update.pageNumber,
    status,
    resultKind,
    translatedPdfPath: hasTranslatedArtifact
      ? existing?.translatedPdfPath ?? pdfPageRelativePath(update.targetLang, update.pageNumber)
      : status === "translated"
        ? null
        : existing?.translatedPdfPath ?? null,
    sourceUnitCount: existing?.sourceUnitCount ?? null,
    translatedUnitCount: existing?.translatedUnitCount ?? null,
    sourceChars: existing?.sourceChars ?? null,
    translatedChars: existing?.translatedChars ?? null,
    artifactVersion: hasTranslatedArtifact ? existing?.artifactVersion ?? now : null,
    artifactCompression: hasTranslatedArtifact ? existing?.artifactCompression ?? "fast" : null,
    artifactBytes: hasTranslatedArtifact ? existing?.artifactBytes ?? null : null,
    artifactCompressionError: hasTranslatedArtifact
      ? existing?.artifactCompressionError ?? null
      : null,
    error: status === "failed" ? existing?.error ?? "可重试" : null,
    updatedAt: now,
    lastRunId: update.runId,
  };

  if (index >= 0) {
    pages[index] = nextPage;
  } else {
    pages.push(nextPage);
  }
  pages.sort((left, right) => left.pageNumber - right.pageNumber);

  return {
    schemaVersion: current?.schemaVersion ?? 2,
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
  page: {
    status: string;
    resultKind?: string | null;
    translatedPdfPath?: string | null;
    updatedAt?: string | null;
  } | null,
) {
  if (page?.status !== "translated") return "pending";
  if (page.resultKind === "no_text") return `${pageNumber}:no_text:${page.updatedAt ?? ""}`;
  return `${pageNumber}:${page.translatedPdfPath ?? ""}:${page.updatedAt ?? "translated"}`;
}

function pdfV3TranslatedPageRenderVersion(
  runId: string | null,
  page: PdfV3PageControlStatus | null,
) {
  if (!runId || page?.state.kind !== "completed") return "pending";
  return `${runId}:${page.state.patch.patchId}:${page.state.patch.translationRevision}:${page.updatedAtMs}`;
}

function pdfV3PageActivity(
  page: PdfV3PageControlStatus | null,
  runState: PdfV3RunState | null,
  requested: boolean,
) {
  if (!requested) return "pending";
  if (page?.state.kind === "failed") return "failed";
  if (
    page?.state.kind === "completed" ||
    page?.state.kind === "preserved"
  ) {
    return "translated";
  }
  if (page?.activeLease) return "translating";
  if (page?.state.kind === "extracted") return "queued";
  if (runState === "running" || runState === "cancelling") return "queued";
  return "pending";
}

function pdfV3TranslatedPageLabel({
  pageNumber,
  page,
  runState,
  requested,
  error,
}: {
  pageNumber: number;
  page: PdfV3PageControlStatus | null;
  runState: PdfV3RunState | null;
  requested: boolean;
  error: string | null;
}) {
  if (!requested) return `第 ${pageNumber} 页不在本次翻译范围内`;
  if (!page) return error ?? `正在读取第 ${pageNumber} 页状态...`;

  switch (page.state.kind) {
    case "completed":
      return `加载第 ${pageNumber} 页译文...`;
    case "preserved":
      return `第 ${pageNumber} 页无需替换文字，显示原页`;
    case "failed":
      return page.state.retryable
        ? `第 ${pageNumber} 页处理失败，可以重试`
        : `第 ${pageNumber} 页处理失败`;
    case "extracted":
      return page.activeLease?.stage === "translation"
        ? null
        : `第 ${pageNumber} 页已解析，等待翻译`;
    case "pending":
      if (page.activeLease) return null;
      if (runState === "paused") return "本次 PDF 翻译已暂停";
      if (runState === "cancelling") return "正在停止本次 PDF 翻译";
      if (runState === "cancelled") return "本次 PDF 翻译已停止";
      return `等待处理第 ${pageNumber} 页`;
  }
}

function displayPageActivity(
  status: PdfPageTranslation["status"] | null,
  pageNumber: number,
  currentTranslatingPageNumber: number | null,
  activePageNumberSet: ReadonlySet<number>,
  isTranslating: boolean,
) {
  if (status === "failed") return "failed";
  if (status === "translated") return "translated";
  if (currentTranslatingPageNumber === pageNumber) return "translating";
  if (status === "translating") return "translating";
  if (status === "queued") return "queued";
  if (isTranslating && activePageNumberSet.has(pageNumber)) return "queued";
  return "pending";
}

function translatedPageLabel(
  pageNumber: number,
  page: { status: string; resultKind?: string | null; error?: string | null } | null,
  activity: ReturnType<typeof displayPageActivity>,
  stablePreviewMode: boolean,
) {
  if (activity === "translating") return null;
  if (activity === "queued") return `等待第 ${pageNumber} 页译文`;
  if (!page) return null;
  if (page.status === "translated") {
    if (page.resultKind === "no_text") return `第 ${pageNumber} 页无可提取文本`;
    return stablePreviewMode
      ? `第 ${pageNumber} 页已完成，预览将在本次结束后加载`
      : `加载第 ${pageNumber} 页译文...`;
  }
  if (page.status === "failed") return `失败原因：${page.error ?? "可重试"}`;
  return null;
}

function pdfPageRelativePath(targetLang: string, pageNumber: number) {
  return `translated-pages/${pdfPageLanguageDir(targetLang)}/page-${String(pageNumber).padStart(4, "0")}.pdf`;
}

function pdfPageLanguageDir(targetLang: string) {
  const slug = targetLang
    .trim()
    .replace(/[^A-Za-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return slug || "unknown";
}

function phaseLabel(phase: string) {
  switch (phase) {
    case "parse":
      return "正在分析版面";
    case "translate":
      return "正在翻译";
    case "render":
      return "正在生成 PDF";
    default:
      return phase;
  }
}
