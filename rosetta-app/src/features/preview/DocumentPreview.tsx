import { useEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { useRosettaStore } from "../../store/useRosettaStore";
import type {
  RosettaBlock,
  RosettaDocument,
  RosettaSourceFile,
  Segment,
} from "../../types/rosetta";

type PreviewSide = "source" | "translation";

export function DocumentPreview({
  currentFileId,
  currentJobId,
}: {
  currentFileId: string | null;
  currentJobId: string | null;
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
  const document =
    activeJobId === currentJobId && activeDocument ? activeDocument : null;
  const segments = activeJobId === currentJobId ? previewSegments : [];
  const files = useMemo(() => documentFiles(document), [document]);
  const selectedFile = useMemo(
    () => files.find((file) => file.id === currentFileId) ?? files[0] ?? null,
    [currentFileId, files]
  );

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

    if (syncFrameRef.current != null) {
      window.cancelAnimationFrame(syncFrameRef.current);
    }

    syncFrameRef.current = window.requestAnimationFrame(() => {
      isSyncingRef.current = true;
      to.scrollTop = ratio * Math.max(maxTo, 0);
      syncFrameRef.current = window.requestAnimationFrame(() => {
        isSyncingRef.current = false;
        syncFrameRef.current = null;
      });
    });
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
    <Card className="flex h-full min-h-0 flex-col overflow-hidden py-0">
      <div className="grid grid-cols-2 border-b bg-muted/40 text-sm text-muted-foreground">
        <div className="border-r px-4 py-3">原文</div>
        <div className="px-4 py-3">译文</div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-2">
        <PreviewPane
          document={document}
          file={selectedFile}
          hasMultipleFiles={files.length > 1}
          hoveredBlockId={hoveredBlockId}
          onHoverBlock={updateHoveredBlock}
          onScroll={() => syncScroll("source")}
          paneRef={sourceRef}
          segments={segments}
          side="source"
        />
        <PreviewPane
          document={document}
          file={selectedFile}
          hasMultipleFiles={files.length > 1}
          hoveredBlockId={hoveredBlockId}
          onHoverBlock={updateHoveredBlock}
          onScroll={() => syncScroll("translation")}
          paneRef={translationRef}
          segments={segments}
          side="translation"
        />
      </div>
    </Card>
  );
}

function PreviewPane({
  document,
  file,
  hasMultipleFiles,
  hoveredBlockId,
  onHoverBlock,
  onScroll,
  paneRef,
  segments,
  side,
}: {
  document: RosettaDocument;
  file: RosettaSourceFile;
  hasMultipleFiles: boolean;
  hoveredBlockId: string | null;
  onHoverBlock: (blockId: string | null) => void;
  onScroll: () => void;
  paneRef: RefObject<HTMLDivElement>;
  segments: Segment[];
  side: PreviewSide;
}) {
  const segmentsByBlock = useMemo(() => groupSegmentsByBlock(segments), [segments]);
  const blocks = useMemo(
    () =>
      document.blocks.filter((block) => (block.fileId ?? "file-1") === file.id),
    [document.blocks, file.id]
  );

  return (
    <div
      className={cn(
        "min-h-0 overflow-auto bg-background",
        side === "source" && "border-r"
      )}
      onScroll={onScroll}
      ref={paneRef}
    >
      <div className="mx-auto flex max-w-3xl flex-col gap-7 px-6 py-6">
        <section className="flex flex-col gap-3">
          {hasMultipleFiles ? (
            <div className="flex items-center justify-between gap-3 border-b pb-2">
              <span className="truncate text-sm font-medium">
                {file.relativePath}
              </span>
              <Badge variant="outline">{file.format}</Badge>
            </div>
          ) : null}

          <div className="flex flex-col gap-1">
            {blocks.map((block) => (
              <PreviewBlock
                block={block}
                document={document}
                file={file}
                hovered={hoveredBlockId === block.id}
                key={`${side}-${block.id}`}
                onHoverBlock={onHoverBlock}
                segmentsByBlock={segmentsByBlock}
                side={side}
              />
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function PreviewBlock({
  block,
  document,
  file,
  hovered,
  onHoverBlock,
  segmentsByBlock,
  side,
}: {
  block: RosettaBlock;
  document: RosettaDocument;
  file: RosettaSourceFile;
  hovered: boolean;
  onHoverBlock: (blockId: string | null) => void;
  segmentsByBlock: Map<string, Segment[]>;
  side: PreviewSide;
}) {
  const text =
    side === "source"
      ? block.sourceText
      : blockTranslation(block, segmentsByBlock, document.targetLang);
  const hasEmptyTranslation =
    side === "translation" && block.shouldTranslate && !text.trim();
  const renderedText = hasEmptyTranslation
    ? ""
    : renderBlockMarkdown(file.format ?? document.format, block, text);

  if (block.type === "metadata" && !renderedText.trim()) {
    return <div className="h-3" />;
  }

  return (
    <div
      className={cn(
        "rounded-md px-2 py-1.5",
        hovered && "bg-muted",
        block.status === "failed" && side === "translation" && "text-destructive"
      )}
      data-block-id={block.id}
      onMouseEnter={() => onHoverBlock(block.id)}
      onMouseLeave={() => onHoverBlock(null)}
    >
      {hasEmptyTranslation ? (
        <div className="min-h-[1.75rem]" />
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
  targetLang: string
) {
  if (!block.shouldTranslate) {
    return block.sourceText;
  }

  const segments = segmentsByBlock.get(block.id);
  if (!segments || segments.length === 0) {
    return block.translatedText?.trim() || "";
  }

  const translated = segments
    .map((segment) => segment.translatedText?.trim() || "")
    .join(segmentJoiner(targetLang))
    .trim();

  return translated;
}

function segmentJoiner(targetLang: string) {
  return isCompactTargetLanguage(targetLang) ? "" : " ";
}

function isCompactTargetLanguage(targetLang: string) {
  return /^(zh|ja|ko)/i.test(targetLang);
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
