import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { MutableRefObject } from "react";
import type React from "react";

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
  /// Optional ref the caller wires up for scroll-sync. We forward our scroll
  /// container so the parent can mirror scrollTop between panes.
  scrollRef?: MutableRefObject<HTMLDivElement | null>;
  onScroll?: () => void;
  pageControls?: (pageIndex: number) => React.ReactNode;
  pageStatus?: (pageIndex: number) => React.ReactNode;
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
  scrollRef,
  onScroll,
  pageControls,
  pageStatus,
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
          {pageControls ? (
            <div className="sticky top-2 z-10 flex w-8 shrink-0 justify-center pt-2">
              {pageControls(pageIndex)}
            </div>
          ) : null}
          <div className="min-w-0 flex-1">
            <PdfPageImage
              jobId={jobId}
              kind={kind}
              pageIndex={pageIndex}
              targetWidth={targetWidth}
              canRender={canRenderPage ? canRenderPage(pageIndex) : true}
              renderPage={renderPage}
              status={pageStatus?.(pageIndex)}
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
}: {
  jobId: string;
  kind: "source" | "translated";
  pageIndex: number;
  targetWidth: number;
  canRender: boolean;
  renderPage?: (pageIndex: number, targetWidth: number) => Promise<Uint8Array>;
  status?: React.ReactNode;
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
    const placeholderHeight = Math.max(targetWidth, 200) * 1.41;
    return (
      <div
        className="flex items-center justify-center rounded border border-border bg-background px-4 text-center text-xs text-muted-foreground"
        style={{ minHeight: `${placeholderHeight}px` }}
      >
        {status ?? `第 ${pageIndex + 1} 页尚未翻译`}
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
    // Placeholder reserves visual space approximating an A4 aspect ratio so
    // the stack height doesn't snap as PNGs stream in. Backend clamps width
    // to MIN_TARGET_WIDTH=200, so we use that as a minimum here too.
    const placeholderHeight = Math.max(targetWidth, 200) * 1.41;
    return (
      <div
        className="flex items-center justify-center rounded border border-border bg-background text-xs text-muted-foreground"
        style={{ minHeight: `${placeholderHeight}px` }}
      >
        {status ?? `加载第 ${pageIndex + 1} 页…`}
      </div>
    );
  }

  return (
    <img
      src={src}
      alt={`第 ${pageIndex + 1} 页`}
      className="block w-full rounded border border-border bg-background shadow-sm"
      // Disable browser-level dragging so the user can pan / select scroll
      // freely without the PNG starting an HTML5 drag operation.
      draggable={false}
    />
  );
}
