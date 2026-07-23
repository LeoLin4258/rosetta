import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type FocusEvent,
  type RefObject,
} from "react";
import { useVirtualizer, type Virtualizer } from "@tanstack/react-virtual";

import { ScrollArea } from "@/components/ui/scroll-area";
import type { PdfPageTranslation } from "@/lib/rosettaJobs";
import { cn } from "@/lib/utils";

import { PdfPageImage } from "./PdfPane";

const PAGE_ROW_HEIGHT = 34;
const PAGE_LIST_OVERSCAN = 8;
const EXPAND_DELAY_MS = 100;
const COLLAPSE_DELAY_MS = 180;
const HOVER_PREVIEW_DELAY_MS = 120;

type PdfPageMapProps = {
  jobId: string;
  pageCount: number;
  pagesByNumber: ReadonlyMap<number, PdfPageTranslation>;
  selectedPages: number[];
  currentPage: number;
  selectionDisabled: boolean;
  onNavigate: (pageNumber: number) => void;
  onSelectedPagesChange: (pages: number[]) => void;
};

export function PdfPageMap({
  jobId,
  pageCount,
  pagesByNumber,
  selectedPages,
  currentPage,
  selectionDisabled,
  onNavigate,
  onSelectedPagesChange,
}: PdfPageMapProps) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const selectionAnchorRef = useRef<number | null>(null);
  const expandTimerRef = useRef<number | null>(null);
  const collapseTimerRef = useRef<number | null>(null);
  const [isExpanded, setIsExpanded] = useState(false);
  const [inspectedPage, setInspectedPage] = useState(currentPage);
  const [previewPage, setPreviewPage] = useState(currentPage);
  const selectedPageSet = useMemo(() => new Set(selectedPages), [selectedPages]);

  const pageVirtualizer = useVirtualizer({
    count: pageCount,
    getScrollElement: () => listRef.current,
    estimateSize: () => PAGE_ROW_HEIGHT,
    overscan: PAGE_LIST_OVERSCAN,
  });

  const clearExpandTimer = useCallback(() => {
    if (expandTimerRef.current == null) return;
    window.clearTimeout(expandTimerRef.current);
    expandTimerRef.current = null;
  }, []);

  const clearCollapseTimer = useCallback(() => {
    if (collapseTimerRef.current == null) return;
    window.clearTimeout(collapseTimerRef.current);
    collapseTimerRef.current = null;
  }, []);

  const expand = useCallback(() => {
    clearCollapseTimer();
    if (isExpanded || expandTimerRef.current != null) return;
    expandTimerRef.current = window.setTimeout(() => {
      expandTimerRef.current = null;
      setInspectedPage(currentPage);
      setPreviewPage(currentPage);
      setIsExpanded(true);
    }, EXPAND_DELAY_MS);
  }, [clearCollapseTimer, currentPage, isExpanded]);

  const expandImmediately = useCallback(() => {
    clearExpandTimer();
    clearCollapseTimer();
    if (!isExpanded) {
      setInspectedPage(currentPage);
      setPreviewPage(currentPage);
      setIsExpanded(true);
    }
  }, [clearCollapseTimer, clearExpandTimer, currentPage, isExpanded]);

  const collapse = useCallback(() => {
    clearExpandTimer();
    if (!isExpanded || collapseTimerRef.current != null) return;
    collapseTimerRef.current = window.setTimeout(() => {
      collapseTimerRef.current = null;
      setIsExpanded(false);
    }, COLLAPSE_DELAY_MS);
  }, [clearExpandTimer, isExpanded]);

  useEffect(() => {
    return () => {
      clearExpandTimer();
      clearCollapseTimer();
    };
  }, [clearCollapseTimer, clearExpandTimer]);

  useEffect(() => {
    selectionAnchorRef.current = null;
    setIsExpanded(false);
    setInspectedPage(1);
    setPreviewPage(1);
  }, [jobId]);

  useEffect(() => {
    if (!isExpanded) return;
    const timeout = window.setTimeout(
      () => setPreviewPage(inspectedPage),
      HOVER_PREVIEW_DELAY_MS,
    );
    return () => window.clearTimeout(timeout);
  }, [inspectedPage, isExpanded]);

  useLayoutEffect(() => {
    if (!isExpanded) return;
    pageVirtualizer.measure();
    pageVirtualizer.scrollToIndex(Math.max(0, currentPage - 1), {
      align: "auto",
    });
  }, [currentPage, isExpanded, pageVirtualizer]);

  function updateSelection(
    pageNumber: number,
    checked: boolean,
    extendRange: boolean,
  ) {
    if (selectionDisabled) return;
    const next = new Set(selectedPages);
    const anchor = selectionAnchorRef.current;
    if (extendRange && anchor != null) {
      const start = Math.min(anchor, pageNumber);
      const end = Math.max(anchor, pageNumber);
      for (let page = start; page <= end; page += 1) {
        if (checked) next.add(page);
        else next.delete(page);
      }
    } else if (checked) {
      next.add(pageNumber);
    } else {
      next.delete(pageNumber);
    }
    selectionAnchorRef.current = pageNumber;
    onSelectedPagesChange([...next].sort((left, right) => left - right));
  }

  function handleCheckboxChange(
    pageNumber: number,
    event: ChangeEvent<HTMLInputElement>,
  ) {
    const nativeEvent = event.nativeEvent as MouseEvent;
    updateSelection(pageNumber, event.target.checked, nativeEvent.shiftKey);
  }

  function handleBlur(event: FocusEvent<HTMLElement>) {
    if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
    collapse();
  }

  return (
    <aside
      className={cn(
        "relative z-20 flex shrink-0 outline-none transition-[width,background-color] duration-200 ease-[cubic-bezier(0.22,1,0.36,1)] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-blue-500/50 motion-reduce:transition-none",
        isExpanded
          ? "w-[min(26rem,48vw)] border-r border-border/50 bg-background"
          : "w-9 bg-muted/30",
      )}
      aria-label="PDF 页面导航与选择"
      tabIndex={0}
      onPointerEnter={expand}
      onPointerLeave={collapse}
      onFocusCapture={expandImmediately}
      onBlurCapture={handleBlur}
    >
      {isExpanded ? (
        <ExpandedPagePanel
          jobId={jobId}
          pageCount={pageCount}
          pagesByNumber={pagesByNumber}
          selectedPageSet={selectedPageSet}
          currentPage={currentPage}
          inspectedPage={inspectedPage}
          previewPage={previewPage}
          selectionDisabled={selectionDisabled}
          listRef={listRef}
          virtualizer={pageVirtualizer}
          onInspect={setInspectedPage}
          onNavigate={onNavigate}
          onCheckboxChange={handleCheckboxChange}
        />
      ) : (
        <CollapsedPageMap
          pageCount={pageCount}
          pagesByNumber={pagesByNumber}
          currentPage={currentPage}
        />
      )}
    </aside>
  );
}

