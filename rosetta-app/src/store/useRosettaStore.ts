import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  ActiveTranslationRun,
  AppThemeMode,
  RosettaDocument,
  RosettaJobBundle,
  RosettaJobSummary,
  RosettaTranslationFile,
  RosettaTranslationFileBundle,
  RwkvConnectionConfig,
  Segment,
  TranslationSegment,
  TranslationRevision,
  TranslationMode,
} from "../types/rosetta";

type RosettaState = {
  themeMode: AppThemeMode;
  rwkv: RwkvConnectionConfig;
  jobs: RosettaJobSummary[];
  activeJobId: string | null;
  activeFileId: string | null;
  activeFileIdByJobId: Record<string, string>;
  activeDocument: RosettaDocument | null;
  previewSegments: Segment[];
  translationFiles: RosettaTranslationFile[];
  activeSourceFileId: string | null;
  activeTranslationFileId: string | null;
  activeSourceFileIdByJobId: Record<string, string>;
  activeTranslationFileIdBySourceKey: Record<string, string>;
  translationSegments: TranslationSegment[];
  translationRevisions: TranslationRevision[];
  activeTranslationRun: ActiveTranslationRun | null;
  setThemeMode: (mode: AppThemeMode) => void;
  updateRwkvConfig: (config: Partial<RwkvConnectionConfig>) => void;
  setTranslationMode: (mode: TranslationMode) => void;
  setJobList: (jobs: RosettaJobSummary[]) => void;
  setActiveJobId: (jobId: string | null) => void;
  setActiveFileId: (fileId: string | null) => void;
  setActiveJobSelection: (
    jobId: string,
    sourceFileId: string | null,
    translationFileId?: string | null
  ) => void;
  setActiveBundle: (bundle: RosettaJobBundle) => void;
  refreshJobBundle: (bundle: RosettaJobBundle) => void;
  setActiveTranslationFileBundle: (
    bundle: RosettaTranslationFileBundle
  ) => void;
  upsertTranslationFile: (translationFile: RosettaTranslationFile) => void;
  clearActiveJob: () => void;
  updateActiveSegments: (segments: Segment[]) => void;
  updateActiveTranslationSegments: (segments: TranslationSegment[]) => void;
  beginTranslationFileSegmentTranslation: (
    segmentIds: string[]
  ) => TranslationSegment[];
  completeTranslationFileSegmentTranslation: (
    segmentIds: string[],
    translations: string[]
  ) => TranslationSegment[];
  failTranslationFileSegmentTranslation: (
    segmentIds: string[],
    error?: string
  ) => TranslationSegment[];
  preparePreviewSegmentRetranslation: (segmentIds: string[]) => Segment[];
  startTranslationRun: (
    run: Omit<ActiveTranslationRun, "completedSegmentIds" | "failedSegmentIds" | "startedAt"> & {
      startedAt?: string;
    }
  ) => void;
  markTranslationRunCompleted: (runId: string, segmentIds: string[]) => void;
  markTranslationRunFailed: (runId: string, segmentIds: string[]) => void;
  finishTranslationRun: (runId: string) => void;
  beginPreviewSegmentTranslation: (segmentIds: string[]) => Segment[];
  completePreviewSegmentTranslation: (
    segmentIds: string[],
    translations: string[]
  ) => Segment[];
  failPreviewSegmentTranslation: (segmentIds: string[], error?: string) => Segment[];
};

function syncJobWithSegments(
  job: RosettaJobSummary,
  segments: Segment[],
  lastError?: string
): RosettaJobSummary {
  const completedSegments = segments.filter((segment) =>
    ["done", "edited", "skipped"].includes(segment.status)
  ).length;
  const failedSegments = segments.filter(
    (segment) => segment.status === "failed"
  ).length;
  const translatingSegments = segments.filter(
    (segment) => segment.status === "translating"
  ).length;
  const status =
    translatingSegments > 0
      ? "translating"
      : failedSegments > 0
        ? "failed"
        : completedSegments === segments.length
          ? "completed"
          : "ready";

  return {
    ...job,
    status,
    updatedAt: Date.now().toString(),
    lastError,
    segmentCount: segments.length,
    completedSegments,
    failedSegments,
  };
}

function replaceJob(
  jobs: RosettaJobSummary[],
  job: RosettaJobSummary
): RosettaJobSummary[] {
  return [job, ...jobs.filter((candidate) => candidate.id !== job.id)].sort(
    (left, right) => right.updatedAt.localeCompare(left.updatedAt)
  );
}

