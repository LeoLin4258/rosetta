import { useEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { useRosettaStore } from "../../store/useRosettaStore";
import type {
  RosettaBlock,
  RosettaDocument,
  RosettaSourceFile,
  Segment,
  TranslationRevision,
} from "../../types/rosetta";

type PreviewSide = "source" | "translation";

type RevisionOption = {
  id: string;
  label: string;
  revision: TranslationRevision;
};

export function DocumentPreview({
  currentFileId,
  currentJobId,
  onReaderScroll,
  onRevisionChange,
  onToggleBlockSelection,
  selectedBlockIds,
  selectedRevisionId,
  selectionEnabled,
  translationRevisions,
}: {
  currentFileId: string | null;
  currentJobId: string | null;
  onReaderScroll?: (direction: "down" | "up") => void;
  onRevisionChange: (revisionId: string) => void;
  onToggleBlockSelection: (blockId: string) => void;
  selectedBlockIds: string[];
  selectedRevisionId: string;
  selectionEnabled: boolean;
  translationRevisions: TranslationRevision[];
}) {
  const activeJobId = useRosettaStore((state) => state.activeJobId);
  const activeDocument = useRosettaStore((state) => state.activeDocument);
  const previewSegments = useRosettaStore((state) => state.previewSegments);
  const [hoveredBlockId, setHoveredBlockId] = useState<string | null>(null);
  const sourceRef = useRef<HTMLDivElement>(null);
  const translationRef = useRef<HTMLDivElement>(null);
  const isSyncingRef = useRef(false);
  const isHoverPausedRef = useRef(false);
  const hoverRestoreTimerRef = useRef<number | null>(null);
  const syncFrameRef = useRef<number | null>(null);
  const lastSourceScrollTopRef = useRef(0);
  const lastTranslationScrollTopRef = useRef(0);
  const document =
    activeJobId === currentJobId && activeDocument ? activeDocument : null;
  const segments = activeJobId === currentJobId ? previewSegments : [];
  const files = useMemo(() => documentFiles(document), [document]);
  const selectedFile = useMemo(
    () =>
      currentFileId
        ? files.find((file) => file.id === currentFileId) ?? null
        : files[0] ?? null,
    [currentFileId, files]
  );
  const revisionOptions = useMemo(
    () =>
      selectedFile
        ? buildRevisionOptions(translationRevisions, selectedFile.id)
        : [],
    [selectedFile, translationRevisions]
  );
  const selectedRevision =
    selectedRevisionId === "current"
      ? null
      : revisionOptions.find((option) => option.id === selectedRevisionId)
          ?.revision ?? null;

  useEffect(() => {
    if (
      selectedRevisionId !== "current" &&
      !revisionOptions.some((option) => option.id === selectedRevisionId)
    ) {
      onRevisionChange("current");
    }
  }, [onRevisionChange, revisionOptions, selectedRevisionId]);

  useEffect(
    () => () => {
      if (hoverRestoreTimerRef.current != null) {
        window.clearTimeout(hoverRestoreTimerRef.current);
      }
      if (syncFrameRef.current != null) {
        window.cancelAnimationFrame(syncFrameRef.current);
      }
    },
    []
  );

  if (!document || !selectedFile) {
    return (
      <Card className="flex h-full min-h-0 py-0">
        <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
          选择一个文件后预览文档。
        </div>
      </Card>
    );
  }

  function syncScroll(side: PreviewSide) {
    const source = sourceRef.current;
    const translation = translationRef.current;
    if (!source || !translation || isSyncingRef.current) {
      return;
    }

    pauseHoverDuringScroll();
    const from = side === "source" ? source : translation;
    const to = side === "source" ? translation : source;
    const maxFrom = from.scrollHeight - from.clientHeight;
    const maxTo = to.scrollHeight - to.clientHeight;
    const ratio = maxFrom > 0 ? from.scrollTop / maxFrom : 0;
    reportReaderScroll(side, from.scrollTop);

    if (syncFrameRef.current != null) {
      window.cancelAnimationFrame(syncFrameRef.current);
    }

    syncFrameRef.current = window.requestAnimationFrame(() => {
      isSyncingRef.current = true;
      const nextScrollTop = ratio * Math.max(maxTo, 0);
      to.scrollTop = nextScrollTop;
      if (side === "source") {
        lastTranslationScrollTopRef.current = nextScrollTop;
      } else {
        lastSourceScrollTopRef.current = nextScrollTop;
      }
      syncFrameRef.current = window.requestAnimationFrame(() => {
        isSyncingRef.current = false;
        syncFrameRef.current = null;
      });
    });
  }

  function reportReaderScroll(side: PreviewSide, scrollTop: number) {
    const lastScrollTopRef =
      side === "source" ? lastSourceScrollTopRef : lastTranslationScrollTopRef;
    const delta = scrollTop - lastScrollTopRef.current;
    lastScrollTopRef.current = scrollTop;

    if (Math.abs(delta) < 10) {
      return;
    }

    onReaderScroll?.(delta > 0 ? "down" : "up");
  }

  function pauseHoverDuringScroll() {
    if (!isHoverPausedRef.current) {
      isHoverPausedRef.current = true;
      setHoveredBlockId(null);
    }

    if (hoverRestoreTimerRef.current != null) {
      window.clearTimeout(hoverRestoreTimerRef.current);
    }

    hoverRestoreTimerRef.current = window.setTimeout(() => {
      isHoverPausedRef.current = false;
      hoverRestoreTimerRef.current = null;
    }, 90);
  }

  function updateHoveredBlock(blockId: string | null) {
    if (isHoverPausedRef.current) {
      return;
    }
    setHoveredBlockId(blockId);
  }

  return (
    <Card className="flex h-full min-h-0 flex-col gap-0 overflow-hidden py-0">
      <div className="grid grid-cols-2 border-b bg-muted/40 text-sm text-muted-foreground">
        <div className="border-r px-4 py-3">原文</div>
        <div className="flex items-center justify-between gap-3 px-4 py-2">
          <div className="flex min-w-0 items-center gap-2">
            <span>译文</span>
            {selectedRevision ? <Badge variant="outline">历史版本</Badge> : null}
          </div>
          <Select value={selectedRevisionId} onValueChange={onRevisionChange}>
            <SelectTrigger size="sm" className="max-w-56">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="current">当前译文</SelectItem>
                {revisionOptions.map((option) => (
                  <SelectItem key={option.id} value={option.id}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-2">
        <PreviewPane
          document={document}
          file={selectedFile}
          hoveredBlockId={hoveredBlockId}
          onHoverBlock={updateHoveredBlock}
          onScroll={() => syncScroll("source")}
          onToggleBlockSelection={onToggleBlockSelection}
          paneRef={sourceRef}
          selectedBlockIds={selectedBlockIds}
          selectedRevision={null}
          segments={segments}
          selectionEnabled={false}
          side="source"
        />
        <PreviewPane
          document={document}
          file={selectedFile}
          hoveredBlockId={hoveredBlockId}
          onHoverBlock={updateHoveredBlock}
          onScroll={() => syncScroll("translation")}
          onToggleBlockSelection={onToggleBlockSelection}
          paneRef={translationRef}
          selectedBlockIds={selectedBlockIds}
          selectedRevision={selectedRevision}
          segments={segments}
          selectionEnabled={selectionEnabled && selectedRevision == null}
          side="translation"
        />
      </div>
    </Card>
  );
}

function PreviewPane({
  document,
  file,
  hoveredBlockId,
  onHoverBlock,
  onScroll,
  onToggleBlockSelection,
  paneRef,
  selectedBlockIds,
  selectedRevision,
  segments,
  selectionEnabled,
  side,
}: {
  document: RosettaDocument;
  file: RosettaSourceFile;
  hoveredBlockId: string | null;
  onHoverBlock: (blockId: string | null) => void;
  onScroll: () => void;
  onToggleBlockSelection: (blockId: string) => void;
  paneRef: RefObject<HTMLDivElement>;
  selectedBlockIds: string[];
  selectedRevision: TranslationRevision | null;
  segments: Segment[];
  selectionEnabled: boolean;
  side: PreviewSide;
}) {
  const segmentsByBlock = useMemo(() => groupSegmentsByBlock(segments), [segments]);
  const selectedBlockIdSet = useMemo(
    () => new Set(selectedBlockIds),
    [selectedBlockIds]
  );
  const blocks = useMemo(
    () =>
      document.blocks.filter((block) => (block.fileId ?? "file-1") === file.id),
    [document.blocks, file.id]
  );
  const virtualizer = useVirtualizer({
    count: blocks.length,
    getScrollElement: () => paneRef.current,
    estimateSize: () => 92,
    overscan: 8,
  });

  return (
    <ScrollArea
      className={cn("h-full min-h-0 bg-background", side === "source" && "border-r")}
      onScroll={onScroll}
      viewportRef={paneRef}
    >
      <div className="mx-auto max-w-3xl px-6 py-6">
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
                  className="absolute left-0 top-0 w-full py-1"
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
                    onHoverBlock={onHoverBlock}
                    onToggleBlockSelection={onToggleBlockSelection}
                    selected={selectedBlockIdSet.has(block.id)}
                    selectedRevision={selectedRevision}
                    segmentsByBlock={segmentsByBlock}
                    selectionEnabled={selectionEnabled}
                    side={side}
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
  onHoverBlock,
  onToggleBlockSelection,
  selected,
  selectedRevision,
  segmentsByBlock,
  selectionEnabled,
  side,
}: {
  block: RosettaBlock;
  document: RosettaDocument;
  file: RosettaSourceFile;
  hovered: boolean;
  onHoverBlock: (blockId: string | null) => void;
  onToggleBlockSelection: (blockId: string) => void;
  selected: boolean;
  selectedRevision: TranslationRevision | null;
  segmentsByBlock: Map<string, Segment[]>;
  selectionEnabled: boolean;
  side: PreviewSide;
}) {
  const text =
    side === "source"
      ? block.sourceText
      : blockTranslation(
          block,
          segmentsByBlock,
          file.targetLang ?? document.targetLang,
          selectedRevision
        );
  const hasEmptyTranslation =
    side === "translation" && block.shouldTranslate && !text.trim();
  const renderedText = hasEmptyTranslation
    ? ""
    : renderBlockMarkdown(file.format ?? document.format, block, text);
  const selectable =
    selectionEnabled &&
    side === "translation" &&
    block.shouldTranslate &&
    hasTranslatableSegments(block.id, segmentsByBlock);

  if (block.type === "metadata" && !renderedText.trim()) {
    return <div className="h-3" />;
  }

  return (
    <div
      aria-pressed={selectable ? selected : undefined}
      className={cn(
        "rounded-lg px-3 py-2 transition-colors",
        hovered && "bg-muted",
        selected && "bg-muted ring-1 ring-ring",
        selectable && "cursor-pointer",
        block.status === "failed" && side === "translation" && "text-destructive"
      )}
      data-block-id={block.id}
      onClick={() => {
        if (selectable) {
          onToggleBlockSelection(block.id);
        }
      }}
      onKeyDown={(event) => {
        if (!selectable || (event.key !== "Enter" && event.key !== " ")) {
          return;
        }
        event.preventDefault();
        onToggleBlockSelection(block.id);
      }}
      onMouseEnter={() => onHoverBlock(block.id)}
      onMouseLeave={() => onHoverBlock(null)}
      role={selectable ? "button" : undefined}
      tabIndex={selectable ? 0 : undefined}
    >
      {hasEmptyTranslation ? (
        <div className="min-h-7" />
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

function documentFiles(document: RosettaDocument | null): RosettaSourceFile[] {
  if (!document) {
    return [];
  }
  if (document.files.length > 0) {
    return document.files;
  }
  return [
    {
      id: "file-1",
      filename: document.filename,
      relativePath: document.filename,
      format: document.format,
      sourceLang: document.sourceLang,
      targetLang: document.targetLang,
      blockIds: document.blocks.map((block) => block.id),
    },
  ];
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
  targetLang: string,
  revision: TranslationRevision | null
) {
  if (!block.shouldTranslate) {
    return block.sourceText;
  }

  const segments = segmentsByBlock.get(block.id);
  if (!segments || segments.length === 0) {
    return revision ? "" : block.translatedText?.trim() || "";
  }

  const translated = segments
    .map((segment) =>
      revision
        ? revision.segmentTranslations[segment.id]?.trim() || ""
        : segment.translatedText?.trim() || ""
    )
    .join(segmentJoiner(targetLang))
    .trim();

  return translated;
}

function hasTranslatableSegments(
  blockId: string,
  segmentsByBlock: Map<string, Segment[]>
) {
  return (
    segmentsByBlock
      .get(blockId)
      ?.some(
        (segment) =>
          segment.status !== "skipped" && segment.sourceText.trim().length > 0
      ) ?? false
  );
}

function buildRevisionOptions(
  revisions: TranslationRevision[],
  fileId: string
): RevisionOption[] {
  const chronological = revisions
    .filter((revision) => revision.fileId === fileId)
    .sort((left, right) => left.createdAt.localeCompare(right.createdAt));
  const labels = new Map(
    chronological.map((revision, index) => [
      revision.id,
      `第 ${index + 1} 次翻译`,
    ])
  );

  return chronological
    .map((revision) => ({
      id: revision.id,
      label: `${labels.get(revision.id) ?? "历史译文"} · ${formatRevisionTime(
        revision.createdAt
      )}`,
      revision,
    }))
    .reverse();
}

function segmentJoiner(targetLang: string) {
  return isCompactTargetLanguage(targetLang) ? "" : " ";
}

function isCompactTargetLanguage(targetLang: string) {
  return /^(zh|ja|ko)/i.test(targetLang);
}

function formatRevisionTime(value: string) {
  const timestamp = Number(value);
  if (!Number.isFinite(timestamp)) {
    return value;
  }

  return new Intl.DateTimeFormat(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp));
}

function renderBlockMarkdown(
  format: "txt" | "markdown",
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
