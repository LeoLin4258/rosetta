import { useEffect, useState } from "react";
import type React from "react";
import { AlertCircle, Loader2 } from "lucide-react";

import { renderRosettaPdfPageAsPng } from "@/lib/rosettaJobs";
import { cn } from "@/lib/utils";

// PDF preview renders backend-rasterized pages as PNGs. This avoids the pdfjs
// font issues seen with pdfium-generated translated PDFs and keeps preview
// behavior identical for source and translated pages.

export type PdfPageActivity =
  | "pending"
  | "queued"
  | "translating"
  | "translated"
  | "failed";

type CachedPng = {
  url: string;
  lastUsed: number;
};

const PDF_PAGE_ASPECT_RATIO = "1 / 1.4142";
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

export function PdfPageImage({
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
        className="rosetta-pdf-page-frame rosetta-pdf-page-frame--placeholder h-full w-full"
        style={{ aspectRatio: PDF_PAGE_ASPECT_RATIO }}
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
      <div
        className="flex h-full w-full items-center justify-center rounded border border-destructive/40 bg-destructive/5 px-3 py-4 text-center text-xs text-destructive"
        style={{ aspectRatio: PDF_PAGE_ASPECT_RATIO }}
      >
        <div>
          <AlertCircle className="mx-auto mb-2 size-4" />
          第 {pageIndex + 1} 页渲染失败：{error}
        </div>
      </div>
    );
  }

  if (!src) {
    return (
      <div
        className="flex h-full w-full items-center justify-center rounded border border-border bg-background text-xs text-muted-foreground"
        style={{ aspectRatio: PDF_PAGE_ASPECT_RATIO }}
      >
        <span className="inline-flex items-center gap-2">
          <Loader2 className="size-3.5 animate-spin motion-reduce:animate-none" />
          {status ?? `加载第 ${pageIndex + 1} 页...`}
        </span>
      </div>
    );
  }

  return (
    <div className="rosetta-pdf-page-frame h-full w-full" style={{ aspectRatio: PDF_PAGE_ASPECT_RATIO }}>
      <img
        src={src}
        alt={`第 ${pageIndex + 1} 页`}
        className="block size-full rounded border border-border bg-background shadow-sm"
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
  const failed = activity === "failed";
  const translating = activity === "translating";

  return (
    <div
      className={cn(
        "flex h-full flex-col items-center justify-center gap-1.5 px-6 text-center text-xs",
        failed ? "text-destructive" : "text-muted-foreground",
      )}
    >
      {translating ? (
        <Loader2 className="mb-0.5 size-4 animate-spin motion-reduce:animate-none" />
      ) : null}
      <div className="font-medium text-foreground">
        {failed
          ? `第 ${pageNumber} 页翻译失败`
          : translating
            ? "翻译中"
            : "未翻译"}
      </div>
      {failed || status ? (
        <div className="max-w-52 leading-5">
          {status ?? "可重试此页。"}
        </div>
      ) : null}
    </div>
  );
}