function CollapsedPageMap({
  pageCount,
  pagesByNumber,
  currentPage,
}: {
  pageCount: number;
  pagesByNumber: ReadonlyMap<number, PdfPageTranslation>;
  currentPage: number;
}) {
  return (
    <div className="relative min-h-0 flex-1" aria-hidden="true">
      <span className="pointer-events-none absolute inset-x-0 inset-y-5 mx-auto w-px translate-x-2 overflow-hidden bg-foreground/10">
        {Array.from({ length: pageCount }, (_, index) => {
          const pageNumber = index + 1;
          return (
            <span
              key={pageNumber}
              className={cn(
                "absolute left-0 right-0 min-h-px",
                collapsedPageStatusClass(
                  pagesByNumber.get(pageNumber)?.status,
                ),
              )}
              style={{
                top: `${(index / pageCount) * 100}%`,
                height: `${Math.max(100 / pageCount, 0.12)}%`,
              }}
            />
          );
        })}
      </span>
      <span
        className="pointer-events-none absolute inset-x-0 z-10 mx-auto size-2 translate-x-2 -translate-y-1/2 rounded-full bg-blue-500 ring-2 ring-muted transition-[top] duration-200 ease-[cubic-bezier(0.22,1,0.36,1)] motion-reduce:transition-none dark:bg-blue-400"
        style={{
          top: `${Math.max(1.5, Math.min(98.5, ((currentPage - 0.5) / pageCount) * 100))}%`,
        }}
      />
    </div>
  );
}

