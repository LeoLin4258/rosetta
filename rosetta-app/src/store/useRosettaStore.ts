import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  AppThemeMode,
  RosettaJob,
  RwkvConnectionConfig,
  Segment,
  TranslationMode,
} from "../types/rosetta";

type RosettaState = {
  themeMode: AppThemeMode;
  rwkv: RwkvConnectionConfig;
  jobs: RosettaJob[];
  previewSegments: Segment[];
  setThemeMode: (mode: AppThemeMode) => void;
  updateRwkvConfig: (config: Partial<RwkvConnectionConfig>) => void;
  setTranslationMode: (mode: TranslationMode) => void;
  beginPreviewSegmentTranslation: (segmentIds: string[]) => void;
  completePreviewSegmentTranslation: (
    segmentIds: string[],
    translations: string[]
  ) => void;
  failPreviewSegmentTranslation: (segmentIds: string[]) => void;
  createDemoJob: () => void;
};

const now = new Date().toISOString();

const demoSegments: Segment[] = [
  {
    id: "segment-1",
    blockId: "block-1",
    order: 1,
    sourceText: "Rosetta keeps document structure outside of the model.",
    translatedText: "Rosetta 将文档结构保留在模型之外。",
    targetLang: "zh-CN",
    kind: "paragraph",
    preserveWhitespace: true,
    status: "done",
  },
  {
    id: "segment-2",
    blockId: "block-2",
    order: 2,
    sourceText: "Code blocks, links, and table boundaries should remain intact.",
    translatedText: "代码块、链接和表格边界应保持完整。",
    targetLang: "zh-CN",
    kind: "paragraph",
    preserveWhitespace: true,
    status: "done",
  },
  {
    id: "segment-3",
    blockId: "block-3",
    order: 3,
    sourceText: "Batch translation will be validated before the scheduler is built.",
    targetLang: "zh-CN",
    kind: "paragraph",
    preserveWhitespace: true,
    status: "pending",
  },
];

const demoJob: RosettaJob = {
  id: "job-demo",
  filename: "demo.md",
  status: "ready",
  createdAt: now,
  updatedAt: now,
  targetLang: "zh-CN",
  segmentCount: demoSegments.length,
  completedSegments: 2,
  failedSegments: 0,
};

function syncDemoJobWithSegments(jobs: RosettaJob[], segments: Segment[]) {
  const completedSegments = segments.filter((segment) =>
    ["done", "edited", "skipped"].includes(segment.status)
  ).length;
  const failedSegments = segments.filter(
    (segment) => segment.status === "failed"
  ).length;
  const translatingSegments = segments.filter(
    (segment) => segment.status === "translating"
  ).length;
  const pendingSegments = segments.filter(
    (segment) => segment.status === "pending"
  ).length;
  const status: RosettaJob["status"] =
    translatingSegments > 0
      ? "translating"
      : failedSegments > 0
        ? "failed"
        : pendingSegments > 0
          ? "ready"
          : "completed";

  return jobs.map((job) =>
    job.id === demoJob.id
      ? {
          ...job,
          status,
          updatedAt: new Date().toISOString(),
          segmentCount: segments.length,
          completedSegments,
          failedSegments,
        }
      : job
  );
}

export const useRosettaStore = create<RosettaState>()(
  persist(
    (set) => ({
      themeMode: "dark",
      rwkv: {
        baseUrl: "https://rwkvconcszserver3.rwkvos.com",
        endpoint: "/v1/chat/completions",
        internalToken: "",
        bodyPassword: "",
        timeoutMs: 120_000,
        mode: "balanced",
      },
      jobs: [demoJob],
      previewSegments: demoSegments,
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
      beginPreviewSegmentTranslation: (segmentIds) =>
        set((state) => {
          const segmentIdSet = new Set(segmentIds);
          const previewSegments: Segment[] = state.previewSegments.map((segment) =>
            segmentIdSet.has(segment.id)
              ? {
                  ...segment,
                  status: "translating" as const,
                  translatedText: undefined,
                }
              : segment
          );

          return {
            previewSegments,
            jobs: syncDemoJobWithSegments(state.jobs, previewSegments),
          };
        }),
      completePreviewSegmentTranslation: (segmentIds, translations) =>
        set((state) => {
          const translationById = new Map(
            segmentIds.map((segmentId, index) => [
              segmentId,
              translations[index],
            ])
          );
          const previewSegments: Segment[] = state.previewSegments.map((segment) => {
            if (!translationById.has(segment.id)) {
              return segment;
            }

            return {
              ...segment,
              translatedText: translationById.get(segment.id),
              status: "done" as const,
            };
          });

          return {
            previewSegments,
            jobs: syncDemoJobWithSegments(state.jobs, previewSegments),
          };
        }),
      failPreviewSegmentTranslation: (segmentIds) =>
        set((state) => {
          const segmentIdSet = new Set(segmentIds);
          const previewSegments: Segment[] = state.previewSegments.map((segment) =>
            segmentIdSet.has(segment.id)
              ? {
                  ...segment,
                  status: "failed" as const,
                }
              : segment
          );

          return {
            previewSegments,
            jobs: syncDemoJobWithSegments(state.jobs, previewSegments),
          };
        }),
      createDemoJob: () =>
        set((state) => ({
          jobs: [demoJob, ...state.jobs.filter((job) => job.id !== demoJob.id)],
          previewSegments: demoSegments,
        })),
    }),
    {
      name: "rosetta-app-settings",
      merge: (persisted, current) => {
        const persistedState = persisted as Partial<RosettaState> | undefined;

        return {
          ...current,
          ...persistedState,
          rwkv: {
            ...current.rwkv,
            ...persistedState?.rwkv,
          },
        };
      },
      partialize: (state) => ({
        themeMode: state.themeMode,
        rwkv: state.rwkv,
      }),
    }
  )
);