function applySegments(
  state: RosettaState,
  segments: Segment[],
  lastError?: string
) {
  const activeJob = state.jobs.find((job) => job.id === state.activeJobId);
  const jobs = activeJob
    ? replaceJob(state.jobs, syncJobWithSegments(activeJob, segments, lastError))
    : state.jobs;

  return {
    jobs,
    previewSegments: segments,
  };
}

function sourceSelectionKey(jobId: string, sourceFileId: string) {
  return `${jobId}:${sourceFileId}`;
}

export const useRosettaStore = create<RosettaState>()(
  persist(
    (set, get) => ({
      themeMode: "dark",
      rwkv: {
        baseUrl: "https://rwkvconcszserver3.rwkvos.com",
        endpoint: "/v1/chat/completions",
        internalToken: "",
        bodyPassword: "",
        timeoutMs: 120_000,
        mode: "balanced",
      },
      jobs: [],
      activeJobId: null,
      activeFileId: null,
      activeFileIdByJobId: {},
      activeDocument: null,
      previewSegments: [],
      translationFiles: [],
      activeSourceFileId: null,
      activeTranslationFileId: null,
      activeSourceFileIdByJobId: {},
      activeTranslationFileIdBySourceKey: {},
      translationSegments: [],
      translationRevisions: [],
      activeTranslationRun: null,
      setThemeMode: (mode) => set({ themeMode: mode }),
      updateRwkvConfig: (config) =>
        set((state) => ({
          rwkv: {
            ...state.rwkv,
            ...config,
          },
        })),
      setTranslationMode: (mode) =>
        set((state) => ({
          rwkv: {
            ...state.rwkv,
            mode,
          },
        })),
      setJobList: (jobs) =>
        set((state) => {
          const activeJobStillExists =
            state.activeJobId != null &&
            jobs.some((job) => job.id === state.activeJobId);
          const validJobIds = new Set(jobs.map((job) => job.id));
          const nextActiveFileIdByJobId = Object.fromEntries(
            Object.entries(state.activeFileIdByJobId).filter(([jobId]) =>
              validJobIds.has(jobId)
            )
          ) as Record<string, string>;

          for (const job of jobs) {
            const selectedFileId = nextActiveFileIdByJobId[job.id];
            const fileStillExists =
              selectedFileId != null &&
              job.sourceFiles.some((file) => file.id === selectedFileId);

            if (!fileStillExists && job.sourceFiles[0]) {
              nextActiveFileIdByJobId[job.id] = job.sourceFiles[0].id;
            }
          }

          if (!activeJobStillExists) {
            return {
              jobs,
              activeJobId: null,
              activeFileId: null,
              activeFileIdByJobId: nextActiveFileIdByJobId,
              activeSourceFileId: null,
              activeTranslationFileId: null,
              activeDocument: null,
              previewSegments: [],
              translationFiles: [],
              translationSegments: [],
              translationRevisions: [],
              activeTranslationRun: null,
            };
          }

          return {
            jobs,
            activeFileId: state.activeJobId
              ? nextActiveFileIdByJobId[state.activeJobId] ?? state.activeFileId
              : state.activeFileId,
            activeFileIdByJobId: nextActiveFileIdByJobId,
          };
        }),
      setActiveJobId: (jobId) =>
        set((state) => {
          const job = state.jobs.find((candidate) => candidate.id === jobId);
          const selectedFileId = jobId
            ? state.activeFileIdByJobId[jobId] ?? job?.sourceFiles[0]?.id ?? null
            : null;
          const isSwitchingJob = state.activeJobId !== jobId;

          return {
            activeJobId: jobId,
            activeFileId: selectedFileId,
            activeSourceFileId: selectedFileId,
            activeTranslationFileId: jobId && selectedFileId
              ? state.activeTranslationFileIdBySourceKey[
                  sourceSelectionKey(jobId, selectedFileId)
                ] ?? null
              : null,
            activeDocument: isSwitchingJob ? null : state.activeDocument,
            previewSegments: isSwitchingJob ? [] : state.previewSegments,
            translationFiles: isSwitchingJob ? [] : state.translationFiles,
            translationSegments: isSwitchingJob ? [] : state.translationSegments,
            translationRevisions: isSwitchingJob
              ? []
              : state.translationRevisions,
            activeTranslationRun: isSwitchingJob
              ? null
              : state.activeTranslationRun,
          };
        }),
      setActiveFileId: (fileId) =>
        set((state) => {
          const activeFileIdByJobId = { ...state.activeFileIdByJobId };
          if (state.activeJobId) {
            if (fileId) {
              activeFileIdByJobId[state.activeJobId] = fileId;
            } else {
              delete activeFileIdByJobId[state.activeJobId];
            }
          }

          return {
            activeFileId: fileId,
            activeSourceFileId: fileId,
            activeTranslationFileId:
              state.activeJobId && fileId
                ? state.activeTranslationFileIdBySourceKey[
                    sourceSelectionKey(state.activeJobId, fileId)
                  ] ?? null
                : null,
            activeFileIdByJobId,
          };
        }),
      setActiveJobSelection: (jobId, fileId, translationFileId = null) =>
        set((state) => {
          const activeFileIdByJobId = { ...state.activeFileIdByJobId };
          const activeSourceFileIdByJobId = {
            ...state.activeSourceFileIdByJobId,
          };
          const activeTranslationFileIdBySourceKey = {
            ...state.activeTranslationFileIdBySourceKey,
          };
          const isSwitchingJob = state.activeJobId !== jobId;
          if (fileId) {
            activeFileIdByJobId[jobId] = fileId;
            activeSourceFileIdByJobId[jobId] = fileId;
            const key = sourceSelectionKey(jobId, fileId);
            if (translationFileId) {
              activeTranslationFileIdBySourceKey[key] = translationFileId;
            } else {
              delete activeTranslationFileIdBySourceKey[key];
            }
          } else {
            delete activeFileIdByJobId[jobId];
            delete activeSourceFileIdByJobId[jobId];
          }

          return {
            activeJobId: jobId,
            activeFileId: fileId,
            activeFileIdByJobId,
            activeSourceFileId: fileId,
            activeTranslationFileId: translationFileId,
            activeSourceFileIdByJobId,
            activeTranslationFileIdBySourceKey,
            activeDocument: isSwitchingJob ? null : state.activeDocument,
            previewSegments: isSwitchingJob ? [] : state.previewSegments,
            translationFiles: isSwitchingJob ? [] : state.translationFiles,
            translationSegments: isSwitchingJob ? [] : state.translationSegments,
            translationRevisions: isSwitchingJob
              ? []
              : state.translationRevisions,
            activeTranslationRun: isSwitchingJob
              ? null
              : state.activeTranslationRun,
          };
        }),
      setActiveBundle: (bundle) =>
        set((state) => {
          const fileIds = bundle.document.files.map((file) => file.id);
          const mappedFileId = state.activeFileIdByJobId[bundle.job.id];
          const selectedFileId =
            mappedFileId != null && fileIds.includes(mappedFileId)
              ? mappedFileId
              : bundle.document.files[0]?.id ?? null;
          const activeFileIdByJobId = { ...state.activeFileIdByJobId };
          if (selectedFileId) {
            activeFileIdByJobId[bundle.job.id] = selectedFileId;
          }

          return {
            jobs: replaceJob(state.jobs, bundle.job),
            activeJobId: bundle.job.id,
            activeFileId: selectedFileId,
            activeSourceFileId: selectedFileId,
            activeTranslationFileId:
              selectedFileId != null
                ? state.activeTranslationFileIdBySourceKey[
                    sourceSelectionKey(bundle.job.id, selectedFileId)
                  ] ??
                  bundle.translationFiles.find(
                    (file) => file.sourceFileId === selectedFileId
                  )?.id ??
                  null
                : null,
            activeFileIdByJobId,
            activeDocument: bundle.document,
            previewSegments: bundle.segments,
            translationFiles: bundle.translationFiles ?? [],
            translationSegments: [],
            translationRevisions: bundle.translationRevisions ?? [],
          };
        }),
      refreshJobBundle: (bundle) =>
        set((state) => {
          const jobs = replaceJob(state.jobs, bundle.job);

          if (state.activeJobId !== bundle.job.id) {
            return { jobs };
          }

          const fileIds = bundle.document.files.map((file) => file.id);
          const mappedFileId = state.activeFileIdByJobId[bundle.job.id];
          const selectedFileId =
            mappedFileId != null && fileIds.includes(mappedFileId)
              ? mappedFileId
              : bundle.document.files[0]?.id ?? null;
          const activeFileIdByJobId = { ...state.activeFileIdByJobId };
          if (selectedFileId) {
            activeFileIdByJobId[bundle.job.id] = selectedFileId;
          } else {
            delete activeFileIdByJobId[bundle.job.id];
          }

          return {
            jobs,
            activeFileId: selectedFileId,
            activeSourceFileId: selectedFileId,
            activeTranslationFileId:
              selectedFileId != null
                ? state.activeTranslationFileIdBySourceKey[
                    sourceSelectionKey(bundle.job.id, selectedFileId)
                  ] ??
                  bundle.translationFiles.find(
                    (file) => file.sourceFileId === selectedFileId
                  )?.id ??
                  null
                : null,
            activeFileIdByJobId,
            activeDocument: bundle.document,
            previewSegments: bundle.segments,
            translationFiles: bundle.translationFiles ?? [],
            translationRevisions: bundle.translationRevisions ?? [],
          };
        }),
      setActiveTranslationFileBundle: (bundle) =>
        set((state) => {
          const translationFiles = [
            bundle.translationFile,
            ...state.translationFiles.filter(
              (file) => file.id !== bundle.translationFile.id
            ),
          ].sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
          const activeTranslationFileIdBySourceKey = {
            ...state.activeTranslationFileIdBySourceKey,
          };
          if (state.activeJobId) {
            activeTranslationFileIdBySourceKey[
              sourceSelectionKey(
                state.activeJobId,
                bundle.translationFile.sourceFileId
              )
            ] = bundle.translationFile.id;
          }

          return {
            translationFiles,
            activeSourceFileId: bundle.translationFile.sourceFileId,
            activeFileId: bundle.translationFile.sourceFileId,
            activeTranslationFileId: bundle.translationFile.id,
            activeTranslationFileIdBySourceKey,
            translationSegments: bundle.segments,
          };
        }),
      upsertTranslationFile: (translationFile) =>
        set((state) => ({
          translationFiles: [
            translationFile,
            ...state.translationFiles.filter((file) => file.id !== translationFile.id),
          ].sort((left, right) => right.updatedAt.localeCompare(left.updatedAt)),
        })),
      clearActiveJob: () =>
        set({
          activeJobId: null,
          activeFileId: null,
          activeSourceFileId: null,
          activeTranslationFileId: null,
          activeDocument: null,
          previewSegments: [],
          translationFiles: [],
          translationSegments: [],
          translationRevisions: [],
          activeTranslationRun: null,
        }),
      updateActiveSegments: (segments) => set((state) => applySegments(state, segments)),
      updateActiveTranslationSegments: (segments) =>
        set({ translationSegments: segments }),
      beginTranslationFileSegmentTranslation: (segmentIds) => {
        const segmentIdSet = new Set(segmentIds);
        const nextSegments = get().translationSegments.map((segment) =>
          segmentIdSet.has(segment.sourceSegmentId)
            ? {
                ...segment,
                status: "translating" as const,
                translatedText: undefined,
                error: undefined,
              }
            : segment
        );
        set({ translationSegments: nextSegments });
        return nextSegments;
      },
      completeTranslationFileSegmentTranslation: (segmentIds, translations) => {
        const translationById = new Map(
          segmentIds.map((segmentId, index) => [segmentId, translations[index]])
        );
        const nextSegments = get().translationSegments.map((segment) =>
          translationById.has(segment.sourceSegmentId)
            ? {
                ...segment,
                translatedText: translationById.get(segment.sourceSegmentId),
                status: "done" as const,
                error: undefined,
              }
            : segment
        );
        set({ translationSegments: nextSegments });
        return nextSegments;
      },
      failTranslationFileSegmentTranslation: (segmentIds, error) => {
        const segmentIdSet = new Set(segmentIds);
        const nextSegments = get().translationSegments.map((segment) =>
          segmentIdSet.has(segment.sourceSegmentId)
            ? {
                ...segment,
                status: "failed" as const,
                error,
              }
            : segment
        );
        set({ translationSegments: nextSegments });
        return nextSegments;
      },
      preparePreviewSegmentRetranslation: (segmentIds) => {
        const segmentIdSet = new Set(segmentIds);
        const nextSegments = get().previewSegments.map((segment) =>
          segmentIdSet.has(segment.id)
            ? {
                ...segment,
                status: "pending" as const,
                translatedText: undefined,
                error: undefined,
              }
            : segment
        );
        set((state) => applySegments(state, nextSegments));
        return nextSegments;
      },
      startTranslationRun: (run) =>
        set({
          activeTranslationRun: {
            ...run,
            completedSegmentIds: [],
            failedSegmentIds: [],
            startedAt: run.startedAt ?? Date.now().toString(),
          },
        }),
      markTranslationRunCompleted: (runId, segmentIds) =>
        set((state) => {
          if (!state.activeTranslationRun || state.activeTranslationRun.id !== runId) {
            return state;
          }
          const completed = new Set(state.activeTranslationRun.completedSegmentIds);
          const failed = new Set(state.activeTranslationRun.failedSegmentIds);
          for (const segmentId of segmentIds) {
            completed.add(segmentId);
            failed.delete(segmentId);
          }
          return {
            activeTranslationRun: {
              ...state.activeTranslationRun,
              completedSegmentIds: [...completed],
              failedSegmentIds: [...failed],
            },
          };
        }),
      markTranslationRunFailed: (runId, segmentIds) =>
        set((state) => {
          if (!state.activeTranslationRun || state.activeTranslationRun.id !== runId) {
            return state;
          }
          const failed = new Set(state.activeTranslationRun.failedSegmentIds);
          for (const segmentId of segmentIds) {
            failed.add(segmentId);
          }
          return {
            activeTranslationRun: {
              ...state.activeTranslationRun,
              failedSegmentIds: [...failed],
            },
          };
        }),
      finishTranslationRun: (runId) =>
        set((state) =>
          state.activeTranslationRun?.id === runId
            ? { activeTranslationRun: null }
            : state
        ),
      beginPreviewSegmentTranslation: (segmentIds) => {
        const segmentIdSet = new Set(segmentIds);
        const nextSegments = get().previewSegments.map((segment) =>
          segmentIdSet.has(segment.id)
            ? {
                ...segment,
                status: "translating" as const,
                translatedText: undefined,
                error: undefined,
              }
            : segment
        );
        set((state) => applySegments(state, nextSegments));
        return nextSegments;
      },
      completePreviewSegmentTranslation: (segmentIds, translations) => {
        const translationById = new Map(
          segmentIds.map((segmentId, index) => [segmentId, translations[index]])
        );
        const nextSegments = get().previewSegments.map((segment) =>
          translationById.has(segment.id)
            ? {
                ...segment,
                translatedText: translationById.get(segment.id),
                status: "done" as const,
                error: undefined,
              }
            : segment
        );
        set((state) => applySegments(state, nextSegments));
        return nextSegments;
      },
      failPreviewSegmentTranslation: (segmentIds, error) => {
        const segmentIdSet = new Set(segmentIds);
        const nextSegments = get().previewSegments.map((segment) =>
          segmentIdSet.has(segment.id)
            ? {
                ...segment,
                status: "failed" as const,
                error,
              }
            : segment
        );
        set((state) => applySegments(state, nextSegments, error));
        return nextSegments;
      },
    }),
    {
      name: "rosetta-app-settings",
      merge: (persisted, current) => {
        const persistedState = persisted as Partial<RosettaState> | undefined;

        return {
          ...current,
          themeMode: persistedState?.themeMode ?? current.themeMode,
          activeJobId: persistedState?.activeJobId ?? current.activeJobId,
          activeFileId: persistedState?.activeFileId ?? current.activeFileId,
          activeFileIdByJobId:
            persistedState?.activeFileIdByJobId ??
            (persistedState?.activeJobId && persistedState?.activeFileId
              ? { [persistedState.activeJobId]: persistedState.activeFileId }
              : current.activeFileIdByJobId),
          activeSourceFileId:
            persistedState?.activeSourceFileId ??
            persistedState?.activeFileId ??
            current.activeSourceFileId,
          activeTranslationFileId:
            persistedState?.activeTranslationFileId ??
            current.activeTranslationFileId,
          activeSourceFileIdByJobId:
            persistedState?.activeSourceFileIdByJobId ??
            persistedState?.activeFileIdByJobId ??
            current.activeSourceFileIdByJobId,
          activeTranslationFileIdBySourceKey:
            persistedState?.activeTranslationFileIdBySourceKey ??
            current.activeTranslationFileIdBySourceKey,
          rwkv: {
            ...current.rwkv,
            ...persistedState?.rwkv,
          },
        };
      },
      partialize: (state) => ({
        themeMode: state.themeMode,
        rwkv: state.rwkv,
        activeJobId: state.activeJobId,
        activeFileId: state.activeFileId,
        activeFileIdByJobId: state.activeFileIdByJobId,
        activeSourceFileId: state.activeSourceFileId,
        activeTranslationFileId: state.activeTranslationFileId,
        activeSourceFileIdByJobId: state.activeSourceFileIdByJobId,
        activeTranslationFileIdBySourceKey:
          state.activeTranslationFileIdBySourceKey,
      }),
    }
  )
);
