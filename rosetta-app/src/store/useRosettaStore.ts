import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  ActiveTranslationRun,
  AppThemeMode,
  ManagedRuntimeInstallProgress,
  ManagedRuntimeStatus,
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
import type { Pdf2zhWorkerStatus } from "../lib/pdf2zhRuntime";

/**
 * Persisted proxy config used **only for remote downloads** of managed runtime
 * artifacts (today: the 1.3 GB model on HuggingFace). Loopback traffic to the
 * local sidecar always bypasses this.
 *
 * Why this exists: bundled `.app` launched from Finder doesn't inherit shell
 * env, so `HTTPS_PROXY` is invisible to reqwest. Users in regions where HF
 * needs a proxy need a UI surface to point Rosetta at one. See
 * `docs/engineering/change-log/2026-05-14-managed-rwkv-a1-fixes-loopback-no-proxy.md`
 * for the loopback-vs-egress reasoning.
 */
export type DownloadProxyConfig = {
  /** Proxy URL such as `http://127.0.0.1:7897` or `socks5://127.0.0.1:7898`.
   *  Empty string = no proxy (reqwest will still fall back to `HTTPS_PROXY`
   *  env if set). */
  url: string;
};

export type ManagedRuntimeSlice = {
  /**
   * Last `get_managed_rwkv_runtime_status` response. `null` before the first
   * call lands; afterwards refreshed on a coarse poll + after lifecycle/install
   * actions. Intentionally **not** persisted — every Rosetta launch re-probes.
   */
  status: ManagedRuntimeStatus | null;
  /**
   * Latest install-progress snapshot from the `managed-rwkv://install-progress`
   * Tauri event. `null` when no install has started this session.
   */
  progress: ManagedRuntimeInstallProgress | null;
  /**
   * Last error from any managed-runtime action (install / start / probe).
   * Cleared on the next successful action.
   */
  lastError: string | null;
};

/**
 * Live pdf2zh phase/percent/page progress for the active PDF translation run.
 * Stored app-level (keyed by jobId) instead of in component state so that
 * switching files and coming back doesn't lose the display.
 */
export type PdfRunProgress = {
  phase: string;
  percent: number | null;
  currentPage: number | null;
  totalPages: number | null;
  /** Cumulative characters returned by RWKV in the current run. */
  translatedChars: number | null;
};

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
  pdfRunProgressByJobId: Record<string, PdfRunProgress>;
  /**
   * Lifecycle of the persistent pdf2zh worker, shown in the app header so the
   * user can see whether the engine is warm. Updated by AppShell from the
   * `rosetta-pdf2zh-worker-status` Tauri event + a one-shot fetch on mount.
   * `null` until the first probe lands.
   */
  pdf2zhWorker: Pdf2zhWorkerStatus | null;
  managedRuntime: ManagedRuntimeSlice;
  downloadProxy: DownloadProxyConfig;
  defaultTargetLang: string;
  langByJobId: Record<string, { sourceLang: string; targetLang: string }>;
  setPdf2zhWorkerStatus: (status: Pdf2zhWorkerStatus | null) => void;
  setManagedRuntimeStatus: (status: ManagedRuntimeStatus | null) => void;
  setManagedRuntimeProgress: (
    progress: ManagedRuntimeInstallProgress | null
  ) => void;
  setManagedRuntimeError: (error: string | null) => void;
  setDownloadProxyUrl: (url: string) => void;
  setDefaultTargetLang: (lang: string) => void;
  setJobLangs: (jobId: string, sourceLang: string, targetLang: string) => void;
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
  clearJobHistory: () => void;
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
  setPdfRunProgress: (jobId: string, progress: PdfRunProgress | null) => void;
  beginPreviewSegmentTranslation: (segmentIds: string[]) => Segment[];
  completePreviewSegmentTranslation: (
    segmentIds: string[],
    translations: string[]
  ) => Segment[];
  failPreviewSegmentTranslation: (segmentIds: string[], error?: string) => Segment[];
};

function normalizePersistedLangByJobId(
  langByJobId: RosettaState["langByJobId"] | undefined
) {
  if (!langByJobId) {
    return undefined;
  }
  return Object.fromEntries(
    Object.entries(langByJobId).map(([jobId, langs]) => [
      jobId,
      {
        sourceLang: langs.sourceLang === "auto" ? "en" : langs.sourceLang,
        targetLang: langs.targetLang,
      },
    ])
  ) as RosettaState["langByJobId"];
}

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
    segments.length === 0
      ? "ready"
      : translatingSegments > 0
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
    (left, right) =>
      right.createdAt.localeCompare(left.createdAt) || right.id.localeCompare(left.id)
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

