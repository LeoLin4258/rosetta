import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  countRosettaPdfPages,
  renderRosettaPdfV3TranslatedPageAsPng,
} from "@/lib/rosettaJobs";
import {
  defaultPdfSelectedPages,
} from "@/lib/pdfPageSelectionPolicy";
import { cn } from "@/lib/utils";
import type {
  PdfV3PageControlStatus,
  PdfV3RunControlStatus,
  PdfV3RunState,
  RosettaDocument,
} from "../../types/rosetta";

import { pdfPreviewPaneWidth, pdfRasterTargetWidth } from "./pdfRasterSizing";
import { PdfPageImage } from "./PdfPane";
import { usePdfV3Preview } from "./usePdfV3Preview";
import type { PdfV3RunOperation } from "../workspace/usePdfV3RunControl";

const PAGE_ASPECT_RATIO = 1.4142;
const PDF_PREVIEW_OVERSCAN_ROWS = 1;
const SOURCE_PAGE_IMAGE_STATE_LIMIT = 96;

type PdfDocumentPreviewProps = {
  jobId: string;
  document: RosettaDocument;
  isTranslating: boolean;
  pdfError?: string | null;
  pdfV3RunStatus: PdfV3RunControlStatus | null;
  pdfV3IsDiscovering: boolean;
  pdfV3DiscoveryError: string | null;
  pdfV3ControlOperation: PdfV3RunOperation | null;
  selectedPages: number[];
  onPageCountChange: (count: number) => void;
  onSelectedPagesChange: (pages: number[]) => void;
  onRetryPdfV3Page: (pageNumber: number) => void;
};

