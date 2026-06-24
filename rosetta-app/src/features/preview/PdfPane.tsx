import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { MutableRefObject } from "react";
import type React from "react";
import { AlertCircle, CheckCircle2, Clock3, Loader2 } from "lucide-react";
import { useVirtualizer } from "@tanstack/react-virtual";

import { ScrollArea } from "@/components/ui/scroll-area";
import { renderRosettaPdfPageAsPng } from "@/lib/rosettaJobs";
import { cn } from "@/lib/utils";

// Phase 2 PDF preview pivot: rasterize via pdfium server-side, display as
// `<img>`. Two approaches we tried first and abandoned:
//
//   1. pdfjs / react-pdf: works for arbitrary CJK PDFs, but renders garbage
//      glyphs for pdfium-render-generated translated PDFs. Root cause:
//      pdfium 0.9.1 emits one CIDFontType2 subset per page (10 subsets for
//      a 10-page output), and pdfjs's @font-face loader mishandles them.
//      The bytes on disk are correct (Preview / sips / Safari proper render
//      them cleanly) — it's purely a pdfjs limitation we can't fix from our
//      side.
//   2. <embed type="application/pdf"> on a Blob URL: WKWebView in Tauri's
//      app mode lacks the PDF plugin Safari proper uses. Pages "load" but
//      stay empty — no text, no images, just the framing chrome.
//
// Rasterizing via pdfium and shipping PNG bytes per page works for ANY PDF
// because we're using the same renderer Preview does. Tradeoff: no text
// selection in the preview pane (export PDF still retains everything).

type PdfPageActivity =
  | "pending"
  | "queued"
  | "translating"
  | "translated"
  | "failed";

type PdfPaneProps = {
  jobId: string | null;
  /// Which PDF on the backend to render. The two side-by-side panes pass
  /// "source" and "translated" respectively.
  kind: "source" | "translated";
  /// Per-page render version. Bump only the page whose underlying PDF bytes
  /// changed; other pages keep their mounted image and Blob URL.
  pageRenderVersion?: (pageIndex: number) => string | number;
  /// Pixel count to request per page. The backend clamps to a sane range.
  /// Pass the pane's container width here so rasterization matches display.
  targetWidth: number;
  /// Number of pages in the PDF. Drives placeholder allocation so the layout
  /// doesn't jump as pages stream in. Pass 0 / null until you know the count.
  pageCount: number | null;
  /// Placeholder text when `pageCount` is null or 0 (no PDF available yet).
  placeholder?: string;
  /// Show a spinner next to the placeholder text.
  placeholderLoading?: boolean;
  /// Optional ref the caller wires up for scroll-sync. We forward our scroll
  /// container so the parent can mirror scrollTop between panes.
  scrollRef?: MutableRefObject<HTMLDivElement | null>;
  onScroll?: () => void;
  pageControls?: (pageIndex: number) => React.ReactNode;
  pageStatus?: (pageIndex: number) => React.ReactNode;
  pageActivity?: (pageIndex: number) => PdfPageActivity | null;
  canRenderPage?: (pageIndex: number) => boolean;
  renderPage?: (pageIndex: number, targetWidth: number) => Promise<Uint8Array>;
};

export type PdfPaneHandle = {
  getScrollAnchor: () => { pageIndex: number; localOffsetRatio: number } | null;
  scrollToPageAnchor: (pageIndex: number, localOffsetRatio: number) => void;
};

type CachedPng = {
  url: string;
  lastUsed: number;
};

const PDF_PAGE_IMAGE_CACHE = new Map<string, CachedPng>();
const PDF_PAGE_IMAGE_CACHE_LIMIT = 96;

function getCachedPng(key: string) {
  const cached = PDF_PAGE_IMAGE_CACHE.get(key);
  if (!cached) return null;
  cached.lastUsed = Date.now();
  return cached.url;
}

