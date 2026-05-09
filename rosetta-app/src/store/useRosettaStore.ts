import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  ActiveTranslationRun,
  AppThemeMode,
  RosettaDocument,
  RosettaJobBundle,
  RosettaJobSummary,
  RwkvConnectionConfig,
  Segment,
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
  translationRevisions: TranslationRevision[];
  activeTranslationRun: ActiveTranslationRun | null;
  setThemeMode: (mode: AppThemeMode) => void;
  updateRwkvConfig: (config: Partial<RwkvConnectionConfig>) => void;
  setTranslationMode: (mode: TranslationMode) => void;
  setJobList: (jobs: RosettaJobSummary[]) => void;
  setActiveJobId: (jobId: string | null) => void;
  setActiveFileId: (fileId: string | null) => void;
  setActiveJobSelection: (jobId: string, fileId: string | null) => void;
  setActiveBundle: (bundle: RosettaJobBundle) => void;
  clearActiveJob: () => void;
  updateActiveSegments: (segments: Segment[]) => void;
  preparePreviewSegmentRetranslation: (segmentIds: string[]) => Segment[];
  startTranslationRun: (
    run: Omit<ActiveTranslationRun, "completedSegmentIds" | "failedSegmentIds" | "startedAt"> & {
      startedAt?: string;
    }
  ) => void;
  markTranslationRunCompleted: (segmentIds: string[]) => void;
  markTranslationRunFailed: (segmentIds: string[]) => void;
  finishTranslationRun: () => void;
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
          const nextActiveJobId = activeJobStillExists
            ? state.activeJobId
            : jobs[0]?.id ?? null;
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

          const nextActiveFileId = nextActiveJobId
            ? nextActiveFileIdByJobId[nextActiveJobId] ?? null
            : null;

          return {
            jobs,
            activeJobId: nextActiveJobId,
            activeFileId: nextActiveFileId,
            activeFileIdByJobId: nextActiveFileIdByJobId,
            activeDocument: activeJobStillExists ? state.activeDocument : null,
            previewSegments: activeJobStillExists ? state.previewSegments : [],
            translationRevisions: activeJobStillExists
              ? state.translationRevisions
              : [],
            activeTranslationRun: activeJobStillExists
              ? state.activeTranslationRun
              : null,
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
            activeDocument: isSwitchingJob ? null : state.activeDocument,
            previewSegments: isSwitchingJob ? [] : state.previewSegments,
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
            activeFileIdByJobId,
          };
        }),
      setActiveJobSelection: (jobId, fileId) =>
        set((state) => {
          const activeFileIdByJobId = { ...state.activeFileIdByJobId };
          const isSwitchingJob = state.activeJobId !== jobId;
          if (fileId) {
            activeFileIdByJobId[jobId] = fileId;
          } else {
            delete activeFileIdByJobId[jobId];
          }

          return {
            activeJobId: jobId,
            activeFileId: fileId,
            activeFileIdByJobId,
            activeDocument: isSwitchingJob ? null : state.activeDocument,
            previewSegments: isSwitchingJob ? [] : state.previewSegments,
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
            activeFileIdByJobId,
            activeDocument: bundle.document,
            previewSegments: bundle.segments,
            translationRevisions: bundle.translationRevisions ?? [],
          };
        }),
      clearActiveJob: () =>
        set({
          activeJobId: null,
          activeFileId: null,
          activeDocument: null,
          previewSegments: [],
          translationRevisions: [],
          activeTranslationRun: null,
        }),
      updateActiveSegments: (segments) => set((state) => applySegments(state, segments)),
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
      markTranslationRunCompleted: (segmentIds) =>
        set((state) => {
          if (!state.activeTranslationRun) {
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
      markTranslationRunFailed: (segmentIds) =>
        set((state) => {
          if (!state.activeTranslationRun) {
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
      finishTranslationRun: () => set({ activeTranslationRun: null }),
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
      }),
    }
  )
);
