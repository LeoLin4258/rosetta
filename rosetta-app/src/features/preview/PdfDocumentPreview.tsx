import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useVirtualizer } from "@tanstack/react-virtual";

import { Card } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  countRosettaPdfPages,
  getRosettaPdfSnapshot,
  renderRosettaPdfTranslatedPageAsPng,
  type PdfPageTranslation,
  type PdfPageTranslationState,
} from "@/lib/rosettaJobs";
import { cn } from "@/lib/utils";
import type {
  RosettaDocument,
  RosettaTranslationFile,
} from "../../types/rosetta";

import { PdfPageImage } from "./PdfPane";

const RASTER_WIDTH = 1800;
const PAGE_ASPECT_RATIO = 1.4142;

type PdfProgress = {
  phase: string;
  percent: number | null;
  currentPage: number | null;
  totalPages: number | null;
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

  const currentTranslatingPageNumber = useMemo(() => {
    if (!isTranslating || !pdfProgress?.currentPage) return null;
    const runPages =
      activePagesInRunOrder.length > 0
        ? activePagesInRunOrder
        : selectedPagesInRunOrder;
    return runPages[pdfProgress.currentPage - 1] ?? null;
  }, [
    activePagesInRunOrder,
    isTranslating,
    pdfProgress?.currentPage,
    selectedPagesInRunOrder,
  ]);

  const estimatedRowSize = useMemo(() => {
    const horizontalPadding = 32;
    const checkboxColumn = 32;
    const gaps = 32;
    const pageWidth = Math.max(
      (viewportWidth - horizontalPadding - checkboxColumn - gaps) / 2,
      240,
    );
    return Math.ceil(pageWidth * PAGE_ASPECT_RATIO + 24);
  }, [viewportWidth]);

  const virtualizer = useVirtualizer({
    count: pages.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => estimatedRowSize,
    overscan: 3,
  });

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

    (async () => {
      try {
        const snapshot = await getRosettaPdfSnapshot(jobId, targetLang);
        if (cancelled) return;
        const srcPages = snapshot.summary.totalPages || snapshot.pages.sourcePageCount;
        setSourcePageCount(srcPages);
        setPdfPageState(snapshot.pages);
        onPageCountChange(srcPages);
        onSelectedPagesChange(Array.from({ length: srcPages }, (_, index) => index + 1));
      } catch (error) {
        try {
          const srcPages = await countRosettaPdfPages(jobId, "source");
          if (cancelled) return;
          setSourcePageCount(srcPages);
          onPageCountChange(srcPages);
          onSelectedPagesChange(Array.from({ length: srcPages }, (_, index) => index + 1));
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

  const virtualItems = virtualizer.getVirtualItems();

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden rounded-none border-0 py-0">
      <ScrollArea className="h-full min-h-0 bg-muted/30" viewportRef={scrollRef}>
        {pages.length === 0 ? (
          <div className="flex min-h-full flex-col items-center justify-center gap-2 px-8 text-center text-sm text-muted-foreground">
            {translationPlaceholderLoading ? (
              <span className="rosetta-pdf-inline-progress" aria-hidden="true" />
            ) : null}
            {sourcePageCount == null ? "加载源 PDF..." : translationPlaceholder}
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
              const activity = displayPageActivity(
                status?.status ?? null,
                pageNumber,
                currentTranslatingPageNumber,
              );

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
                        targetWidth={RASTER_WIDTH}
                        canRender
                        activity={status?.status ?? null}
                      />
                    </div>

                    <div className="min-w-0">
                      <PdfPageImage
                        jobId={jobId}
                        kind="translated"
                        pageIndex={pageIndex}
                        renderVersion={translatedPageRenderVersion(pageNumber, status)}
                        targetWidth={RASTER_WIDTH}
                        canRender={status?.status === "translated"}
                        activity={activity}
                        renderPage={(index, width) =>
                          renderRosettaPdfTranslatedPageAsPng(
                            jobId,
                            index + 1,
                            width,
                            targetLang,
                          )
                        }
                        status={translatedPageLabel(pageNumber, status, activity)}
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
  const nextPage: PdfPageTranslation = {
    pageNumber: update.pageNumber,
    status,
    translatedPdfPath:
      status === "translated"
        ? existing?.translatedPdfPath ??
          pdfPageRelativePath(update.targetLang, update.pageNumber)
        : existing?.translatedPdfPath ?? null,
    artifactVersion: status === "translated" ? existing?.artifactVersion ?? now : null,
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

function displayPageActivity(
  status: PdfPageTranslation["status"] | null,
  pageNumber: number,
  currentTranslatingPageNumber: number | null,
) {
  if (status === "failed") return "failed";
  if (status === "translated") return "translated";
  if (currentTranslatingPageNumber === pageNumber) return "translating";
  return "pending";
}

function translatedPageLabel(
  pageNumber: number,
  page: { status: string; error?: string | null } | null,
  activity: ReturnType<typeof displayPageActivity>,
) {
  if (!page) return null;
  if (page.status === "translated") return `加载第 ${pageNumber} 页译文...`;
  if (activity === "translating") return null;
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
