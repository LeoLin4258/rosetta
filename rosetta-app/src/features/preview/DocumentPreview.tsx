import { useEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { languageLabel } from "@/lib/languages";
import {
  pdfMarkdownErrorMessage,
  readPdfMarkdownAsset,
  type PdfMarkdownComponentStatus,
  type PdfMarkdownExtractionStatus,
  type PdfMarkdownPreview,
  type PdfMarkdownRenderedBlock,
} from "@/lib/rosettaJobs";
import { cn } from "@/lib/utils";
import type {
  RosettaBlock,
  RosettaDocument,
  RosettaSourceDocumentFormat,
  RosettaSourceFile,
  RosettaTranslationFile,
  RosettaTranslationOutputFormat,
  Segment,
  TranslationSegment,
} from "../../types/rosetta";

import { PdfDocumentPreview } from "./PdfDocumentPreview";

type PreviewSide = "source" | "translation";

export function DocumentPreview({
  jobId = null,
  document,
  selectedOutputFormat,
  pdfMarkdownComponentStatus = null,
  pdfMarkdownExtractionStatus = null,
  pdfMarkdownPreview = null,
  pdfMarkdownPreviewError = null,
  hoveredBlockId,
  isTranslating = false,
  liveProgress,
  layout = "bilingual",
  onBlockHover,
  onBlockLeave,
  onToggleBlockSelection,
  selectedBlockIds = [],
  selectionEnabled = false,
  sourceFile,
  sourceSegments,
  translationFile,
  translationSegments,
  pdfProgress,
  pdfError,
  pdfActivePages = [],
  pdfSelectedPages = [],
  sourceEditing = false,
  sourceEditText = "",
  sourceEditSaving = false,
  sourceEditEnabled = false,
  onSourceEditCancel,
  onSourceEditChange,
  onSourceEditSave,
  onSourceEditStart,
  onPdfPageCountChange,
  onPdfCurrentPageChange,
  onPdfSelectedPagesChange,
  pdfCurrentPage = 1,
  pdfNavigationRequest = null,
}: {
  /// Required for PDF preview (needed to resolve `<job_dir>/source.pdf` and
  /// trigger translated-PDF generation). Other format paths don't use it; the
  /// standalone source/translation preview pages can omit it and fall back to
  /// the block-list rendering.
  jobId?: string | null;
  document: RosettaDocument | null;
  selectedOutputFormat?: RosettaTranslationOutputFormat;
  pdfMarkdownComponentStatus?: PdfMarkdownComponentStatus | null;
  pdfMarkdownExtractionStatus?: PdfMarkdownExtractionStatus | null;
  pdfMarkdownPreview?: PdfMarkdownPreview | null;
  pdfMarkdownPreviewError?: string | null;
  hoveredBlockId?: string | null;
  /// True while a translation run is actively writing segments. PDF preview
  /// uses this to differentiate "翻译中" from "等待翻译"; other formats ignore.
  isTranslating?: boolean;
  /// Live segment counts from `activeTranslationRun`. PDF preview needs the
  /// real-time progress for its right-pane placeholder; the persisted counts
  /// on `translationFile` only update after a run finishes.
  liveProgress?: { completed: number; total: number };
  layout?: "bilingual" | "source";
  onBlockHover?: (blockId: string) => void;
  onBlockLeave?: () => void;
  onToggleBlockSelection?: (blockIds: string[]) => void;
  selectedBlockIds?: string[];
  selectionEnabled?: boolean;
  sourceFile: RosettaSourceFile | null;
  sourceSegments: Segment[];
  translationFile: RosettaTranslationFile | null;
  translationSegments: TranslationSegment[];
  /// PDF-specific: live phase+percent (+per-page progress) from pdf2zh
  /// progress events. See WorkspacePage for shape rationale.
  pdfProgress?: {
    phase: string;
    percent: number | null;
    currentPage: number | null;
    totalPages: number | null;
    completedPages?: number | null;
  } | null;
  /// PDF-specific: error message from the last failed PDF generation.
  pdfError?: string | null;
  pdfActivePages?: number[];
  pdfSelectedPages?: number[];
  sourceEditing?: boolean;
  sourceEditText?: string;
  sourceEditSaving?: boolean;
  sourceEditEnabled?: boolean;
  onSourceEditCancel?: () => void;
  onSourceEditChange?: (value: string) => void;
  onSourceEditSave?: () => void;
  onSourceEditStart?: () => void;
  onPdfPageCountChange?: (count: number) => void;
  onPdfCurrentPageChange?: (pageNumber: number) => void;
  onPdfSelectedPagesChange?: (pages: number[]) => void;
  pdfCurrentPage?: number;
  pdfNavigationRequest?: { pageNumber: number; requestId: number } | null;
}) {
  // PDF documents get a dedicated react-pdf-based preview. The temporary
  // markdown-block fallback below is kept as the renderer for txt/md and as
  // the "block list / edit" view that Phase 3 will add a toggle for.
  if (
    document &&
    jobId &&
    document.format === "pdf" &&
    layout === "bilingual" &&
    selectedOutputFormat !== "markdown"
  ) {
    // During a live translation, the persisted `translationFile.completedSegments`
    // only updates after the run finishes — relying on it makes the right-pane
    // placeholder stay frozen at "0 / N" until completion. Switch to the live
    // counts from `liveProgress` (sourced from `activeTranslationRun` in
    // WorkspacePage) so the placeholder ticks up in real time.
    const liveCompleted = liveProgress?.completed ?? translationFile?.completedSegments ?? 0;
    const liveTotal =
      liveProgress?.total ?? translationFile?.segmentCount ?? sourceSegments.length;
    return (
      <PdfDocumentPreview
        jobId={jobId}
        document={document}
        translationFile={translationFile}
        segmentCount={liveTotal}
        completedSegments={liveCompleted}
        failedSegments={translationFile?.failedSegments ?? 0}
        isTranslating={isTranslating}
        pdfProgress={pdfProgress}
        pdfError={pdfError}
        activePages={pdfActivePages}
        selectedPages={pdfSelectedPages}
        currentPage={pdfCurrentPage}
        navigationRequest={pdfNavigationRequest}
        onPageCountChange={onPdfPageCountChange ?? (() => {})}
        onCurrentPageChange={onPdfCurrentPageChange ?? (() => {})}
        onSelectedPagesChange={onPdfSelectedPagesChange ?? (() => {})}
      />
    );
  }

  if (
    document &&
    jobId &&
    document.format === "pdf" &&
    layout === "bilingual" &&
    selectedOutputFormat === "markdown"
  ) {
    return (
      <PdfMarkdownDocumentPreview
        jobId={jobId}
        componentStatus={pdfMarkdownComponentStatus}
        extractionStatus={pdfMarkdownExtractionStatus}
        preview={pdfMarkdownPreview}
        previewError={pdfMarkdownPreviewError}
        hoveredBlockId={hoveredBlockId ?? null}
        isTranslating={isTranslating}
        onBlockHover={onBlockHover}
        onBlockLeave={onBlockLeave}
        onToggleBlockSelection={onToggleBlockSelection}
        selectedBlockIds={selectedBlockIds}
        selectionEnabled={selectionEnabled}
        sourceSegments={sourceSegments}
        translationFile={translationFile}
        translationSegments={translationSegments}
      />
    );
  }
  const sourceRef = useRef<HTMLDivElement>(null);
  const translationRef = useRef<HTMLDivElement>(null);
  const scrollDriverRef = useRef<PreviewSide | null>(null);
  const scrollDriverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (scrollDriverTimeoutRef.current) clearTimeout(scrollDriverTimeoutRef.current);
    };
  }, []);

  if (!document || !sourceFile) {
    return (
      <Card className="flex h-full min-h-0 py-0">
        <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
          选择一个源文件。
        </div>
      </Card>
    );
  }

  if (layout === "source") {
    return (
      <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden py-0">
        <SourcePaneHeader
          canEdit={sourceEditEnabled}
          editing={sourceEditing}
          saving={sourceEditSaving}
          onCancel={onSourceEditCancel}
          onEdit={onSourceEditStart}
          onSave={onSourceEditSave}
        />
        <div className="min-h-0 flex-1">
          {sourceEditing ? (
            <SourceEditPane
              value={sourceEditText}
              onChange={onSourceEditChange}
            />
          ) : (
            <PreviewPane
              document={document}
              file={sourceFile}
              hoveredBlockId={hoveredBlockId ?? null}
              onBlockHover={onBlockHover}
              onBlockLeave={onBlockLeave}
              onToggleBlockSelection={onToggleBlockSelection}
              onScroll={() => {}}
              paneRef={sourceRef}
              selectedBlockIds={selectedBlockIds}
              selectionEnabled={selectionEnabled}
              side="source"
              sourceSegments={sourceSegments}
              translationSegments={translationSegments}
              isTranslating={isTranslating}
            />
          )}
        </div>
      </Card>
    );
  }

  function syncScroll(side: PreviewSide) {
    // Ignore scroll events fired by the pane we just programmatically scrolled.
    if (scrollDriverRef.current !== null && scrollDriverRef.current !== side) return;

    const from = side === "source" ? sourceRef.current : translationRef.current;
    const to = side === "source" ? translationRef.current : sourceRef.current;
    if (!from || !to) return;

    const maxFrom = from.scrollHeight - from.clientHeight;
    const maxTo = to.scrollHeight - to.clientHeight;
    const ratio = maxFrom > 0 ? from.scrollTop / maxFrom : 0;
    const targetScrollTop = ratio * Math.max(maxTo, 0);

    // Dead-zone: skip tiny adjustments that the virtualizer triggers as it
    // re-measures items — these cause the 5-second tail of continued scrolling.
    if (Math.abs(to.scrollTop - targetScrollTop) < 2) return;

    // Mark this side as the scroll driver for 150 ms.  Any scroll events from
    // the other pane during that window are treated as programmatic echoes.
    scrollDriverRef.current = side;
    if (scrollDriverTimeoutRef.current) clearTimeout(scrollDriverTimeoutRef.current);
    scrollDriverTimeoutRef.current = setTimeout(() => {
      scrollDriverRef.current = null;
    }, 150);

    to.scrollTop = targetScrollTop;
  }

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden py-0">
      <div className="grid grid-cols-2 border-b bg-muted/40 text-sm text-muted-foreground">
        <SourcePaneHeader
          canEdit={sourceEditEnabled}
          editing={sourceEditing}
          saving={sourceEditSaving}
          onCancel={onSourceEditCancel}
          onEdit={onSourceEditStart}
          onSave={onSourceEditSave}
        />
        <div className="flex items-center justify-between gap-3 px-4 py-3">
          <span>译文</span>
          {translationFile ? (
            <Badge variant="outline">{languageLabel(translationFile.targetLang)}</Badge>
          ) : null}
        </div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-2">
        {sourceEditing ? (
          <SourceEditPane
            value={sourceEditText}
            onChange={onSourceEditChange}
          />
        ) : (
          <PreviewPane
            document={document}
            file={sourceFile}
            hoveredBlockId={hoveredBlockId ?? null}
            onBlockHover={onBlockHover}
            onBlockLeave={onBlockLeave}
            onToggleBlockSelection={onToggleBlockSelection}
            onScroll={() => syncScroll("source")}
            paneRef={sourceRef}
            selectedBlockIds={selectedBlockIds}
            selectionEnabled={selectionEnabled}
            side="source"
            sourceSegments={sourceSegments}
            translationSegments={translationSegments}
            isTranslating={isTranslating}
          />
        )}
        {translationFile ? (
          <PreviewPane
            document={document}
            file={sourceFile}
            hoveredBlockId={hoveredBlockId ?? null}
            onBlockHover={onBlockHover}
            onBlockLeave={onBlockLeave}
            onToggleBlockSelection={onToggleBlockSelection}
            onScroll={() => syncScroll("translation")}
            paneRef={translationRef}
            selectedBlockIds={selectedBlockIds}
            selectionEnabled={selectionEnabled}
            side="translation"
            sourceSegments={sourceSegments}
            translationSegments={translationSegments}
            isTranslating={isTranslating}
          />
        ) : (
          <div className="flex min-h-0 items-center justify-center bg-background px-8 text-center text-sm text-muted-foreground">
            选择或创建一个目标语言译文文件。
          </div>
        )}
      </div>
    </Card>
  );
}

