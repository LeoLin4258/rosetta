type TranslationRunProgress = {
  completedSegmentIds: string[];
  targetSegmentIds: string[];
};

type TranslationFileProgress = {
  completedSegments: number;
  segmentCount: number;
};

type SourceFileProgress = {
  segmentCount?: number;
};

export function resolveTranslationProgress(
  activeRun: TranslationRunProgress | null,
  translationFile: TranslationFileProgress | null,
  sourceFile: SourceFileProgress | null,
) {
  return {
    completed:
      activeRun?.completedSegmentIds.length ??
      translationFile?.completedSegments ??
      0,
    total:
      activeRun?.targetSegmentIds.length ??
      translationFile?.segmentCount ??
      sourceFile?.segmentCount ??
      0,
  };
}

export function canExportSelectedTranslation(
  isNativePdfOutput: boolean,
  translationFile: TranslationFileProgress | null,
) {
  if (!translationFile) return false;
  if (isNativePdfOutput) return true;
  return (
    translationFile.segmentCount > 0 &&
    translationFile.completedSegments >= translationFile.segmentCount
  );
}