export function PdfDocumentPreview({
  jobId,
  document,
  isTranslating,
  pdfError,
  pdfV3RunStatus,
  pdfV3IsDiscovering,
  pdfV3DiscoveryError,
  pdfV3ControlOperation,
  selectedPages,
  onPageCountChange,
  onSelectedPagesChange,
  onRetryPdfV3Page,
}: PdfDocumentPreviewProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [viewportWidth, setViewportWidth] = useState(0);
  const [sourcePageCount, setSourcePageCount] = useState<number | null>(null);
  const [sourcePageImages, setSourcePageImages] = useState<Record<number, string>>({});

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

  const pageCount = sourcePageCount ?? 0;

  const estimatedRowSize = useMemo(() => {
    const pageWidth = pdfPreviewPaneWidth(viewportWidth) || 240;
    return Math.ceil(pageWidth * PAGE_ASPECT_RATIO + 24);
  }, [viewportWidth]);

  const rasterTargetWidth = useMemo(() => {
    const devicePixelRatio =
      typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;
    return pdfRasterTargetWidth(viewportWidth, devicePixelRatio);
  }, [viewportWidth]);

  const virtualizer = useVirtualizer({
    count: pageCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => estimatedRowSize,
    overscan: PDF_PREVIEW_OVERSCAN_ROWS,
  });

  const virtualItems = virtualizer.getVirtualItems();
  const firstVisiblePageNumber =
    virtualItems[0]?.index != null ? virtualItems[0].index + 1 : 1;
  const pdfV3Preview = usePdfV3Preview({
    jobId,
    runStatus: pdfV3RunStatus,
    visiblePageNumber: firstVisiblePageNumber,
    isDiscovering: pdfV3IsDiscovering,
    discoveryError: pdfV3DiscoveryError,
  });
  const pdfV3RunId = pdfV3Preview.run?.runId ?? null;
  const pdfV3SelectionLocked =
    pdfV3RunStatus != null &&
    pdfV3RunStatus.state !== "cancelled" &&
    pdfV3RunStatus.state !== "completed";

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

  useEffect(() => {
    let cancelled = false;
    setSourcePageCount(null);
    setSourcePageImages({});

    void countRosettaPdfPages(jobId, "source")
      .then((pageCount) => {
        if (cancelled) return;
        setSourcePageCount(pageCount);
        onPageCountChange(pageCount);
        onSelectedPagesChange(defaultPdfSelectedPages(pageCount));
      })
      .catch((error) => {
        if (!cancelled) {
          console.error("[pdf-v3] failed to read source page count", error);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [jobId, onPageCountChange, onSelectedPagesChange]);

  const extractionStatus = document.extractionStatus ?? "done";

  const translationPlaceholder = (() => {
    if (extractionStatus === "pending") return "PDF 正在解析，请稍候...";
    if (extractionStatus === "failed") return "PDF 解析失败，请重新导入。";
    if (pdfV3IsDiscovering) return "正在读取 PDF v3 运行状态...";
    if (pdfError) return `PDF v3：${pdfError}`;
    if (isTranslating) return "PDF v3 正在按页处理...";
    if (pdfV3RunStatus?.state === "paused") return "PDF v3 翻译已暂停。";
    return "等待翻译。选择页面后即可创建新的 PDF v3 运行。";
  })();

  const translationPlaceholderLoading =
    extractionStatus === "pending" ||
    pdfV3IsDiscovering ||
    isTranslating;

  function togglePage(pageNumber: number, checked: boolean) {
    const next = checked
      ? [...selectedPages, pageNumber]
      : selectedPages.filter((page) => page !== pageNumber);
    const normalized = [...new Set(next)].sort((a, b) => a - b);
    onSelectedPagesChange(normalized);
  }

  const handleSourcePageRendered = useCallback((pageIndex: number, src: string | null) => {
    setSourcePageImages((current) => {
      if (src && current[pageIndex] === src) return current;
      if (!src && !current[pageIndex]) return current;
      const next = { ...current };
      if (src) next[pageIndex] = src;
      else delete next[pageIndex];
      const retainedPageIndexes = Object.keys(next).map(Number);
      if (retainedPageIndexes.length > SOURCE_PAGE_IMAGE_STATE_LIMIT) {
        retainedPageIndexes
          .sort(
            (left, right) =>
              Math.abs(right - pageIndex) - Math.abs(left - pageIndex),
          )
          .slice(0, retainedPageIndexes.length - SOURCE_PAGE_IMAGE_STATE_LIMIT)
          .forEach((index) => delete next[index]);
      }
      return next;
    });
  }, []);

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden rounded-none border-0 py-0">
      <ScrollArea className="h-full min-h-0 bg-muted/30" viewportRef={scrollRef}>
        {pageCount === 0 ? (
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
              const pageIndex = item.index;
              const pageNumber = pageIndex + 1;
              const pdfV3Page = pdfV3Preview.pagesByNumber.get(pageNumber) ?? null;
              const hasPdfV3Run = pdfV3Preview.run != null;
              const pdfV3PageRequested = hasPdfV3Run
                ? pdfV3Preview.isPageRequested(pageNumber)
                : false;
              const sourcePageSrc = sourcePageImages[pageIndex] ?? null;
              const showSourceAsTranslation =
                pdfV3Page?.state.kind === "preserved";
              const activity = hasPdfV3Run
                ? pdfV3PageActivity(
                    pdfV3Page,
                    pdfV3Preview.runState,
                    pdfV3PageRequested,
                  )
                : "pending";
              const canRenderTranslation = hasPdfV3Run
                ? showSourceAsTranslation
                  ? !!sourcePageSrc
                  : pdfV3Page?.state.kind === "completed"
                : false;
              const translationStatus = hasPdfV3Run
                ? pdfV3TranslatedPageLabel({
                    pageNumber,
                    page: pdfV3Page,
                    runState: pdfV3Preview.runState,
                    requested: pdfV3PageRequested,
                    error: pdfV3Preview.error,
                  })
                : pdfV3Preview.discoveryError ??
                  (pdfV3Preview.isDiscovering
                    ? "正在读取 PDF v3 运行状态..."
                    : "等待创建 PDF v3 运行");
              const retryAction =
                hasPdfV3Run &&
                pdfV3Page?.state.kind === "failed" &&
                pdfV3Page.state.retryable &&
                pdfV3Preview.run?.ownedByCurrentSession ? (
                  <Button
                    type="button"
                    size="xs"
                    variant="outline"
                    disabled={pdfV3ControlOperation != null}
                    onClick={() => onRetryPdfV3Page(pageNumber)}
                  >
                    <RefreshCw className="size-3" />
                    重试此页
                  </Button>
                ) : null;

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
                      pageIndex === pageCount - 1 && "pb-4",
                    )}
                  >
                    <div className="flex items-center justify-center">
                      <input
                        type="checkbox"
                        aria-label={`选择第 ${pageNumber} 页`}
                        checked={
                          pdfV3SelectionLocked
                            ? pdfV3Preview.isPageRequested(pageNumber)
                            : selectedPages.includes(pageNumber)
                        }
                        disabled={isTranslating || pdfV3SelectionLocked}
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
                        activity={pdfV3Page?.activeLease ? "translating" : null}
                        onRendered={handleSourcePageRendered}
                      />
                    </div>

                    <div className="min-w-0">
                      <PdfPageImage
                        jobId={jobId}
                        kind="translated"
                        pageIndex={pageIndex}
                        renderVersion={
                          pdfV3TranslatedPageRenderVersion(
                            pdfV3RunId,
                            pdfV3Page,
                          )
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
                        renderPage={renderPdfV3TranslatedPage}
                        status={translationStatus}
                        action={retryAction}
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
