import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  AppThemeMode,
  RosettaDocument,
  RosettaJobBundle,
  RosettaJobSummary,
  RwkvConnectionConfig,
  Segment,
  TranslationMode,
} from "../types/rosetta";

type RosettaState = {
  themeMode: AppThemeMode;
  rwkv: RwkvConnectionConfig;
  jobs: RosettaJobSummary[];
  activeJobId: string | null;
  activeDocument: RosettaDocument | null;
  previewSegments: Segment[];
  setThemeMode: (mode: AppThemeMode) => void;
  updateRwkvConfig: (config: Partial<RwkvConnectionConfig>) => void;
  setTranslationMode: (mode: TranslationMode) => void;
  setJobList: (jobs: RosettaJobSummary[]) => void;
  setActiveJobId: (jobId: string | null) => void;
  setActiveBundle: (bundle: RosettaJobBundle) => void;
  clearActiveJob: () => void;
  updateActiveSegments: (segments: Segment[]) => void;
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
      activeDocument: null,
      previewSegments: [],
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
          return {
            jobs,
            activeJobId: activeJobStillExists ? state.activeJobId : jobs[0]?.id ?? null,
            activeDocument: activeJobStillExists ? state.activeDocument : null,
            previewSegments: activeJobStillExists ? state.previewSegments : [],
          };
        }),
      setActiveJobId: (jobId) => set({ activeJobId: jobId }),
      setActiveBundle: (bundle) =>
        set((state) => ({
          jobs: replaceJob(state.jobs, bundle.job),
          activeJobId: bundle.job.id,
          activeDocument: bundle.document,
          previewSegments: bundle.segments,
        })),
      clearActiveJob: () =>
        set({
          activeJobId: null,
          activeDocument: null,
          previewSegments: [],
        }),
      updateActiveSegments: (segments) => set((state) => applySegments(state, segments)),
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
      }),
    }
  )
);
