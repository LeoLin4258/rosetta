import { useEffect, useRef, useState } from "react";
import { ChevronDownIcon, ChevronUpIcon, LoaderCircleIcon, XIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { saveRosettaTranslationSegments } from "@/lib/rosettaJobs";
import { useRosettaStore } from "@/store/useRosettaStore";
import { cn } from "@/lib/utils";
import type { RosettaBlock, TranslationHistoryEntry } from "@/types/rosetta";

type Props = {
  block: RosettaBlock;
  jobId: string;
  translationFileId: string | null;
  onClose: () => void;
  onRetranslate: (blockId: string) => void;
  isRetranslating: boolean;
};

export function SegmentEditorDrawer({
  block,
  jobId,
  translationFileId,
  onClose,
  onRetranslate,
  isRetranslating,
}: Props) {
  const previewSegments = useRosettaStore((s) => s.previewSegments);
  const translationSegments = useRosettaStore((s) => s.translationSegments);
  const updateActiveTranslationSegments = useRosettaStore(
    (s) => s.updateActiveTranslationSegments
  );

  const blockSourceSegments = previewSegments.filter(
    (s) => s.blockId === block.id
  );
  const tsMap = new Map(
    translationSegments.map((ts) => [ts.sourceSegmentId, ts])
  );

  // Build per-segment initial values: { segmentId -> translatedText }
  const initialEdits = Object.fromEntries(
    blockSourceSegments.map((s) => [
      s.id,
      tsMap.get(s.id)?.translatedText ?? "",
    ])
  );
  const [edits, setEdits] = useState<Record<string, string>>(initialEdits);
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [historyOpen, setHistoryOpen] = useState(false);

  // Reset edits when the block changes (click different segment)
  const prevBlockId = useRef(block.id);
  useEffect(() => {
    if (prevBlockId.current !== block.id) {
      prevBlockId.current = block.id;
      setEdits(
        Object.fromEntries(
          blockSourceSegments.map((s) => [
            s.id,
            tsMap.get(s.id)?.translatedText ?? "",
          ])
        )
      );
      setSaveError(null);
      setHistoryOpen(false);
    }
  });

  // Esc to close
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const isDirty = blockSourceSegments.some(
    (s) => (edits[s.id] ?? "") !== (tsMap.get(s.id)?.translatedText ?? "")
  );

  const allHistory: TranslationHistoryEntry[] = blockSourceSegments.flatMap(
    (s) => tsMap.get(s.id)?.translationHistory ?? []
  );

  async function handleSave() {
    if (!translationFileId) return;
    setSaveError(null);
    setIsSaving(true);
    try {
      const updated = translationSegments.map((ts) => {
        if (edits[ts.sourceSegmentId] === undefined) return ts;
        return {
          ...ts,
          translatedText: edits[ts.sourceSegmentId],
          status: "edited" as const,
        };
      });
      const result = await saveRosettaTranslationSegments(
        jobId,
        translationFileId,
        updated
      );
      updateActiveTranslationSegments(result.segments);
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : "保存失败");
    } finally {
      setIsSaving(false);
    }
  }

  const sourceText = blockSourceSegments.map((s) => s.sourceText).join("\n\n");

  return (
    <div className="flex h-full w-[440px] shrink-0 flex-col border-l bg-background">
      {/* Header */}
      <div className="flex shrink-0 items-center justify-between border-b px-4 py-3">
        <span className="text-sm font-medium text-foreground/80">段落编辑</span>
        <button
          type="button"
          onClick={onClose}
          className="rounded-md p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          title="关闭 (Esc)"
        >
          <XIcon className="size-4" />
        </button>
      </div>

      {/* Scrollable body */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="space-y-5 p-4">
          {/* Source */}
          <div>
            <p className="mb-1.5 text-xs font-medium uppercase tracking-wide text-muted-foreground/60">
              原文
            </p>
            <p className="whitespace-pre-wrap rounded-md bg-muted/40 px-3 py-2.5 text-sm leading-relaxed">
              {sourceText || <span className="italic text-muted-foreground">（无内容）</span>}
            </p>
          </div>

          {/* Translation — one textarea per segment */}
          <div>
            <p className="mb-1.5 text-xs font-medium uppercase tracking-wide text-muted-foreground/60">
              译文
            </p>
            <div className="space-y-2">
              {blockSourceSegments.length === 0 ? (
                <p className="text-sm text-muted-foreground">无可翻译内容</p>
              ) : (
                blockSourceSegments.map((seg) => (
                  <textarea
                    key={seg.id}
                    className={cn(
                      "w-full resize-none rounded-md border bg-background px-3 py-2.5 text-sm leading-relaxed",
                      "focus:outline-none focus:ring-1 focus:ring-ring",
                      !translationFileId && "opacity-50"
                    )}
                    rows={Math.max(3, Math.ceil((edits[seg.id] ?? "").length / 40))}
                    value={edits[seg.id] ?? ""}
                    onChange={(e) =>
                      setEdits((prev) => ({
                        ...prev,
                        [seg.id]: e.target.value,
                      }))
                    }
                    disabled={!translationFileId}
                    placeholder={translationFileId ? "输入译文…" : "请先翻译文档"}
                  />
                ))
              )}
            </div>
          </div>

          {/* Translation history */}
          {allHistory.length > 0 && (
            <div>
              <button
                type="button"
                className="flex w-full items-center gap-1 text-xs text-muted-foreground/60 transition-colors hover:text-muted-foreground"
                onClick={() => setHistoryOpen((o) => !o)}
              >
                {historyOpen ? (
                  <ChevronUpIcon className="size-3" />
                ) : (
                  <ChevronDownIcon className="size-3" />
                )}
                翻译历史 ({allHistory.length} 条)
              </button>
              {historyOpen && (
                <div className="mt-2 space-y-2">
                  {allHistory.map((h) => (
                    <div
                      key={h.id}
                      className="rounded-md border border-border/40 px-3 py-2"
                    >
                      <p className="mb-1 text-xs text-muted-foreground/50">
                        {new Date(h.createdAt).toLocaleString()} · {h.targetLang}
                      </p>
                      <p className="text-sm leading-relaxed">{h.translatedText}</p>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {saveError && (
            <p className="text-xs text-destructive">{saveError}</p>
          )}
        </div>
      </div>

      {/* Action footer */}
      <div className="shrink-0 border-t p-3">
        <div className="flex gap-2">
          <Button
            size="sm"
            variant="outline"
            className="flex-1"
            disabled={isRetranslating || !translationFileId}
            onClick={() => onRetranslate(block.id)}
          >
            {isRetranslating ? (
              <LoaderCircleIcon className="size-3.5 animate-spin" />
            ) : null}
            重新翻译
          </Button>
          <Button
            size="sm"
            className="flex-1"
            disabled={!isDirty || isSaving || !translationFileId}
            onClick={() => void handleSave()}
          >
            {isSaving ? (
              <LoaderCircleIcon className="size-3.5 animate-spin" />
            ) : null}
            保存编辑
          </Button>
        </div>
      </div>
    </div>
  );
}
