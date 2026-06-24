import { selectProvider } from "@/lib/providers";
import {
  cancelRwkvTranslationRun,
  getRwkvTranslationRunStatus,
  startRwkvLlamaCppChatRun,
  startRwkvMobileBatchChatRun,
  startRwkvTranslationRun,
} from "@/lib/rwkvApi";
import type {
  RosettaTranslationFile,
  RosettaTranslationFileBundle,
  RwkvConnectionConfig,
  RwkvProviderHandle,
  RwkvTranslationRunStatus,
  Segment,
  SegmentStatus,
  TranslationSegment,
} from "@/types/rosetta";

export type TranslationRunResult = "completed" | "failed" | "noop" | "stopped";

export type TranslationRunTarget = Pick<Segment, "id" | "order" | "sourceText">;

const RUN_STATUS_POLL_MS = 300;

export function translationTargetsForStatuses({
  includeSkipped = false,
  sourceSegments,
  translationSegments,
  statuses,
}: {
  includeSkipped?: boolean;
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
      const status = statusBySourceSegmentId.get(segment.id);
      if (!includeSkipped && (segment.status === "skipped" || status === "skipped")) {
        return false;
      }
      if (statuses === "all") {
        return true;
      }
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
  provider,
  request,
  targets,
  translationFile,
}: {
  batchSize: number;
  cancelPromise?: Promise<"stopped">;
  jobId: string;
  onBatchCompleted?: (sourceSegmentIds: string[]) => void;
  onBatchFailed?: (sourceSegmentIds: string[]) => void;
  onTranslationFileSaved?: (bundle: RosettaTranslationFileBundle) => void;
  /**
   * Provider handle to dispatch the run through. When omitted, the runner
   * derives a `rwkv-lightning-contents` handle from `request` — preserving
   * pre-Phase-1 behavior for every existing call site.
   */
  provider?: RwkvProviderHandle;
  request: Omit<RwkvConnectionConfig, "mode"> & {
    sourceLang?: string | null;
    targetLang: string;
  };
  targets: TranslationRunTarget[];
  translationFile: RosettaTranslationFile;
}): Promise<TranslationRunResult> {
  if (targets.length === 0) {
    return "noop";
  }

  const runId = `run-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  const completed = new Set<string>();
  const failed = new Set<string>();
  let startError: unknown = null;
  let cancelRequested = false;

  const resolvedProvider: RwkvProviderHandle =
    provider ??
    selectProvider({
      config: {
        baseUrl: request.baseUrl,
        endpoint: request.endpoint,
        internalToken: request.internalToken,
        bodyPassword: request.bodyPassword,
        timeoutMs: request.timeoutMs,
        providerPreference: request.providerPreference,
      },
    });

  const startPromise = startRunForProvider({
    provider: resolvedProvider,
    runId,
    jobId,
    translationFileId: translationFile.id,
    sourceSegmentIds: targets.map((target) => target.id),
    sourceLang: request.sourceLang,
    targetLang: request.targetLang,
    batchSize,
  }).catch((error) => {
    startError = error;
    throw error;
  });

  cancelPromise?.then(() => {
    cancelRequested = true;
    void cancelRwkvTranslationRun(runId).catch(() => {});
  });

  try {
    while (true) {
      if (startError) {
        throw startError;
      }

      const status = await getRunStatusWithRetry(runId);
      if (status.translationFile && status.segments) {
        onTranslationFileSaved?.({
          translationFile: status.translationFile,
          segments: status.segments,
        });
      }

      const nextCompleted = status.completedSegmentIds.filter(
        (segmentId) => !completed.has(segmentId)
      );
      if (nextCompleted.length > 0) {
        nextCompleted.forEach((segmentId) => completed.add(segmentId));
        onBatchCompleted?.(nextCompleted);
      }

      const nextFailed = status.failedSegmentIds.filter(
        (segmentId) => !failed.has(segmentId)
      );
      if (nextFailed.length > 0) {
        nextFailed.forEach((segmentId) => failed.add(segmentId));
        onBatchFailed?.(nextFailed);
      }

      if (status.state === "completed") {
        await startPromise.catch(() => {});
        return "completed";
      }
      if (status.state === "failed") {
        await startPromise.catch(() => {});
        return "failed";
      }
      if (status.state === "cancelled") {
        await startPromise.catch(() => {});
        return "stopped";
      }

      if (cancelRequested && status.state === "running") {
        void cancelRwkvTranslationRun(runId).catch(() => {});
      }
      await delay(RUN_STATUS_POLL_MS);
    }
  } catch (error) {
    await startPromise.catch(() => {});
    throw error;
  }
}

function startRunForProvider(params: {
  provider: RwkvProviderHandle;
  runId: string;
  jobId: string;
  translationFileId: string;
  sourceSegmentIds: string[];
  sourceLang?: string | null;
  targetLang: string;
  batchSize: number;
}): Promise<RwkvTranslationRunStatus> {
  const {
    provider,
    runId,
    jobId,
    translationFileId,
    sourceSegmentIds,
    sourceLang,
    targetLang,
    batchSize,
  } = params;

  if (provider.id === "rwkv-mobile-batch-chat") {
    return startRwkvMobileBatchChatRun({
      runId,
      jobId,
      translationFileId,
      sourceSegmentIds,
      baseUrl: provider.baseUrl,
      timeoutMs: provider.timeoutMs,
      sourceLang,
      targetLang,
      batchSize,
    });
  }
  if (provider.id === "llama-cpp-chat-completions") {
    return startRwkvLlamaCppChatRun({
      runId,
      jobId,
      translationFileId,
      sourceSegmentIds,
      baseUrl: provider.baseUrl,
      timeoutMs: provider.timeoutMs,
      sourceLang,
      targetLang,
      batchSize,
    });
  }

  return startRwkvTranslationRun({
    runId,
    jobId,
    translationFileId,
    sourceSegmentIds,
    baseUrl: provider.baseUrl,
    endpoint: provider.endpoint,
    internalToken: provider.internalToken,
    bodyPassword: provider.bodyPassword,
    timeoutMs: provider.timeoutMs,
    sourceLang,
    targetLang,
    batchSize,
  });
}

async function getRunStatusWithRetry(runId: string) {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    try {
      return await getRwkvTranslationRunStatus(runId);
    } catch (error) {
      if (attempt === 4) {
        throw error;
      }
      await delay(RUN_STATUS_POLL_MS);
    }
  }
  return getRwkvTranslationRunStatus(runId);
}

function delay(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}
