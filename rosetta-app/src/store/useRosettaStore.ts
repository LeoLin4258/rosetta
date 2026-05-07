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

export const useRosettaStore = create<RosettaState>()(
  persist(
    (set) => ({
      themeMode: "dark",
      rwkv: {
        baseUrl: "http://127.0.0.1:8000",
        batchEndpoint: "/translate/v1/batch-translate",
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
      createDemoJob: () =>
        set((state) => ({
          jobs: [demoJob, ...state.jobs.filter((job) => job.id !== demoJob.id)],
          previewSegments: demoSegments,
        })),
    }),
    {
      name: "rosetta-app-settings",
      partialize: (state) => ({
        themeMode: state.themeMode,
        rwkv: state.rwkv,
      }),
    }
  )
);
