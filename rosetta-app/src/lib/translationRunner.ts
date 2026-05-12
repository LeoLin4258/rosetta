import { saveRosettaTranslationSegments } from "@/lib/rosettaJobs";
import { translateRwkvTextsWithApi } from "@/lib/rwkvApi";
import {
  chunkItems,
  markTranslationSegmentsDone,
  markTranslationSegmentsFailed,
  markTranslationSegmentsPending,
  markTranslationSegmentsTranslating,
} from "@/lib/translationSegments";
import type {
  RosettaTranslationFile,
  RosettaTranslationFileBundle,
  RwkvConnectionConfig,
  Segment,
  SegmentStatus,
  TranslationSegment,
} from "@/types/rosetta";

export type TranslationRunResult = "completed" | "failed" | "noop" | "stopped";

export type TranslationRunTarget = Pick<Segment, "id" | "order" | "sourceText">;

export function translationTargetsForStatuses({
  sourceSegments,
  translationSegments,
  statuses,
}: {
  sourceSegments: Segment[];
  translationSegments: TranslationSegment[];
  statuses: SegmentStatus[] | "all";
}) {
  const statusBySourceSegmentId = new Map(
    translationSegments.map((segment) => [segment.sourceSegmentId, segment.status])
  );

  return sourceSegments
    .filter((segment) => {
      if (segment.sourceText.trim().length === 0) {
        return false;
      }
      if (statuses === "all") {
        return true;
      }
      const status = statusBySourceSegmentId.get(segment.id);
      return status != null && statuses.includes(status);
    })
    .sort((left, right) => left.order - right.order)
    .map((segment) => ({
      id: segment.id,
      order: segment.order,
      sourceText: segment.sourceText,
    }));
}

export async function runTranslationBatches({
  batchSize,
  cancelPromise,
  jobId,
  onBatchCompleted,
  onBatchFailed,
  onTranslationFileSaved,
  request,
  targets,
  translationFile,
  translationSegments,
}: {
  batchSize: number;
  cancelPromise?: Promise<"stopped">;
  jobId: string;
  onBatchCompleted?: (sourceSegmentIds: string[]) => void;
  onBatchFailed?: (sourceSegmentIds: string[]) => void;
  onTranslationFileSaved?: (bundle: RosettaTranslationFileBundle) => void;
  request: Omit<RwkvConnectionConfig, "mode"> & {
    sourceLang?: string | null;
    targetLang: string;
  };
  targets: TranslationRunTarget[];
  translationFile: RosettaTranslationFile;
  translationSegments: TranslationSegment[];
}): Promise<TranslationRunResult> {
  if (targets.length === 0) {
    return "noop";
  }

  let workingSegments = translationSegments;
  let currentBatchSegmentIds: string[] = [];

  try {
    for (const batch of chunkItems(targets, batchSize)) {
      currentBatchSegmentIds = batch.map((segment) => segment.id);
      workingSegments = markTranslationSegmentsTranslating(
        workingSegments,
        currentBatchSegmentIds
      );
      workingSegments = await saveAndNotify({
        jobId,
        onTranslationFileSaved,
        segments: workingSegments,
        translationFileId: translationFile.id,
      });

      const translationRequest = translateRwkvTextsWithApi({
        baseUrl: request.baseUrl,
        endpoint: request.endpoint,
        internalToken: request.internalToken,
        bodyPassword: request.bodyPassword,
        timeoutMs: request.timeoutMs,
        sourceLang: request.sourceLang,
        targetLang: request.targetLang,
        sourceTexts: batch.map((segment) => segment.sourceText),
      });
      const result = cancelPromise
        ? await Promise.race([translationRequest, cancelPromise])
        : await translationRequest;

      if (result === "stopped") {
        workingSegments = markTranslationSegmentsPending(
          workingSegments,
          currentBatchSegmentIds
        );
        await saveAndNotify({
          jobId,
          onTranslationFileSaved,
          segments: workingSegments,
          translationFileId: translationFile.id,
        });
        return "stopped";
      }

      if (!result.ok || result.translations.length !== batch.length) {
        const message = !result.ok
          ? result.message
          : `RWKV API 返回 ${result.translations.length} 条译文，但本批有 ${batch.length} 条文本。`;
        workingSegments = markTranslationSegmentsFailed(
          workingSegments,
          currentBatchSegmentIds,
          message
        );
        onBatchFailed?.(currentBatchSegmentIds);
        await saveAndNotify({
          jobId,
          onTranslationFileSaved,
          segments: workingSegments,
          translationFileId: translationFile.id,
        });
        return "failed";
      }

      workingSegments = markTranslationSegmentsDone(
        workingSegments,
        currentBatchSegmentIds,
        result.translations
      );
      onBatchCompleted?.(currentBatchSegmentIds);
      workingSegments = await saveAndNotify({
        jobId,
        onTranslationFileSaved,
        segments: workingSegments,
        translationFileId: translationFile.id,
      });
    }

    return "completed";
  } catch (error) {
    if (currentBatchSegmentIds.length === 0) {
      return "failed";
    }

    const message =
      error instanceof Error ? error.message : "RWKV API 翻译调用失败。";
    workingSegments = markTranslationSegmentsFailed(
      workingSegments,
      currentBatchSegmentIds,
      message
    );
    onBatchFailed?.(currentBatchSegmentIds);
    await saveAndNotify({
      jobId,
      onTranslationFileSaved,
      segments: workingSegments,
      translationFileId: translationFile.id,
    });
    return "failed";
  }
}

async function saveAndNotify({
  jobId,
  onTranslationFileSaved,
  segments,
  translationFileId,
}: {
  jobId: string;
  onTranslationFileSaved?: (bundle: RosettaTranslationFileBundle) => void;
  segments: TranslationSegment[];
  translationFileId: string;
}) {
  const bundle = await saveRosettaTranslationSegments(
    jobId,
    translationFileId,
    segments
  );
  onTranslationFileSaved?.(bundle);
  return bundle.segments;
}