function syncJobWithTranslationFile(
  job: RosettaJobSummary,
  translationFile: RosettaTranslationFile
): RosettaJobSummary {
  if (!job.sourceFiles.some((file) => file.id === translationFile.sourceFileId)) {
    return job;
  }

  const updatedSourceFiles = job.sourceFiles.map((file) =>
    file.id === translationFile.sourceFileId
      ? {
          ...file,
          translationStatus: translationFile.status,
          segmentCount: translationFile.segmentCount,
          completedSegments: translationFile.completedSegments,
          failedSegments: translationFile.failedSegments,
          translatingSegments: translationFile.status === "translating" ? 1 : 0,
        }
      : file
  );
  const canAggregate =
    updatedSourceFiles.length > 0 &&
    updatedSourceFiles.every((file) => typeof file.segmentCount === "number");

  if (!canAggregate && updatedSourceFiles.length > 1) {
    return {
      ...job,
      sourceFiles: updatedSourceFiles,
    };
  }

  const segmentCount = canAggregate
    ? updatedSourceFiles.reduce((sum, file) => sum + (file.segmentCount ?? 0), 0)
    : translationFile.segmentCount;
  const completedSegments = canAggregate
    ? updatedSourceFiles.reduce((sum, file) => sum + (file.completedSegments ?? 0), 0)
    : translationFile.completedSegments;
  const failedSegments = canAggregate
    ? updatedSourceFiles.reduce((sum, file) => sum + (file.failedSegments ?? 0), 0)
    : translationFile.failedSegments;
  const translatingSegments = canAggregate
    ? updatedSourceFiles.reduce((sum, file) => sum + (file.translatingSegments ?? 0), 0)
    : translationFile.status === "translating"
      ? 1
      : 0;
  const status =
    translatingSegments > 0
      ? "translating"
      : failedSegments > 0
        ? "failed"
        : segmentCount > 0 && completedSegments >= segmentCount
          ? "completed"
          : "ready";

  return {
    ...job,
    sourceFiles: updatedSourceFiles,
    status,
    segmentCount,
    completedSegments,
    failedSegments,
  };
}

