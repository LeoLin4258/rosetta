import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useRosettaStore } from "../../store/useRosettaStore";

export function SegmentPreviewList() {
  const segments = useRosettaStore((state) => state.previewSegments);
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: segments.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 116,
    overscan: 6,
  });

  return (
    <div className="min-h-0 rounded-lg border border-zinc-800 bg-zinc-950">
      <div className="grid grid-cols-2 border-b border-zinc-800 bg-zinc-900 text-sm text-zinc-400">
        <div className="border-r border-zinc-800 px-4 py-3">原文</div>
        <div className="px-4 py-3">译文</div>
      </div>
      <div className="h-[420px] overflow-auto" ref={parentRef}>
        <div
          className="relative w-full"
          style={{ height: `${virtualizer.getTotalSize()}px` }}
        >
          {virtualizer.getVirtualItems().map((item) => {
            const segment = segments[item.index];

            return (
              <div
                className="absolute left-0 top-0 grid w-full grid-cols-2 border-b border-zinc-900 text-sm"
                key={segment.id}
                style={{
                  height: `${item.size}px`,
                  transform: `translateY(${item.start}px)`,
                }}
              >
                <div className="border-r border-zinc-900 px-4 py-3 text-zinc-300">
                  <div className="mb-2 text-xs text-zinc-600">#{segment.order}</div>
                  {segment.sourceText}
                </div>
                <div className="px-4 py-3 text-zinc-200">
                  <div className="mb-2 text-xs text-zinc-600">{segment.status}</div>
                  {segment.translatedText ?? "等待翻译"}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