function PdfMarkdownDocumentPreview({
  jobId,
  componentStatus,
  extractionStatus,
  preview,
  previewError,
  hoveredBlockId,
  isTranslating,
  onBlockHover,
  onBlockLeave,
  onToggleBlockSelection,
  selectedBlockIds,
  selectionEnabled,
  sourceSegments,
  translationFile,
  translationSegments,
}: {
  jobId: string;
  componentStatus: PdfMarkdownComponentStatus | null;
  extractionStatus: PdfMarkdownExtractionStatus | null;
  preview: PdfMarkdownPreview | null;
  previewError: string | null;
  hoveredBlockId: string | null;
  isTranslating: boolean;
  onBlockHover?: (blockId: string) => void;
  onBlockLeave?: () => void;
  onToggleBlockSelection?: (blockIds: string[]) => void;
  selectedBlockIds: string[];
  selectionEnabled: boolean;
  sourceSegments: Segment[];
  translationFile: RosettaTranslationFile | null;
  translationSegments: TranslationSegment[];
}) {
  const statusMessage = pdfMarkdownPreviewStatus(
    componentStatus,
    extractionStatus,
    previewError,
  );
  if (statusMessage || !preview) {
    return (
      <Card className="flex h-full min-h-0 items-center justify-center py-0">
        <div className="max-w-md px-8 text-center">
          <p className="text-sm font-medium text-foreground">
            {statusMessage?.title ?? "正在生成 Markdown 预览"}
          </p>
          {statusMessage?.detail ? (
            <p className="mt-1.5 text-xs leading-5 text-muted-foreground">
              {statusMessage.detail}
            </p>
          ) : null}
        </div>
      </Card>
    );
  }

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden py-0">
      <div className="grid grid-cols-2 border-b bg-muted/40 text-sm text-muted-foreground">
        <div className="border-r px-4 py-3">原文 Markdown</div>
        <div className="flex items-center justify-between gap-3 px-4 py-3">
          <span>译文 Markdown</span>
          {translationFile ? (
            <Badge variant="outline">
              {languageLabel(translationFile.targetLang)}
            </Badge>
          ) : null}
        </div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-2">
        <PdfMarkdownPane
          blocks={preview.sourceBlocks}
          hoveredBlockId={hoveredBlockId}
          isTranslating={isTranslating}
          jobId={jobId}
          onBlockHover={onBlockHover}
          onBlockLeave={onBlockLeave}
          onToggleBlockSelection={onToggleBlockSelection}
          selectedBlockIds={selectedBlockIds}
          selectionEnabled={selectionEnabled}
          side="source"
          sourceSegments={sourceSegments}
          translationSegments={translationSegments}
        />
        {translationFile && preview.translationBlocks ? (
          <PdfMarkdownPane
            blocks={preview.translationBlocks}
            hoveredBlockId={hoveredBlockId}
            isTranslating={isTranslating}
            jobId={jobId}
            onBlockHover={onBlockHover}
            onBlockLeave={onBlockLeave}
            onToggleBlockSelection={onToggleBlockSelection}
            selectedBlockIds={selectedBlockIds}
            selectionEnabled={selectionEnabled}
            side="translation"
            sourceSegments={sourceSegments}
            translationSegments={translationSegments}
          />
        ) : (
          <div className="flex min-h-0 items-center justify-center bg-background px-8 text-center text-sm text-muted-foreground">
            选择目标语言后开始翻译。
          </div>
        )}
      </div>
    </Card>
  );
}

