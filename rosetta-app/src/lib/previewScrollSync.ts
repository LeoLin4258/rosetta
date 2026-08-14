export type PreviewScrollMetrics = {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
};

export type PreviewScrollSide = "source" | "translation";

export function previewScrollMayDrive(
  driver: PreviewScrollSide | null,
  side: PreviewScrollSide,
): boolean {
  return driver === side;
}

export function proportionalPreviewScrollTop(
  source: PreviewScrollMetrics,
  target: Omit<PreviewScrollMetrics, "scrollTop">,
): number {
  const sourceMax = Math.max(0, source.scrollHeight - source.clientHeight);
  const targetMax = Math.max(0, target.scrollHeight - target.clientHeight);
  if (sourceMax === 0 || targetMax === 0) return 0;
  const ratio = Math.min(1, Math.max(0, source.scrollTop / sourceMax));
  return ratio * targetMax;
}

export function previewScrollTargetChanged(
  current: number,
  target: number,
  deadZone = 2,
): boolean {
  return Math.abs(current - target) >= deadZone;
}

export function isPreviewScrollKey(key: string): boolean {
  return ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(
    key,
  );
}
