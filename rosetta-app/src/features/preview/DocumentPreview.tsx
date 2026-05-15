import { useEffect, useMemo, useRef } from "react";
import type { RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { languageLabel } from "@/lib/languages";
import { cn } from "@/lib/utils";
import type {
  RosettaBlock,
  RosettaDocument,
  RosettaSourceDocumentFormat,
  RosettaSourceFile,
  RosettaTranslationFile,
  Segment,
  TranslationSegment,
} from "../../types/rosetta";

type PreviewSide = "source" | "translation";

export function DocumentPreview({
  document,
  hoveredBlockId,
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
}: {
  document: RosettaDocument | null;
  hoveredBlockId?: string | null;
  layout?: "bilingual" | "source";
  onBlockHover?: (blockId: string) => void;
  onBlockLeave?: () => void;
  onToggleBlockSelection?: (blockId: string) => void;
  selectedBlockIds?: string[];
  selectionEnabled?: boolean;
  sourceFile: RosettaSourceFile | null;
  sourceSegments: Segment[];
  translationFile: RosettaTranslationFile | null;
  translationSegments: TranslationSegment[];
}) {
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
        <div className="border-b bg-muted/40 px-4 py-3 text-sm text-muted-foreground">
          原文
        </div>
        <div className="min-h-0 flex-1">
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
          />
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
        <div className="border-r px-4 py-3">原文</div>
        <div className="flex items-center justify-between gap-3 px-4 py-3">
          <span>译文</span>
          {translationFile ? (
            <Badge variant="outline">{languageLabel(translationFile.targetLang)}</Badge>
          ) : null}
        </div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-2">
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
        />
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
}: {
  document: RosettaDocument;
  file: RosettaSourceFile;
  hoveredBlockId: string | null;
  onBlockHover?: (blockId: string) => void;
  onBlockLeave?: () => void;
  onToggleBlockSelection?: (blockId: string) => void;
  onScroll: () => void;
  paneRef: RefObject<HTMLDivElement>;
  selectedBlockIds: string[];
  selectionEnabled: boolean;
  side: PreviewSide;
  sourceSegments: Segment[];
  translationSegments: TranslationSegment[];
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
}: {
  block: RosettaBlock;
  document: RosettaDocument;
  file: RosettaSourceFile;
  hovered: boolean;
  onBlockHover?: (blockId: string) => void;
  onBlockLeave?: () => void;
  onToggleBlockSelection?: (blockId: string) => void;
  selected: boolean;
  selectionEnabled: boolean;
  segmentsByBlock: Map<string, Segment[]>;
  side: PreviewSide;
  translationBySegmentId: Map<string, TranslationSegment>;
}) {
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
        hasEmptyTranslation && "text-muted-foreground"
      )}
      data-block-id={block.id}
      onClick={() => {
        if (selectable) {
          onToggleBlockSelection?.(block.id);
        }
      }}
      onKeyDown={(event) => {
        if (!selectable || (event.key !== "Enter" && event.key !== " ")) {
          return;
        }
        event.preventDefault();
        onToggleBlockSelection?.(block.id);
      }}
      onMouseEnter={() => onBlockHover?.(block.id)}
      onMouseLeave={onBlockLeave}
      role={selectable ? "button" : undefined}
      tabIndex={selectable ? 0 : undefined}
      title={selectable ? "点击选择重翻" : undefined}
    >
      {hasEmptyTranslation ? (
        <p className="min-h-7 text-sm leading-7">等待翻译</p>
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