function syncJobWithTranslationFiles(
  job: RosettaJobSummary,
  translationFiles: RosettaTranslationFile[]
): RosettaJobSummary {
  return translationFiles.reduce(
    (syncedJob, translationFile) =>
      syncJobWithTranslationFile(syncedJob, translationFile),
    job
  );
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
        providerPreference: "local",
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
      pdfRunProgressByJobId: {},
      pdf2zhWorker: null,
      managedRuntime: {
        status: null,
        progress: null,
        lastError: null,
      },
      downloadProxy: {
        url: "",
      },
      defaultTargetLang: "zh-CN",
      langByJobId: {},
      setDownloadProxyUrl: (url) =>
        set(() => ({
          downloadProxy: { url: url.trim() },
        })),
      setDefaultTargetLang: (lang) => set({ defaultTargetLang: lang }),
      setJobLangs: (jobId, sourceLang, targetLang) =>
        set((state) => ({
          langByJobId: {
            ...state.langByJobId,
            [jobId]: { sourceLang, targetLang },
          },
        })),
      setPdf2zhWorkerStatus: (status) => set({ pdf2zhWorker: status }),
      setManagedRuntimeStatus: (status) =>
        set((state) => ({
          managedRuntime: {
            ...state.managedRuntime,
            status,
            // Deliberately do NOT touch `lastError` here. Status refreshes
            // happen after install/start/stop actions (success and failure);
            // wiping the error on every refresh hides the very reason the
            // user just tried to retry. Action handlers in
            // `useManagedRwkvRuntime` reset the error at the start of each
            // attempt instead.
          },
        })),
      setManagedRuntimeProgress: (progress) =>
        set((state) => ({
          managedRuntime: {
            ...state.managedRuntime,
            progress,
          },
        })),
      setManagedRuntimeError: (error) =>
        set((state) => ({
          managedRuntime: {
            ...state.managedRuntime,
            lastError: error,
          },
        })),
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
          const syncedJob = syncJobWithTranslationFiles(
            bundle.job,
            bundle.translationFiles ?? []
          );
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
          const selectedTranslationFile =
            selectedFileId != null
              ? bundle.translationFiles.find(
                  (file) =>
                    file.id ===
                    state.activeTranslationFileIdBySourceKey[
                      sourceSelectionKey(bundle.job.id, selectedFileId)
                    ]
                ) ??
                bundle.translationFiles.find(
                  (file) => file.sourceFileId === selectedFileId
                ) ??
                null
              : null;

          return {
            jobs: replaceJob(state.jobs, syncedJob),
            activeJobId: syncedJob.id,
            activeFileId: selectedFileId,
            activeSourceFileId: selectedFileId,
            activeTranslationFileId: selectedTranslationFile?.id ?? null,
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
          const syncedJob = syncJobWithTranslationFiles(
            bundle.job,
            bundle.translationFiles ?? []
          );
          const jobs = replaceJob(state.jobs, syncedJob);

          if (state.activeJobId !== syncedJob.id) {
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
          const selectedTranslationFile =
            selectedFileId != null
              ? bundle.translationFiles.find(
                  (file) =>
                    file.id ===
                    state.activeTranslationFileIdBySourceKey[
                      sourceSelectionKey(bundle.job.id, selectedFileId)
                    ]
                ) ??
                bundle.translationFiles.find(
                  (file) => file.sourceFileId === selectedFileId
                ) ??
                null
              : null;

          return {
            jobs,
            activeFileId: selectedFileId,
            activeSourceFileId: selectedFileId,
            activeTranslationFileId: selectedTranslationFile?.id ?? null,
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
        set((state) => {
          const translationFiles = [
            translationFile,
            ...state.translationFiles.filter((file) => file.id !== translationFile.id),
          ].sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
          const activeJob = state.jobs.find((job) => job.id === state.activeJobId);
          const jobs = activeJob
            ? replaceJob(state.jobs, syncJobWithTranslationFile(activeJob, translationFile))
            : state.jobs;

          return {
            jobs,
            translationFiles,
          };
        }),
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
      clearJobHistory: () =>
        set({
          jobs: [],
          activeJobId: null,
          activeFileId: null,
          activeFileIdByJobId: {},
          activeSourceFileId: null,
          activeTranslationFileId: null,
          activeSourceFileIdByJobId: {},
          activeTranslationFileIdBySourceKey: {},
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
        set((state) => {
          if (state.activeTranslationRun?.id !== runId) return state;
          const pdfProgress = { ...state.pdfRunProgressByJobId };
          delete pdfProgress[state.activeTranslationRun.jobId];
          return {
            activeTranslationRun: null,
            pdfRunProgressByJobId: pdfProgress,
          };
        }),
      setPdfRunProgress: (jobId, progress) =>
        set((state) => {
          const next = { ...state.pdfRunProgressByJobId };
          if (progress) next[jobId] = progress;
          else delete next[jobId];
          return { pdfRunProgressByJobId: next };
        }),
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
        const persistedRwkv = persistedState?.rwkv;
        const persistedHasProviderPreference =
          !!persistedRwkv &&
          Object.prototype.hasOwnProperty.call(
            persistedRwkv,
            "providerPreference"
          );

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
            ...persistedRwkv,
            providerPreference: persistedHasProviderPreference
              ? persistedRwkv.providerPreference
              : persistedRwkv
                ? "remote-api"
                : current.rwkv.providerPreference,
          },
          downloadProxy: {
            ...current.downloadProxy,
            ...persistedState?.downloadProxy,
          },
          defaultTargetLang:
            persistedState?.defaultTargetLang ?? current.defaultTargetLang,
          langByJobId:
            normalizePersistedLangByJobId(persistedState?.langByJobId) ??
            current.langByJobId,
        };
      },
      partialize: (state) => ({
        themeMode: state.themeMode,
        rwkv: state.rwkv,
        downloadProxy: state.downloadProxy,
        defaultTargetLang: state.defaultTargetLang,
        activeJobId: state.activeJobId,
        activeFileId: state.activeFileId,
        activeFileIdByJobId: state.activeFileIdByJobId,
        activeSourceFileId: state.activeSourceFileId,
        activeTranslationFileId: state.activeTranslationFileId,
        activeSourceFileIdByJobId: state.activeSourceFileIdByJobId,
        activeTranslationFileIdBySourceKey:
          state.activeTranslationFileIdBySourceKey,
        langByJobId: state.langByJobId,
      }),
    }
  )
);