function ExpandedPagePanel({
  jobId,
  pageCount,
  pagesByNumber,
  selectedPageSet,
  currentPage,
  inspectedPage,
  previewPage,
  selectionDisabled,
  listRef,
  virtualizer,
  onInspect,
  onNavigate,
  onCheckboxChange,
}: {
  jobId: string;
  pageCount: number;
  pagesByNumber: ReadonlyMap<number, PdfPageTranslation>;
  selectedPageSet: ReadonlySet<number>;
  currentPage: number;
  inspectedPage: number;
  previewPage: number;
  selectionDisabled: boolean;
  listRef: RefObject<HTMLDivElement>;
  virtualizer: Virtualizer<HTMLDivElement, Element>;
  onInspect: (pageNumber: number) => void;
  onNavigate: (pageNumber: number) => void;
  onCheckboxChange: (
    pageNumber: number,
    event: ChangeEvent<HTMLInputElement>,
  ) => void;
}) {
  const previewPages = [previewPage - 1, previewPage, previewPage + 1];

  return (
    <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
      <div className="grid min-h-0 flex-1 grid-cols-[10rem_minmax(0,1fr)]">
        <ScrollArea
          viewportRef={listRef}
          className="min-h-0 border-r border-border/40 bg-muted/[0.08]"
          aria-label="PDF 页面列表"
        >
          <div
            className="relative w-full"
            style={{ height: `${virtualizer.getTotalSize()}px` }}
          >
            {virtualizer.getVirtualItems().map((item) => {
              const pageNumber = item.index + 1;
              return (
                <div
                  key={pageNumber}
                  className="absolute left-1.5 right-1.5 top-0 w-auto"
                  data-index={item.index}
                  ref={virtualizer.measureElement}
                  style={{ transform: `translateY(${item.start}px)` }}
                >
                  <ExpandedPageRow
                    pageNumber={pageNumber}
                    status={pagesByNumber.get(pageNumber)?.status ?? "pending"}
                    checked={selectedPageSet.has(pageNumber)}
                    current={currentPage === pageNumber}
                    inspected={inspectedPage === pageNumber}
                    disabled={selectionDisabled}
                    onInspect={onInspect}
                    onNavigate={onNavigate}
                    onCheckboxChange={onCheckboxChange}
                  />
                </div>
              );
            })}
          </div>
        </ScrollArea>

        <div className="min-w-0 overflow-hidden bg-muted/[0.025]">
          <div
            className="relative h-full min-h-0 overflow-hidden"
            style={{
              WebkitMaskImage:
                "linear-gradient(to bottom, transparent 0, black 3rem, black calc(100% - 3rem), transparent 100%)",
              maskImage:
                "linear-gradient(to bottom, transparent 0, black 3rem, black calc(100% - 3rem), transparent 100%)",
            }}
          >
            <div className="absolute inset-x-3 top-1/2 flex -translate-y-1/2 flex-col gap-3">
              {previewPages.map((pageNumber) =>
                pageNumber >= 1 && pageNumber <= pageCount ? (
                  <PreviewPageThumbnail
                    key={`${jobId}-page-map-${pageNumber}`}
                    jobId={jobId}
                    pageNumber={pageNumber}
                    previewPage={previewPage}
                    onNavigate={onNavigate}
                  />
                ) : (
                  <div
                    key={`${jobId}-page-map-empty-${pageNumber}`}
                    className="aspect-[1/1.4142] w-full shrink-0"
                    aria-hidden="true"
                  />
                ),
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function ExpandedPageRow({
  pageNumber,
  status,
  checked,
  current,
  inspected,
  disabled,
  onInspect,
  onNavigate,
  onCheckboxChange,
}: {
  pageNumber: number;
  status: PdfPageTranslation["status"];
  checked: boolean;
  current: boolean;
  inspected: boolean;
  disabled: boolean;
  onInspect: (pageNumber: number) => void;
  onNavigate: (pageNumber: number) => void;
  onCheckboxChange: (
    pageNumber: number,
    event: ChangeEvent<HTMLInputElement>,
  ) => void;
}) {
  return (
    <div
      className={cn(
        "flex h-8 items-center gap-1.5 rounded-md px-1.5 transition-colors duration-150 hover:bg-muted/55 motion-reduce:transition-none",
        current && "bg-blue-500/[0.06] ring-1 ring-inset ring-blue-500/25",
        !current && inspected && "bg-muted/65",
      )}
      onPointerEnter={() => onInspect(pageNumber)}
      onFocusCapture={() => onInspect(pageNumber)}
    >
      <input
        type="checkbox"
        aria-label={`选择第 ${pageNumber} 页`}
        checked={checked}
        disabled={disabled}
        onClick={(event) => event.stopPropagation()}
        onChange={(event) => onCheckboxChange(pageNumber, event)}
        className="size-3.5 shrink-0 rounded border-border accent-primary"
      />
      <button
        type="button"
        aria-label={`转到第 ${pageNumber} 页，${pageStatusLabel(status)}`}
        aria-current={current ? "page" : undefined}
        onClick={() => onNavigate(pageNumber)}
        className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded px-1 text-left text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
      >
        <span
          className={cn("size-1.5 shrink-0 rounded-full", pageStatusClass(status))}
          aria-hidden="true"
        />
        <span className="min-w-0 flex-1 truncate tabular-nums">
          第 {pageNumber} 页
        </span>
      </button>
    </div>
  );
}

function PreviewPageThumbnail({
  jobId,
  pageNumber,
  previewPage,
  onNavigate,
}: {
  jobId: string;
  pageNumber: number;
  previewPage: number;
  onNavigate: (pageNumber: number) => void;
}) {
  const isPrimary = pageNumber === previewPage;

  return (
    <button
      type="button"
      aria-label={`转到第 ${pageNumber} 页`}
      onClick={() => onNavigate(pageNumber)}
      className={cn(
        "group flex w-full shrink-0 items-center justify-center rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/60",
        isPrimary ? "opacity-100" : "opacity-60 hover:opacity-90",
      )}
    >
      <span className="aspect-[1/1.4142] w-full overflow-hidden rounded-md bg-background">
        <PdfPageImage
          jobId={jobId}
          kind="source"
          pageIndex={pageNumber - 1}
          renderVersion={0}
          targetWidth={256}
          canRender
          imageAlt={`第 ${pageNumber} 页原文快速预览`}
        />
      </span>
    </button>
  );
}

function pageStatusClass(status?: PdfPageTranslation["status"]) {
  switch (status) {
    case "translated":
      return "bg-emerald-500/70";
    case "failed":
      return "bg-destructive/65";
    case "translating":
      return "bg-amber-500/70";
    case "queued":
      return "bg-primary/30";
    default:
      return "bg-muted-foreground/20";
  }
}

function collapsedPageStatusClass(status?: PdfPageTranslation["status"]) {
  switch (status) {
    case "translated":
      return "bg-emerald-500/50";
    case "failed":
      return "bg-destructive/55";
    case "translating":
      return "bg-amber-500/55";
    case "queued":
      return "bg-primary/25";
    default:
      return "bg-transparent";
  }
}

function pageStatusLabel(status?: PdfPageTranslation["status"]) {
  switch (status) {
    case "translated":
      return "已翻译";
    case "failed":
      return "翻译失败";
    case "translating":
      return "翻译中";
    case "queued":
      return "等待处理";
    default:
      return "未翻译";
  }
}