function putCachedPng(key: string, url: string) {
  const previous = PDF_PAGE_IMAGE_CACHE.get(key);
  if (previous?.url === url) {
    previous.lastUsed = Date.now();
    return;
  }
  if (previous) URL.revokeObjectURL(previous.url);
  PDF_PAGE_IMAGE_CACHE.set(key, { url, lastUsed: Date.now() });

  while (PDF_PAGE_IMAGE_CACHE.size > PDF_PAGE_IMAGE_CACHE_LIMIT) {
    let oldestKey: string | null = null;
    let oldestSeen = Number.POSITIVE_INFINITY;
    for (const [entryKey, entry] of PDF_PAGE_IMAGE_CACHE) {
      if (entry.lastUsed < oldestSeen) {
        oldestSeen = entry.lastUsed;
        oldestKey = entryKey;
      }
    }
    if (!oldestKey) break;
    const oldest = PDF_PAGE_IMAGE_CACHE.get(oldestKey);
    if (oldest) URL.revokeObjectURL(oldest.url);
    PDF_PAGE_IMAGE_CACHE.delete(oldestKey);
  }
}

export const PdfPane = forwardRef<PdfPaneHandle, PdfPaneProps>(function PdfPane({
  jobId,
  kind,
  pageRenderVersion,
  targetWidth,
  pageCount,
  placeholder,
  placeholderLoading,
  scrollRef,
  onScroll,
  pageControls,
  pageStatus,
  pageActivity,
  canRenderPage,
  renderPage,
}, ref) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [viewportWidth, setViewportWidth] = useState(0);

  useLayoutEffect(() => {
    if (!scrollRef) return;
    scrollRef.current = containerRef.current;
    return () => {
      if (scrollRef.current === containerRef.current) {
        scrollRef.current = null;
      }
    };
  }, [scrollRef]);

  useLayoutEffect(() => {
    const node = containerRef.current;
    if (!node) return;

    function updateWidth() {
      setViewportWidth(node?.clientWidth ?? 0);
    }

    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const pages = useMemo(
    () =>
      pageCount && pageCount > 0
        ? Array.from({ length: pageCount }, (_, i) => i)
        : [],
    [pageCount],
  );

  const estimatedRowSize = useMemo(() => {
    const imageWidth = Math.max(viewportWidth - 76, 260);
    return Math.round(imageWidth * 1.41 + 12);
  }, [viewportWidth]);

  const virtualizer = useVirtualizer({
    count: pages.length,
    getScrollElement: () => containerRef.current,
    estimateSize: () => estimatedRowSize,
    overscan: 3,
  });

  useImperativeHandle(
    ref,
    () => ({
      getScrollAnchor() {
        const node = containerRef.current;
        if (!node) return null;
        const virtualItems = virtualizer.getVirtualItems();
        if (virtualItems.length === 0) return null;

        const scrollTop = node.scrollTop;
        const anchor =
          virtualItems.find((item) => item.start + item.size > scrollTop) ??
          virtualItems[0];
        const localOffsetRatio =
          anchor.size > 0
            ? Math.max(0, Math.min(1, (scrollTop - anchor.start) / anchor.size))
            : 0;
        return { pageIndex: anchor.index, localOffsetRatio };
      },
      scrollToPageAnchor(pageIndex, localOffsetRatio) {
        const node = containerRef.current;
        if (!node) return;
        const virtualItems = virtualizer.getVirtualItems();
        const mounted = virtualItems.find((item) => item.index === pageIndex);
        const rowSize = mounted?.size ?? estimatedRowSize;
        const rowStart = mounted?.start ?? pageIndex * estimatedRowSize;
        const nextScrollTop = Math.max(
          0,
          Math.min(
            rowStart + rowSize * localOffsetRatio,
            node.scrollHeight - node.clientHeight,
          ),
        );
        node.scrollTop = nextScrollTop;
      },
    }),
    [estimatedRowSize, virtualizer],
  );

  if (!jobId || pages.length === 0) {
    return (
      <ScrollArea
        className="h-full min-h-0 bg-background"
        viewportRef={containerRef}
      >
        <div className="flex min-h-full flex-col items-center justify-center gap-2 px-8 text-center text-sm text-muted-foreground">
          {placeholderLoading && <Loader2 className="size-5 animate-spin" />}
          {placeholder ?? "等待 PDF…"}
        </div>
      </ScrollArea>
    );
  }

  const virtualItems = virtualizer.getVirtualItems();

  return (
    <ScrollArea
      onScroll={onScroll}
      className="h-full min-h-0 bg-muted/30"
      viewportRef={containerRef}
    >
      <div
        className="relative w-full"
        style={{ height: `${virtualizer.getTotalSize()}px` }}
      >
        {virtualItems.map((item) => {
          const pageIndex = pages[item.index];
          return (
            <div
              key={`${jobId}-${kind}-${pageIndex}`}
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
                  "flex min-w-0 items-start gap-3 px-4 py-1.5",
                  pageIndex === 0 && "pt-4",
                  pageIndex === pages.length - 1 && "pb-4",
                )}
              >
                {/*
                  The control gutter is rendered unconditionally so the two panes
                  line up — the source pane fills it with a per-page checkbox while
                  the translated pane leaves it empty.
                */}
                <div className="sticky top-2 z-10 flex w-8 shrink-0 justify-center pt-2">
                  {pageControls ? pageControls(pageIndex) : null}
                </div>
                <div className="min-w-0 flex-1">
                  <PdfPageImage
                    jobId={jobId}
                    kind={kind}
                    pageIndex={pageIndex}
                    renderVersion={pageRenderVersion?.(pageIndex) ?? 0}
                    targetWidth={targetWidth}
                    canRender={canRenderPage ? canRenderPage(pageIndex) : true}
                    renderPage={renderPage}
                    status={pageStatus?.(pageIndex)}
                    activity={pageActivity?.(pageIndex) ?? null}
                  />
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </ScrollArea>
  );
});

/// One page rendered as a PNG. Fetches bytes on mount, wraps them into an
/// object URL so the `<img>` can render without a base64 round-trip. Blob URLs
/// are cached at module scope so returning to a recently viewed PDF can paint
/// immediately while the backend state refreshes.
function PdfPageImage({
  jobId,
  kind,
  pageIndex,
  renderVersion,
  targetWidth,
  canRender,
  renderPage,
  status,
  activity,
}: {
  jobId: string;
  kind: "source" | "translated";
  pageIndex: number;
  renderVersion: string | number;
  targetWidth: number;
  canRender: boolean;
  renderPage?: (pageIndex: number, targetWidth: number) => Promise<Uint8Array>;
  status?: React.ReactNode;
  activity?: PdfPageActivity | null;
}) {
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const cacheKey = `${jobId}:${kind}:${pageIndex}:${targetWidth}:${renderVersion}`;

  useEffect(() => {
    let cancelled = false;
    setError(null);

    (async () => {
      try {
        if (!canRender) {
          setSrc(null);
          return;
        }
        const cached = getCachedPng(cacheKey);
        if (cached) {
          setSrc(cached);
          return;
        }
        const bytes = renderPage
          ? await renderPage(pageIndex, targetWidth)
          : await renderRosettaPdfPageAsPng(jobId, kind, pageIndex, targetWidth);
        if (cancelled) return;
        // Slice to a private buffer so revoking our URL never disturbs the
        // caller's view of the Uint8Array.
        const buf = bytes.slice().buffer as ArrayBuffer;
        const blob = new Blob([buf], { type: "image/png" });
        const createdUrl = URL.createObjectURL(blob);
        putCachedPng(cacheKey, createdUrl);
        setSrc(createdUrl);
      } catch (err) {
        if (cancelled) return;
        setError(
          err instanceof Error ? err.message : String(err ?? "PDF 页渲染失败"),
        );
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [cacheKey, canRender, jobId, kind, pageIndex, renderPage, targetWidth]);

  if (!canRender) {
    return (
      <div
        className="rosetta-pdf-page-frame rosetta-pdf-page-frame--placeholder w-full"
        // A4 portrait aspect ratio (~1:1.41) keeps the placeholder height
        // matched to the actual pane width — much better than a fixed pixel
        // height tied to the raster width, which is now a constant 1800 px
        // and would otherwise reserve absurd vertical space.
        style={{ aspectRatio: "1 / 1.41" }}
      >
        {kind === "translated" ? (
          <PdfPagePlaceholder
            activity={activity}
            pageNumber={pageIndex + 1}
            status={status}
          />
        ) : (
          <div className="flex h-full items-center justify-center px-4 text-center text-xs text-muted-foreground">
            {status ?? `第 ${pageIndex + 1} 页尚未翻译`}
          </div>
        )}
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded border border-destructive/40 bg-destructive/5 px-3 py-4 text-center text-xs text-destructive">
        <AlertCircle className="mx-auto mb-2 size-4" />
        第 {pageIndex + 1} 页渲染失败：{error}
      </div>
    );
  }

  if (!src) {
    // Placeholder reserves an A4-ratio space so the stack height doesn't snap
    // as PNGs stream in. aspect-ratio: 1/1.41 keeps it pane-width sized — a
    // pixel-based height tied to the raster width would over-reserve since
    // the raster is rendered at MAX (1800 px) and CSS scales it down.
    return (
      <div
        className="flex w-full items-center justify-center rounded border border-border bg-background text-xs text-muted-foreground"
        style={{ aspectRatio: "1 / 1.41" }}
      >
        <span className="inline-flex items-center gap-2">
          <Loader2 className="size-3.5 animate-spin motion-reduce:animate-none" />
          {status ?? `加载第 ${pageIndex + 1} 页…`}
        </span>
      </div>
    );
  }

  return (
    <div className="rosetta-pdf-page-frame">
      <img
        src={src}
        alt={`第 ${pageIndex + 1} 页`}
        className="block w-full rounded border border-border bg-background shadow-sm"
        // Disable browser-level dragging so the user can pan / select scroll
        // freely without the PNG starting an HTML5 drag operation.
        draggable={false}
      />
      {kind === "source" && activity === "translating" ? (
        <div className="rosetta-pdf-page-scan" aria-hidden="true" />
      ) : null}
    </div>
  );
}

function PdfPagePlaceholder({
  activity,
  pageNumber,
  status,
}: {
  activity?: PdfPageActivity | null;
  pageNumber: number;
  status?: React.ReactNode;
}) {
  if (activity === "translating") {
    return <PdfPageSkeleton status={status} />;
  }

  const failed = activity === "failed";
  const queued = activity === "queued";
  return (
    <div
      className={cn(
        "flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-xs",
        failed ? "text-destructive" : "text-muted-foreground",
      )}
    >
      {failed ? (
        <AlertCircle className="size-4" />
      ) : queued ? (
        <Clock3 className="size-4" />
      ) : (
        <CheckCircle2 className="size-4 opacity-0" aria-hidden="true" />
      )}
      <div className="font-medium text-foreground">
        {failed
          ? `第 ${pageNumber} 页翻译失败`
          : queued
            ? `第 ${pageNumber} 页排队中`
            : `第 ${pageNumber} 页未翻译`}
      </div>
      <div className="max-w-52 leading-5">
        {status ?? (failed ? "可重试此页。" : "导出时保留原文。")}
      </div>
    </div>
  );
}

function PdfPageSkeleton({
  status,
}: {
  status?: React.ReactNode;
}) {
  return (
    <div
      className="rosetta-pdf-page-skeleton"
      data-active="true"
    >
      <div className="rosetta-pdf-skeleton-header">
        <span />
        <span />
      </div>
      <div className="rosetta-pdf-skeleton-title">
        <span />
        <span />
      </div>
      <div className="rosetta-pdf-skeleton-abstract">
        <span />
        <span />
        <span />
        <span />
      </div>
      <div className="rosetta-pdf-skeleton-columns">
        <div>
          <span />
          <span />
          <span />
          <span />
          <span />
        </div>
        <div>
          <span />
          <span />
          <span />
          <span />
        </div>
      </div>
      <div className="rosetta-pdf-skeleton-figure" />
      <div className="rosetta-pdf-skeleton-caption">
        <span />
        <span />
      </div>
      <div className="rosetta-pdf-skeleton-status">{status}</div>
    </div>
  );
}
