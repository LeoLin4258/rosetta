import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useRosettaStore } from "../../store/useRosettaStore";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";

export function SegmentPreviewList() {
  const segments = useRosettaStore((state) => state.previewSegments);
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: segments.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 116,
    overscan: 6,
  });

  if (segments.length === 0) {
    return (
      <Card className="min-h-0 py-0">
        <div className="flex h-[420px] items-center justify-center text-sm text-muted-foreground">
          当前没有可预览的段落。
        </div>
      </Card>
    );
  }

  return (
    <Card className="min-h-0 py-0">
      <div className="grid grid-cols-2 border-b bg-muted/40 text-sm text-muted-foreground">
        <div className="border-r px-4 py-3">原文</div>
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
                className="absolute left-0 top-0 grid w-full grid-cols-2 border-b text-sm"
                key={segment.id}
                style={{
                  height: `${item.size}px`,
                  transform: `translateY(${item.start}px)`,
                }}
              >
                <div className="border-r px-4 py-3 text-muted-foreground">
                  <div className="mb-2 text-xs">#{segment.order}</div>
                  {segment.sourceText}
                </div>
                <div className="px-4 py-3">
                  <div className="mb-2">
                    <Badge variant="outline">{segment.status}</Badge>
                  </div>
                  {segment.translatedText ?? "等待翻译"}
                  {segment.error ? (
                    <p className="mt-2 text-xs text-destructive">{segment.error}</p>
                  ) : null}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </Card>
  );
}
