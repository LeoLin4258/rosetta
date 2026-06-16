import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { MutableRefObject } from "react";
import type React from "react";
import { Loader2 } from "lucide-react";

import { renderRosettaPdfPageAsPng } from "@/lib/rosettaJobs";

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
  /// Defeats the rasterize cache so "重新生成" produces a fresh render after
  /// the user clicks it. Bump this value when the underlying PDF bytes change.
  cacheKey?: string | number;
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

export function PdfPane({
  jobId,
  kind,
  cacheKey,
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
}: PdfPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!scrollRef) return;
    scrollRef.current = containerRef.current;
    return () => {
      if (scrollRef.current === containerRef.current) {
        scrollRef.current = null;
      }
    };
  }, [scrollRef]);

  // Render pages bottom-up into a vertical scrollable stack. Each PdfPageImage
  // independently fetches its own PNG on mount — we keep this loop trivial so
  // adding lazy/virtualized loading later (intersection observers, windowing)
  // doesn't require restructuring this component.
  const pages = useMemo(
    () =>
      pageCount && pageCount > 0
        ? Array.from({ length: pageCount }, (_, i) => i)
        : [],
    [pageCount],
  );

  if (!jobId || pages.length === 0) {
    return (
      <div
        ref={containerRef}
        className="flex h-full min-h-0 flex-col items-center justify-center gap-2 overflow-auto bg-background px-8 text-center text-sm text-muted-foreground"
      >
        {placeholderLoading && <Loader2 className="size-5 animate-spin" />}
        {placeholder ?? "等待 PDF…"}
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      onScroll={onScroll}
      className="flex h-full min-h-0 flex-col items-stretch gap-3 overflow-auto bg-muted/30 px-4 py-4"
    >
      {pages.map((pageIndex) => (
        <div
          key={`${jobId}-${kind}-${cacheKey ?? "v0"}-${pageIndex}`}
          className="flex min-w-0 items-start gap-3"
        >
          {/*
            The control gutter is rendered unconditionally so the two panes
            line up — the source pane fills it with a per-page checkbox while
            the translated pane leaves it empty. Without this, the source
            image area is ~44 px narrower (w-8 + gap-3), making the same
            translated PDF page visually larger than its source counterpart.
          */}
          <div className="sticky top-2 z-10 flex w-8 shrink-0 justify-center pt-2">
            {pageControls ? pageControls(pageIndex) : null}
          </div>
          <div className="min-w-0 flex-1">
            <PdfPageImage
              jobId={jobId}
              kind={kind}
              pageIndex={pageIndex}
              targetWidth={targetWidth}
              canRender={canRenderPage ? canRenderPage(pageIndex) : true}
              renderPage={renderPage}
              status={pageStatus?.(pageIndex)}
              activity={pageActivity?.(pageIndex) ?? null}
            />
          </div>
        </div>
      ))}
    </div>
  );
}

/// One page rendered as a PNG. Fetches bytes on mount, wraps them into an
/// object URL so the `<img>` can render without a base64 round-trip, and
/// revokes the URL on unmount to keep memory bounded as the user re-renders
/// (e.g., clicks "重新生成").
function PdfPageImage({
  jobId,
  kind,
  pageIndex,
  targetWidth,
  canRender,
  renderPage,
  status,
  activity,
}: {
  jobId: string;
  kind: "source" | "translated";
  pageIndex: number;
  targetWidth: number;
  canRender: boolean;
  renderPage?: (pageIndex: number, targetWidth: number) => Promise<Uint8Array>;
  status?: React.ReactNode;
  activity?: PdfPageActivity | null;
}) {
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let createdUrl: string | null = null;
    setSrc(null);
    setError(null);

    (async () => {
      try {
        if (!canRender) return;
        const bytes = renderPage
          ? await renderPage(pageIndex, targetWidth)
          : await renderRosettaPdfPageAsPng(jobId, kind, pageIndex, targetWidth);
        if (cancelled) return;
        // Slice to a private buffer so revoking our URL never disturbs the
        // caller's view of the Uint8Array.
        const buf = bytes.slice().buffer as ArrayBuffer;
        const blob = new Blob([buf], { type: "image/png" });
        createdUrl = URL.createObjectURL(blob);
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
      if (createdUrl) URL.revokeObjectURL(createdUrl);
    };
  }, [canRender, jobId, kind, pageIndex, renderPage, targetWidth]);

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
          <PdfPageSkeleton active={activity === "translating"} status={status} />
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
        {status ?? `加载第 ${pageIndex + 1} 页…`}
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

function PdfPageSkeleton({
  active,
  status,
}: {
  active: boolean;
  status?: React.ReactNode;
}) {
  return (
    <div
      className="rosetta-pdf-page-skeleton"
      data-active={active ? "true" : "false"}
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
