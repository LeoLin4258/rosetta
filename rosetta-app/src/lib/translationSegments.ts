import type {
  RosettaTranslationFile,
  TranslationSegment,
} from "@/types/rosetta";

export function chunkItems<T>(items: T[], size: number) {
  const chunks: T[][] = [];
  for (let index = 0; index < items.length; index += size) {
    chunks.push(items.slice(index, index + size));
  }
  return chunks;
}

export function translationProgressPercent(
  translationFile: RosettaTranslationFile
) {
  if (translationFile.segmentCount <= 0) {
    return 0;
  }
  return Math.min(
    100,
    Math.round(
      (translationFile.completedSegments / translationFile.segmentCount) * 100
    )
  );
}

export function markTranslationSegmentsTranslating(
  segments: TranslationSegment[],
  sourceSegmentIds: Iterable<string>
) {
  const segmentIdSet = new Set(sourceSegmentIds);
  return segments.map((segment) =>
    segmentIdSet.has(segment.sourceSegmentId)
      ? {
          ...segment,
          status: "translating" as const,
          translatedText: undefined,
          error: undefined,
        }
      : segment
  );
}

export function markTranslationSegmentsPending(
  segments: TranslationSegment[],
  sourceSegmentIds: Iterable<string>
) {
  const segmentIdSet = new Set(sourceSegmentIds);
  return segments.map((segment) =>
    segmentIdSet.has(segment.sourceSegmentId)
      ? {
          ...segment,
          status: "pending" as const,
          translatedText: undefined,
          error: undefined,
        }
      : segment
  );
}

export function markTranslationSegmentsDone(
  segments: TranslationSegment[],
  sourceSegmentIds: string[],
  translations: string[]
) {
  const translationById = new Map(
    sourceSegmentIds.map((segmentId, index) => [segmentId, translations[index]])
  );
  return segments.map((segment) =>
    translationById.has(segment.sourceSegmentId)
      ? {
          ...segment,
          translatedText: translationById.get(segment.sourceSegmentId),
          status: "done" as const,
          error: undefined,
        }
      : segment
  );
}

export function markTranslationSegmentsFailed(
  segments: TranslationSegment[],
  sourceSegmentIds: Iterable<string>,
  error: string
) {
  const segmentIdSet = new Set(sourceSegmentIds);
  return segments.map((segment) =>
    segmentIdSet.has(segment.sourceSegmentId)
      ? {
          ...segment,
          status: "failed" as const,
          error,
        }
      : segment
  );
}
