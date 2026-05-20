import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { languageLabel } from "@/lib/languages";
import {
  countRosettaPdfPages,
  getRosettaPdfAssets,
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
  /// Called when the user clicks "重新生成".
  onRegenerate?: () => void;
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
  onRegenerate,
}: PdfDocumentPreviewProps) {
  const sourceScrollRef = useRef<HTMLDivElement | null>(null);
  const translationScrollRef = useRef<HTMLDivElement | null>(null);
  const scrollDriverRef = useRef<PreviewSide | null>(null);
  const scrollDriverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [sourcePageCount, setSourcePageCount] = useState<number | null>(null);
  const [translatedPageCount, setTranslatedPageCount] = useState<number | null>(null);
  const [paneWidth, setPaneWidth] = useState<number>(DEFAULT_RASTER_WIDTH);
  // Bumping forces PdfPane to re-fetch all pages — used after regeneration so
  // the user immediately sees the new translated PDF.
  const [translatedCacheKey, setTranslatedCacheKey] = useState(0);
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

  // Probe both PDFs whenever the job changes.
  useEffect(() => {
    let cancelled = false;
    setSourcePageCount(null);
    setTranslatedPageCount(null);

    (async () => {
      try {
        const assets = await getRosettaPdfAssets(jobId);
        if (cancelled) return;
        const srcPages = await countRosettaPdfPages(jobId, "source");
        if (cancelled) return;
        setSourcePageCount(srcPages);
        if (assets.translatedPdf) {
          const tPages = await countRosettaPdfPages(jobId, "translated");
          if (cancelled) return;
          setTranslatedPageCount(tPages);
        } else {
          setTranslatedPageCount(0);
        }
      } catch (error) {
        if (cancelled) return;
        console.error("[pdf] failed to probe PDF page counts for job", jobId, error);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [jobId]);

  useEffect(() => {
    return () => {
      if (scrollDriverTimeoutRef.current) clearTimeout(scrollDriverTimeoutRef.current);
    };
  }, []);

  // Re-probe translated page count whenever the translation file is updated
  // (e.g. after WorkspacePage calls loadRosettaJob following a successful run).
  useEffect(() => {
    if (translationFile?.status !== "translated") return;
    void countRosettaPdfPages(jobId, "translated")
      .then((pages) => {
        setTranslatedPageCount(pages);
        setTranslatedCacheKey((key) => key + 1);
      })
      .catch(() => {});
  }, [jobId, translationFile?.status, translationFile?.updatedAt]);

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

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden py-0">
      <div className="grid grid-cols-2 border-b bg-muted/40 text-sm text-muted-foreground">
        <div className="border-r px-4 py-3">
          <span>原文 PDF</span>
          <span className="ml-2 text-xs">
            {document.filename}
          </span>
        </div>
        <div className="flex items-center justify-between gap-3 px-4 py-3">
          <div className="flex items-center gap-2">
            <span>译文 PDF</span>
            {translationFile ? (
              <Badge variant="outline">{languageLabel(translationFile.targetLang)}</Badge>
            ) : null}
          </div>
          {translationComplete || pdfAlreadyTranslated ? (
            <Button
              size="sm"
              variant="ghost"
              onClick={onRegenerate}
              disabled={isTranslating}
            >
              {isTranslating ? "生成中…" : "重新生成"}
            </Button>
          ) : null}
        </div>
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
          />
        </div>
        <div className="min-h-0">
          <PdfPane
            jobId={jobId}
            kind="translated"
            cacheKey={translatedCacheKey}
            pageCount={translatedPageCount}
            targetWidth={paneWidth}
            placeholder={translationPlaceholder}
            scrollRef={translationScrollRef}
            onScroll={() => syncScroll("translation")}
          />
        </div>
      </div>
    </Card>
  );
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