function PdfMarkdownPane({
  blocks,
  hoveredBlockId,
  isTranslating,
  jobId,
  onBlockHover,
  onBlockLeave,
  onToggleBlockSelection,
  selectedBlockIds,
  selectionEnabled,
  side,
  sourceSegments,
  translationSegments,
}: {
  blocks: PdfMarkdownRenderedBlock[];
  hoveredBlockId: string | null;
  isTranslating: boolean;
  jobId: string;
  onBlockHover?: (blockId: string) => void;
  onBlockLeave?: () => void;
  onToggleBlockSelection?: (blockIds: string[]) => void;
  selectedBlockIds: string[];
  selectionEnabled: boolean;
  side: PreviewSide;
  sourceSegments: Segment[];
  translationSegments: TranslationSegment[];
}) {
  const paneRef = useRef<HTMLDivElement>(null);
  const segmentsByBlock = useMemo(
    () => groupSegmentsByBlock(sourceSegments),
    [sourceSegments],
  );
  const translationBySegmentId = useMemo(
    () =>
      new Map(
        translationSegments.map((segment) => [
          segment.sourceSegmentId,
          segment,
        ]),
      ),
    [translationSegments],
  );
  const markdownComponents = useMemo<Components>(
    () => ({
      img: (props) => (
        <PdfMarkdownImage
          jobId={jobId}
          src={typeof props.src === "string" ? props.src : undefined}
          alt={props.alt}
        />
      ),
    }),
    [jobId],
  );
  const virtualizer = useVirtualizer({
    count: blocks.length,
    getScrollElement: () => paneRef.current,
    estimateSize: () => 112,
    overscan: 8,
  });

  return (
    <ScrollArea
      className={cn("h-full min-h-0 bg-background", side === "source" && "border-r")}
      viewportRef={paneRef}
    >
      <div className="mx-auto max-w-(--rosetta-reader-max-width) px-6 py-6">
        <div
          className="relative w-full"
          style={{ height: `${virtualizer.getTotalSize()}px` }}
        >
          {virtualizer.getVirtualItems().map((item) => {
            const block = blocks[item.index];
            const groupSegments = block.blockIds.flatMap(
              (blockId) => segmentsByBlock.get(blockId) ?? [],
            );
            const activity = blockTranslationActivity(
              groupSegments,
              translationBySegmentId,
              isTranslating,
            );
            const selected = block.blockIds.some((id) =>
              selectedBlockIds.includes(id),
            );
            const selectable = selectionEnabled && groupSegments.length > 0;
            const primaryBlockId = block.blockIds[0] ?? null;
            const emptyTranslation =
              side === "translation" &&
              groupSegments.length > 0 &&
              groupSegments.some(
                (segment) =>
                  !translationBySegmentId
                    .get(segment.id)
                    ?.translatedText?.trim(),
              );

            return (
              <div
                className="absolute left-0 top-0 w-full"
                data-index={item.index}
                key={`${side}-${block.blockIds.join("-")}-${item.index}`}
                ref={virtualizer.measureElement}
                style={{ transform: `translateY(${item.start}px)` }}
              >
                <div
                  aria-pressed={selectable ? selected : undefined}
                  className={cn(
                    "relative rounded-md px-3 py-2 transition-colors",
                    selectable && "cursor-pointer",
                    primaryBlockId === hoveredBlockId && "bg-muted/60",
                    selected && "bg-primary/10 ring-1 ring-primary/25",
                    side === "source" &&
                      activity === "translating" &&
                      "rosetta-markdown-source-scanning",
                  )}
                  onClick={() => {
                    if (selectable) onToggleBlockSelection?.(block.blockIds);
                  }}
                  onKeyDown={(event) => {
                    if (!selectable || (event.key !== "Enter" && event.key !== " ")) return;
                    event.preventDefault();
                    onToggleBlockSelection?.(block.blockIds);
                  }}
                  onMouseEnter={() => {
                    if (primaryBlockId) onBlockHover?.(primaryBlockId);
                  }}
                  onMouseLeave={onBlockLeave}
                  role={selectable ? "button" : undefined}
                  tabIndex={selectable ? 0 : undefined}
                  title={selectable ? "点击选择重翻" : undefined}
                >
                  {emptyTranslation ? (
                    isTranslating ? (
                      <MarkdownTranslationSkeleton active={activity === "translating"} />
                    ) : (
                      <p className="min-h-7 text-sm leading-7 text-muted-foreground">
                        等待翻译
                      </p>
                    )
                  ) : (
                    <div className="rosetta-markdown-preview">
                      <ReactMarkdown
                        remarkPlugins={[remarkGfm]}
                        components={markdownComponents}
                      >
                        {block.markdown}
                      </ReactMarkdown>
                    </div>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </ScrollArea>
  );
}

function PdfMarkdownImage({
  jobId,
  src,
  alt,
}: {
  jobId: string;
  src?: string;
  alt?: string;
}) {
  const [objectUrl, setObjectUrl] = useState<string | null>(null);
  const [unavailable, setUnavailable] = useState(false);

  useEffect(() => {
    if (!src) {
      setUnavailable(true);
      return;
    }
    let active = true;
    let nextUrl: string | null = null;
    setObjectUrl(null);
    setUnavailable(false);
    void readPdfMarkdownAsset(jobId, src)
      .then((bytes) => {
        if (!active) return;
        nextUrl = URL.createObjectURL(
          new Blob([bytes.slice().buffer as ArrayBuffer], {
            type: pdfMarkdownImageMime(src),
          }),
        );
        setObjectUrl(nextUrl);
      })
      .catch(() => {
        if (active) setUnavailable(true);
      });
    return () => {
      active = false;
      if (nextUrl) URL.revokeObjectURL(nextUrl);
    };
  }, [jobId, src]);

  if (unavailable || !src) {
    return (
      <span className="block rounded-md border border-dashed px-3 py-6 text-center text-xs text-muted-foreground">
        图片不可用
      </span>
    );
  }
  if (!objectUrl) {
    return <span className="block h-24 animate-pulse rounded-md bg-muted" />;
  }
  return (
    <img
      src={objectUrl}
      alt={alt ?? ""}
      className="h-auto max-w-full rounded-sm"
      loading="lazy"
    />
  );
}

function pdfMarkdownImageMime(path: string) {
  const extension = path.split(".").pop()?.toLowerCase();
  if (extension === "jpg" || extension === "jpeg") return "image/jpeg";
  if (extension === "webp") return "image/webp";
  return "image/png";
}

function pdfMarkdownPreviewStatus(
  componentStatus: PdfMarkdownComponentStatus | null,
  extractionStatus: PdfMarkdownExtractionStatus | null,
  previewError: string | null,
) {
  if (previewError) {
    return { title: "Markdown 预览不可用", detail: previewError };
  }
  if (!componentStatus) {
    return { title: "正在检查 Markdown 组件", detail: null };
  }
  if (componentStatus.state === "unsupported") {
    return { title: "当前平台不支持 Markdown 输出", detail: componentStatus.message };
  }
  if (componentStatus.state === "not-installed") {
    return { title: "Markdown 组件尚未安装", detail: "可从上方工具栏开始下载。" };
  }
  if (componentStatus.state === "needs-repair") {
    return { title: "Markdown 组件需要修复", detail: componentStatus.message };
  }
  if (!extractionStatus || extractionStatus.state === "idle") {
    return { title: "尚未提取 Markdown", detail: "可从上方工具栏开始提取。" };
  }
  if (extractionStatus.state === "extracting") {
    return {
      title: "正在提取 Markdown",
      detail: `${extractionStatus.completedPages}/${extractionStatus.pageCount || "-"} 页`,
    };
  }
  if (extractionStatus.state === "stale") {
    return { title: "源 PDF 已变化", detail: "需要重新提取 Markdown。" };
  }
  if (extractionStatus.state === "failed") {
    return {
      title: "Markdown 提取失败",
      detail:
        pdfMarkdownErrorMessage(extractionStatus.errorCode) ??
        "可在上方工具栏重试。",
    };
  }
  if (extractionStatus.state === "cancelled") {
    return { title: "Markdown 提取已取消", detail: "已完成的临时结果不会作为就绪数据使用。" };
  }
  return null;
}

function SourcePaneHeader({
  canEdit,
  editing,
  saving,
  onCancel,
  onEdit,
  onSave,
}: {
  canEdit: boolean;
  editing: boolean;
  saving: boolean;
  onCancel?: () => void;
  onEdit?: () => void;
  onSave?: () => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3 border-r px-4 py-3">
      <span>原文</span>
      {canEdit ? (
        editing ? (
          <div className="flex items-center gap-1.5">
            <Button
              size="xs"
              variant="ghost"
              onClick={onCancel}
              disabled={saving}
            >
              取消
            </Button>
            <Button
              size="xs"
              variant="secondary"
              onClick={onSave}
              disabled={saving}
            >
              {saving ? "保存中" : "保存"}
            </Button>
          </div>
        ) : (
          <Button size="xs" variant="ghost" onClick={onEdit}>
            编辑
          </Button>
        )
      ) : null}
    </div>
  );
}

function SourceEditPane({
  value,
  onChange,
}: {
  value: string;
  onChange?: (value: string) => void;
}) {
  return (
    <div className="flex min-h-0 border-r bg-background">
      <textarea
        value={value}
        onChange={(event) => onChange?.(event.target.value)}
        placeholder="输入原文。空行会作为段落分隔。"
        className="min-h-0 flex-1 resize-none bg-background px-6 py-6 text-sm leading-7 outline-none placeholder:text-muted-foreground"
        spellCheck={false}
      />
    </div>
  );
}

function PreviewPane({
  document,
  file,
  hoveredBlockId,
  onBlockHover,
  onBlockLeave,
  onToggleBlockSelection,
  onScroll,
  paneRef,
  selectedBlockIds,
  selectionEnabled,
  side,
  sourceSegments,
  translationSegments,
  isTranslating,
}: {
  document: RosettaDocument;
  file: RosettaSourceFile;
  hoveredBlockId: string | null;
  onBlockHover?: (blockId: string) => void;
  onBlockLeave?: () => void;
  onToggleBlockSelection?: (blockIds: string[]) => void;
  onScroll: () => void;
  paneRef: RefObject<HTMLDivElement>;
  selectedBlockIds: string[];
  selectionEnabled: boolean;
  side: PreviewSide;
  sourceSegments: Segment[];
  translationSegments: TranslationSegment[];
  isTranslating: boolean;
}) {
  const segmentsByBlock = useMemo(
    () => groupSegmentsByBlock(sourceSegments),
    [sourceSegments]
  );
  const translationBySegmentId = useMemo(
    () =>
      new Map(
        translationSegments.map((segment) => [
          segment.sourceSegmentId,
          segment,
        ])
      ),
    [translationSegments]
  );
  const blocks = useMemo(
    () =>
      document.blocks.filter((block) => (block.fileId ?? "file-1") === file.id),
    [document.blocks, file.id]
  );
  const virtualizer = useVirtualizer({
    count: blocks.length,
    getScrollElement: () => paneRef.current,
    estimateSize: () => 96,
    overscan: 8,
  });

  return (
    <ScrollArea
      className={cn("h-full min-h-0 bg-background", side === "source" && "border-r")}
      onScroll={onScroll}
      viewportRef={paneRef}
    >
      <div className="mx-auto max-w-(--rosetta-reader-max-width) px-6 py-6">
        {blocks.length === 0 ? (
          <div className="flex min-h-32 items-center justify-center text-sm text-muted-foreground">
            当前文件没有可预览内容。
          </div>
        ) : (
          <div
            className="relative w-full"
            style={{ height: `${virtualizer.getTotalSize()}px` }}
          >
            {virtualizer.getVirtualItems().map((item) => {
              const block = blocks[item.index];

              return (
                <div
                  className="absolute left-0 top-0 w-full"
                  data-index={item.index}
                  key={`${side}-${block.id}`}
                  ref={virtualizer.measureElement}
                  style={{
                    transform: `translateY(${item.start}px)`,
                  }}
                >
                  <PreviewBlock
                    block={block}
                    document={document}
                    file={file}
                    hovered={hoveredBlockId === block.id}
                    onBlockHover={onBlockHover}
                    onBlockLeave={onBlockLeave}
                    onToggleBlockSelection={onToggleBlockSelection}
                    selected={selectedBlockIds.includes(block.id)}
                    selectionEnabled={selectionEnabled}
                    segmentsByBlock={segmentsByBlock}
                    side={side}
                    translationBySegmentId={translationBySegmentId}
                    isTranslating={isTranslating}
                  />
                </div>
              );
            })}
          </div>
        )}
      </div>
    </ScrollArea>
  );
}

function PreviewBlock({
  block,
  document,
  file,
  hovered,
  onBlockHover,
  onBlockLeave,
  onToggleBlockSelection,
  selected,
  selectionEnabled,
  segmentsByBlock,
  side,
  translationBySegmentId,
  isTranslating,
}: {
  block: RosettaBlock;
  document: RosettaDocument;
  file: RosettaSourceFile;
  hovered: boolean;
  onBlockHover?: (blockId: string) => void;
  onBlockLeave?: () => void;
  onToggleBlockSelection?: (blockIds: string[]) => void;
  selected: boolean;
  selectionEnabled: boolean;
  segmentsByBlock: Map<string, Segment[]>;
  side: PreviewSide;
  translationBySegmentId: Map<string, TranslationSegment>;
  isTranslating: boolean;
}) {
  const blockSegments = segmentsByBlock.get(block.id) ?? [];
  const activity = blockTranslationActivity(
    blockSegments,
    translationBySegmentId,
    isTranslating,
  );
  const text =
    side === "source"
      ? block.sourceText
      : blockTranslation(block, segmentsByBlock, translationBySegmentId);
  const hasEmptyTranslation =
    side === "translation" && block.shouldTranslate && !text.trim();
  const renderedText = hasEmptyTranslation
    ? ""
    : renderBlockMarkdown(file.format ?? document.format, block, text);
  const selectable =
    selectionEnabled &&
    block.shouldTranslate &&
    (segmentsByBlock.get(block.id)?.length ?? 0) > 0;

  if (block.type === "metadata" && !renderedText.trim()) {
    return <div className="h-3" />;
  }

  return (
    <div
      aria-pressed={selectable ? selected : undefined}
      className={cn(
        "relative rounded-md px-3 py-1.5 transition-colors",
        selectable && "cursor-pointer",
        hovered && "bg-muted/60",
        selected && "bg-primary/10 ring-1 ring-primary/25",
        hasEmptyTranslation && "text-muted-foreground",
        side === "source" &&
          activity === "translating" &&
          "rosetta-markdown-source-scanning"
      )}
      data-block-id={block.id}
      onClick={() => {
        if (selectable) {
          onToggleBlockSelection?.([block.id]);
        }
      }}
      onKeyDown={(event) => {
        if (!selectable || (event.key !== "Enter" && event.key !== " ")) {
          return;
        }
        event.preventDefault();
        onToggleBlockSelection?.([block.id]);
      }}
      onMouseEnter={() => onBlockHover?.(block.id)}
      onMouseLeave={onBlockLeave}
      role={selectable ? "button" : undefined}
      tabIndex={selectable ? 0 : undefined}
      title={selectable ? "点击选择重翻" : undefined}
    >
      {hasEmptyTranslation ? (
        isTranslating ? (
          <MarkdownTranslationSkeleton active={activity === "translating"} />
        ) : (
          <p className="min-h-7 text-sm leading-7">等待翻译</p>
        )
      ) : file.format === "markdown" ? (
        <div className="rosetta-markdown-preview">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{renderedText}</ReactMarkdown>
        </div>
      ) : (
        <p className="whitespace-pre-wrap text-sm leading-7">{renderedText}</p>
      )}
    </div>
  );
}

function MarkdownTranslationSkeleton({ active }: { active: boolean }) {
  return (
    <div
      className="rosetta-markdown-translation-skeleton"
      data-active={active ? "true" : "false"}
      aria-label={active ? "当前段落翻译中" : "段落等待翻译"}
    >
      <span />
      <span />
      <span />
    </div>
  );
}

function blockTranslationActivity(
  segments: Segment[],
  translationBySegmentId: Map<string, TranslationSegment>,
  isTranslating: boolean,
) {
  if (!isTranslating || segments.length === 0) return null;
  const statuses = segments.map((segment) => translationBySegmentId.get(segment.id)?.status);
  if (statuses.some((status) => status === "translating")) return "translating";
  if (statuses.some((status) => status === "pending")) return "queued";
  if (segments.some((segment) => !translationBySegmentId.get(segment.id)?.translatedText?.trim())) {
    return "queued";
  }
  return "translated";
}

function groupSegmentsByBlock(segments: Segment[]) {
  const grouped = new Map<string, Segment[]>();
  for (const segment of segments) {
    const blockSegments = grouped.get(segment.blockId);
    if (blockSegments) {
      blockSegments.push(segment);
    } else {
      grouped.set(segment.blockId, [segment]);
    }
  }
  for (const blockSegments of grouped.values()) {
    blockSegments.sort(
      (left, right) =>
        (left.segmentIndexInBlock ?? 0) - (right.segmentIndexInBlock ?? 0)
    );
  }
  return grouped;
}

function blockTranslation(
  block: RosettaBlock,
  segmentsByBlock: Map<string, Segment[]>,
  translationBySegmentId: Map<string, TranslationSegment>
) {
  if (!block.shouldTranslate) {
    return block.sourceText;
  }

  const segments = segmentsByBlock.get(block.id);
  if (!segments || segments.length === 0) {
    return "";
  }

  return segments
    .map((segment) => {
      const translation = translationBySegmentId.get(segment.id);
      return translation?.translatedText?.trim() ?? "";
    })
    .join(segmentJoiner(translationBySegmentId, segments))
    .trim();
}

function segmentJoiner(
  translationBySegmentId: Map<string, TranslationSegment>,
  segments: Segment[]
) {
  const targetLang =
    segments
      .map((segment) => translationBySegmentId.get(segment.id)?.targetLang)
      .find(Boolean) ?? "";
  return /^(zh|ja|ko)/i.test(targetLang) ? "" : " ";
}

function renderBlockMarkdown(
  format: RosettaSourceDocumentFormat,
  block: RosettaBlock,
  text: string
) {
  if (format !== "markdown") {
    return text;
  }

  switch (block.type) {
    case "heading":
      return `${styleMarker(block, "#")} ${text}`;
    case "list_item":
      return `${styleMarker(block, "-")} ${text}`;
    case "blockquote":
      return `> ${text}`;
    default:
      return text;
  }
}

function styleMarker(block: RosettaBlock, fallback: string) {
  const marker = block.style?.marker;
  return typeof marker === "string" && marker.trim() ? marker : fallback;
}
