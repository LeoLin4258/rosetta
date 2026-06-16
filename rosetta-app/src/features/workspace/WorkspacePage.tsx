import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  cancelRosettaTranslatedPdf,
  countRosettaPdfPages,
  createRosettaTranslationRevision,
  ensureRosettaTranslationFile,
  exportRosettaTranslatedPdf,
  exportRosettaTranslationFile,
  importRosettaDocumentFromPath,
  importRosettaProjectFromDirectory,
  loadRosettaJob,
  loadRosettaTranslationFile,
  pickRosettaExportPath,
  translateRosettaPdfPages,
  updateTxtSourceFile,
} from "@/lib/rosettaJobs";
import { selectProvider } from "@/lib/providers";
import { isManagedRuntimeReady } from "@/lib/useManagedRwkvRuntime";
import {
  runTranslationBatches,
  translationTargetsForStatuses,
} from "@/lib/translationRunner";
import { defaultExportFilename, exportFormatForSource } from "@/lib/rosettaExport";
import { useRosettaStore } from "@/store/useRosettaStore";
import type { RosettaJobBundle } from "@/types/rosetta";
import { DocumentPreview } from "@/features/preview/DocumentPreview";
import {
  isPdf2zhReady,
  useManagedPdf2zhRuntime,
} from "@/lib/useManagedPdf2zhRuntime";
import {
  prewarmPdf2zhWorker,
  type Pdf2zhInstallProgress,
} from "@/lib/pdf2zhRuntime";

import { WorkspaceEmpty } from "./WorkspaceEmpty";
import { WorkspaceTopbar } from "./WorkspaceTopbar";

const BATCH_SIZE = 16;
const DEFAULT_SOURCE_LANG = "en";

function normalizeSourceLang(lang?: string | null) {
  return lang && lang !== "auto" ? lang : DEFAULT_SOURCE_LANG;
}

export function WorkspacePage() {
  const activeJobId = useRosettaStore((s) => s.activeJobId);
  const activeDocument = useRosettaStore((s) => s.activeDocument);
  const activeSourceFileId = useRosettaStore((s) => s.activeSourceFileId);
  const activeTranslationFileId = useRosettaStore((s) => s.activeTranslationFileId);
  const previewSegments = useRosettaStore((s) => s.previewSegments);
  const translationSegments = useRosettaStore((s) => s.translationSegments);
  const translationFiles = useRosettaStore((s) => s.translationFiles);
  const activeTranslationRun = useRosettaStore((s) => s.activeTranslationRun);
  const jobs = useRosettaStore((s) => s.jobs);
  const rwkv = useRosettaStore((s) => s.rwkv);
  const managedRuntimeStatus = useRosettaStore((s) => s.managedRuntime.status);
  const pdf2zhWorkerStatus = useRosettaStore((s) => s.pdf2zhWorker);
  const defaultTargetLang = useRosettaStore((s) => s.defaultTargetLang);
  const langByJobId = useRosettaStore((s) => s.langByJobId);
  const setJobLangs = useRosettaStore((s) => s.setJobLangs);

  const setActiveBundle = useRosettaStore((s) => s.setActiveBundle);
  const refreshJobBundle = useRosettaStore((s) => s.refreshJobBundle);
  const setActiveTranslationFileBundle = useRosettaStore((s) => s.setActiveTranslationFileBundle);
  const upsertTranslationFile = useRosettaStore((s) => s.upsertTranslationFile);
  const updateActiveTranslationSegments = useRosettaStore((s) => s.updateActiveTranslationSegments);
  const startTranslationRun = useRosettaStore((s) => s.startTranslationRun);
  const markTranslationRunCompleted = useRosettaStore((s) => s.markTranslationRunCompleted);
  const markTranslationRunFailed = useRosettaStore((s) => s.markTranslationRunFailed);
  const finishTranslationRun = useRosettaStore((s) => s.finishTranslationRun);

  const [isDraggingOver, setIsDraggingOver] = useState(false);
  const [pageError, setPageError] = useState<string | null>(null);
  const [pdfError, setPdfError] = useState<string | null>(null);
  const [selectedBlockIds, setSelectedBlockIds] = useState<string[]>([]);
  const [pdfPageCount, setPdfPageCount] = useState(0);
  const [pdfSelectedPages, setPdfSelectedPages] = useState<number[]>([]);
  const [pdfForceRetranslate, setPdfForceRetranslate] = useState(false);
  const [hoveredBlockId, setHoveredBlockId] = useState<string | null>(null);
  const [isEditingSource, setIsEditingSource] = useState(false);
  const [sourceDraft, setSourceDraft] = useState("");
  const [isSavingSource, setIsSavingSource] = useState(false);
  // Live pdf2zh phase/page progress. Subscribed app-level in AppShell and
  // keyed by jobId, so it survives switching files mid-run.
  const pdfRunProgressByJobId = useRosettaStore((s) => s.pdfRunProgressByJobId);

  const cancelRef = useRef<(() => void) | null>(null);
  const pdf2zhRuntime = useManagedPdf2zhRuntime();

  // Per-job language selections, with fallback to document default / global default
  const jobLangs = activeJobId ? langByJobId[activeJobId] : undefined;
  const sourceLang = normalizeSourceLang(
    jobLangs?.sourceLang ?? activeDocument?.sourceLang
  );
  const targetLang = jobLangs?.targetLang ?? defaultTargetLang;

  function handleSourceLangChange(lang: string) {
    if (activeJobId) setJobLangs(activeJobId, lang, targetLang);
  }

  function handleTargetLangChange(lang: string) {
    if (activeJobId) setJobLangs(activeJobId, sourceLang, lang);
  }

  const activeJob = jobs.find((j) => j.id === activeJobId) ?? null;
  const activeTranslationFile =
    translationFiles.find((f) => f.id === activeTranslationFileId) ?? null;
  const sourceFile =
    activeDocument?.files.find((f) => f.id === activeSourceFileId) ??
    activeDocument?.files[0] ??
    null;
  const activeFileTranslationRun =
    activeTranslationRun &&
    activeTranslationRun.jobId === activeJobId &&
    activeTranslationRun.sourceFileId === activeSourceFileId
      ? activeTranslationRun
      : null;
  const isTranslating = !!activeFileTranslationRun;
  const isTranslationBusyElsewhere =
    !!activeTranslationRun && !activeFileTranslationRun;

  const completedCount = activeFileTranslationRun?.completedSegmentIds.length ?? 0;
  const totalCount = activeFileTranslationRun?.targetSegmentIds.length ?? 0;
  const pdfEngineProgressMessage = pdfInstallProgressMessage(pdf2zhRuntime.progress);

  // Reset block selection when switching documents
  useEffect(() => {
    setSelectedBlockIds([]);
    setPdfPageCount(0);
    setPdfSelectedPages([]);
    setPdfForceRetranslate(false);
    setIsEditingSource(false);
    setSourceDraft("");
  }, [activeDocument?.id]);

  useEffect(() => {
    setIsEditingSource(false);
    setSourceDraft("");
  }, [activeSourceFileId]);

  const handlePdfPageCountChange = useCallback((count: number) => {
    setPdfPageCount(count);
  }, []);

  const handlePdfSelectedPagesChange = useCallback((pages: number[]) => {
    setPdfSelectedPages(pages);
  }, []);

  const isPdfJob = sourceFile?.format === "pdf";
  const pdfProgress =
    isPdfJob && activeJobId ? pdfRunProgressByJobId[activeJobId] ?? null : null;
  const pdfEngineUnavailable =
    isPdfJob &&
    (pdf2zhWorkerStatus?.state === "not-installed" ||
      pdf2zhRuntime.status?.state === "not-installed");
  const pdfEngineUnavailableMessage =
    pdf2zhWorkerStatus?.message ??
    pdf2zhRuntime.status?.message ??
    "PDF 组件未安装，请在设置中安装后再翻译。";

  // Prewarm the persistent pdf2zh worker as soon as a PDF document is open,
  // so its ~13 s Python import overlaps with the user picking pages instead
  // of delaying the first translate click. The backend command checks pack
  // readiness itself (this hook's `status` is only populated lazily, so
  // gating on it here would never fire). Idempotent and fire-and-forget.
  useEffect(() => {
    if (!isPdfJob) return;
    void prewarmPdf2zhWorker().catch(() => {});
  }, [isPdfJob, activeJobId]);

  useEffect(() => {
    if (!isPdfJob) return;
    void pdf2zhRuntime.refreshStatus();
  }, [isPdfJob, activeJobId, pdf2zhRuntime.refreshStatus]);

  // After a document is loaded (or switched), restore translation segments if
  // there's a known active translation file but no segments in memory yet.
  useEffect(() => {
    if (!activeJobId || !activeTranslationFileId || !activeDocument || isTranslating) return;
    if (translationSegments.length > 0) return;

    void loadRosettaTranslationFile(activeJobId, activeTranslationFileId)
      .then((bundle) => setActiveTranslationFileBundle(bundle))
      .catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeDocument?.id, activeTranslationFileId, activeJobId]);

  // Register Tauri window file-drop events.
  // Use an `unmounted` flag so the async `.then(fn => ...)` callback can
  // immediately unsubscribe if React StrictMode already ran the cleanup before
  // the Promise resolved — without this, the first listener leaks and every
  // drop fires the handler twice.
  useEffect(() => {
    const appWindow = getCurrentWindow();
    let unmounted = false;
    let unlisten: (() => void) | null = null;

    appWindow
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setIsDraggingOver(true);
        } else if (event.payload.type === "leave") {
          setIsDraggingOver(false);
        } else if (event.payload.type === "drop") {
          setIsDraggingOver(false);
          void handleDroppedPaths(event.payload.paths);
        }
      })
      .then((fn) => {
        if (unmounted) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(console.error);

    return () => {
      unmounted = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleDroppedPaths(paths: string[]) {
    if (paths.length === 0) return;
    setPageError(null);

    for (const path of paths) {
      try {
        const bundle = await tryImportPath(path);
        setActiveBundle(bundle);
      } catch (err) {
      setPageError(errorMessage(err, "导入失败"));
      }
    }
  }

  async function tryImportPath(path: string): Promise<RosettaJobBundle> {
    const ext = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
    const isFile = ["txt", "md", "markdown", "pdf"].includes(ext);
    if (isFile) {
      return importRosettaDocumentFromPath(path);
    }
    return importRosettaProjectFromDirectory(path);
  }

  const handleImported = useCallback(
    (bundle: RosettaJobBundle) => {
      setActiveBundle(bundle);
    },
    [setActiveBundle]
  );

  function buildProvider() {
    return selectProvider({
      config: rwkv,
      override:
        rwkv.providerPreference === "local"
          ? "rwkv-mobile-batch-chat"
          : "rwkv-lightning-contents",
      managedRuntimeReady: isManagedRuntimeReady(managedRuntimeStatus),
      managedRuntimeBaseUrl: managedRuntimeStatus?.process.baseUrl ?? undefined,
    });
  }

  function buildCancelPair(): [Promise<"stopped">, () => void] {
    let resolve!: () => void;
    const promise = new Promise<"stopped">((r) => {
      resolve = () => r("stopped");
    });
    return [promise, resolve];
  }

  function isViewingTranslationFile(
    jobId: string,
    sourceFileId: string,
    translationFileId: string
  ) {
    const state = useRosettaStore.getState();
    return (
      state.activeJobId === jobId &&
      state.activeSourceFileId === sourceFileId &&
      state.activeTranslationFileId === translationFileId
    );
  }

  async function ensurePdf2zhReadyForTranslation() {
    const current = await pdf2zhRuntime.refreshStatus();
    if (isPdf2zhReady(current)) return;

    if (current?.state === "unsupported") {
      throw new Error(current.message);
    }

    throw new Error(
      current?.message ??
        "PDF 组件未安装。请先在设置中安装 PDF 组件，再返回翻译。"
    );
  }

  async function handleTranslate(targetLang: string, srcLang: string) {
    if (!activeJobId || !activeSourceFileId) return;
    setPageError(null);
    setPdfError(null);
    setSelectedBlockIds([]);

    // Declared outside try so the catch block can always call finishTranslationRun.
    let runId: string | null = null;

    try {
      const tfBundle = await ensureRosettaTranslationFile(
        activeJobId,
        activeSourceFileId,
        targetLang
      );
      setActiveTranslationFileBundle(tfBundle);

      if (sourceFile?.format === "pdf") {
        if (pdfSelectedPages.length === 0) {
          setPdfError("请选择要翻译的页面。");
          return;
        }
        const pageSelection = formatPageSelection(pdfSelectedPages);
        const force = pdfForceRetranslate || activeTranslationFile?.status === "translated";
        await handleTranslatePdfPages(pageSelection, force, targetLang, srcLang);
        return;
      }

      const targets = translationTargetsForStatuses({
        sourceSegments: previewSegments,
        translationSegments: tfBundle.segments,
        statuses: ["pending", "failed"],
      });

      if (targets.length === 0) return;

      runId = `run-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      const [cancelPromise, cancelResolve] = buildCancelPair();
      cancelRef.current = cancelResolve;

      startTranslationRun({
        id: runId,
        jobId: activeJobId,
        sourceFileId: activeSourceFileId,
        translationFileId: tfBundle.translationFile.id,
        scope: "file",
        targetSegmentIds: targets.map((t) => t.id),
      });

      const result = await runTranslationBatches({
        batchSize: BATCH_SIZE,
        cancelPromise,
        jobId: activeJobId,
        provider: buildProvider(),
        request: {
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          providerPreference: rwkv.providerPreference,
          sourceLang: srcLang,
          targetLang,
        },
        targets,
        translationFile: tfBundle.translationFile,
        onBatchCompleted: (ids) => markTranslationRunCompleted(runId!, ids),
        onBatchFailed: (ids) => markTranslationRunFailed(runId!, ids),
        onTranslationFileSaved: (saved) => {
          upsertTranslationFile(saved.translationFile);
          if (
            isViewingTranslationFile(
              activeJobId,
              activeSourceFileId,
              tfBundle.translationFile.id
            )
          ) {
            updateActiveTranslationSegments(saved.segments);
          }
        },
      });

      finishTranslationRun(runId!);
      cancelRef.current = null;

      if (result === "failed") {
        setPageError("翻译失败，请检查 API 配置或网络。");
      }

      // Use refreshJobBundle (not setActiveBundle) to preserve translation segments
      const freshBundle = await loadRosettaJob(activeJobId);
      refreshJobBundle(freshBundle);
    } catch (err) {
      const msg = errorMessage(err, "");
      if (!msg.includes("已取消")) {
        if (sourceFile?.format === "pdf") {
          setPdfError(errorMessage(err, "翻译出错。"));
        } else {
          setPageError(errorMessage(err, "翻译出错。"));
        }
      }
      if (runId) finishTranslationRun(runId);
    }
  }

  async function handleTranslatePdfPages(
    pageSelection: string,
    force: boolean,
    targetLangOverride = targetLang,
    sourceLangOverride = sourceLang,
  ) {
    if (!activeJobId || !activeSourceFileId) return null;
    const pageTargetLang = targetLangOverride;
    setPageError(null);
    setPdfError(null);
    setSelectedBlockIds([]);

    let runId: string | null = null;

    try {
      const tfBundle = await ensureRosettaTranslationFile(
        activeJobId,
        activeSourceFileId,
        pageTargetLang,
      );
      setActiveTranslationFileBundle(tfBundle);
      await ensurePdf2zhReadyForTranslation();
      const provider = buildProvider();

      runId = `run-pdf-pages-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      cancelRef.current = () => { void cancelRosettaTranslatedPdf(); };
      startTranslationRun({
        id: runId,
        jobId: activeJobId,
        sourceFileId: activeSourceFileId,
        translationFileId: tfBundle.translationFile.id,
        scope: "file",
        targetSegmentIds: [`pdf-pages:${pageSelection}`],
      });

      const state = await translateRosettaPdfPages(activeJobId, {
        pageSelection,
        targetLang: pageTargetLang,
        rwkvBaseUrl: provider.baseUrl,
        providerEndpoint: provider.id === "rwkv-lightning-contents" ? provider.endpoint : undefined,
        providerInternalToken: provider.id === "rwkv-lightning-contents" ? provider.internalToken : undefined,
        providerBodyPassword: provider.id === "rwkv-lightning-contents" ? provider.bodyPassword : undefined,
        sourceLang: normalizeSourceLang(sourceLangOverride),
        timeoutMs: rwkv.timeoutMs,
        force,
      });

      markTranslationRunCompleted(runId, [`pdf-pages:${pageSelection}`]);
      finishTranslationRun(runId);
      cancelRef.current = null;
      runId = null;
      const freshBundle = await loadRosettaJob(activeJobId);
      refreshJobBundle(freshBundle);
      const refreshedTranslation = freshBundle.translationFiles.find(
        (file) => file.id === tfBundle.translationFile.id,
      );
      if (refreshedTranslation) {
        setActiveTranslationFileBundle({
          translationFile: refreshedTranslation,
          segments: [],
        });
      }
      return state;
    } catch (err) {
      const msg = errorMessage(err, "");
      if (!msg.includes("已取消")) {
        setPdfError(errorMessage(err, "PDF 按页翻译出错。"));
      }
      if (runId) finishTranslationRun(runId);
      cancelRef.current = null;
      return null;
    }
  }

  async function handleRetranslateSelected() {
    if (!activeJobId || !activeSourceFileId || selectedBlockIds.length === 0) return;
    const retranslateTargetLang = activeTranslationFile?.targetLang ?? targetLang;
    setPageError(null);

    let runId: string | null = null;

    try {
      // Reset the selected blocks' segments to pending via a revision
      const revisionBundle = await createRosettaTranslationRevision(
        activeJobId,
        activeSourceFileId,
        "selection-retranslation",
        selectedBlockIds
      );
      // Only refresh if the backend included source segments; some backends return
      // an empty segments list for revision bundles, which would wipe the preview.
      if (revisionBundle.segments.length > 0) {
        refreshJobBundle(revisionBundle);
      }

      const tfBundle = await ensureRosettaTranslationFile(
        activeJobId,
        activeSourceFileId,
        retranslateTargetLang
      );

      // Use previewSegments from the store (always populated) rather than
      // revisionBundle.segments, which may be empty on some backends.
      const blockSegments = previewSegments.filter(
        (s) => selectedBlockIds.includes(s.blockId) && s.sourceText.trim()
      );
      const targets = translationTargetsForStatuses({
        sourceSegments: blockSegments,
        translationSegments: tfBundle.segments,
        statuses: "all",
      });

      if (targets.length === 0) return;

      // Only update the store's translation file state after confirming there are
      // segments to translate — avoids blanking the translation column on early return.
      setActiveTranslationFileBundle(tfBundle);

      runId = `run-sel-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      const [cancelPromise, cancelResolve] = buildCancelPair();
      cancelRef.current = cancelResolve;

      startTranslationRun({
        id: runId,
        jobId: activeJobId,
        sourceFileId: activeSourceFileId,
        translationFileId: tfBundle.translationFile.id,
        scope: "selection",
        targetSegmentIds: targets.map((t) => t.id),
      });

      await runTranslationBatches({
        batchSize: BATCH_SIZE,
        cancelPromise,
        jobId: activeJobId,
        provider: buildProvider(),
        request: {
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          providerPreference: rwkv.providerPreference,
          sourceLang,
          targetLang: retranslateTargetLang,
        },
        targets,
        translationFile: tfBundle.translationFile,
        onBatchCompleted: (ids) => markTranslationRunCompleted(runId!, ids),
        onBatchFailed: (ids) => markTranslationRunFailed(runId!, ids),
        onTranslationFileSaved: (saved) => {
          upsertTranslationFile(saved.translationFile);
          if (
            isViewingTranslationFile(
              activeJobId,
              activeSourceFileId,
              tfBundle.translationFile.id
            )
          ) {
            updateActiveTranslationSegments(saved.segments);
          }
        },
      });

      finishTranslationRun(runId!);
      cancelRef.current = null;
      setSelectedBlockIds([]);

      const freshBundle = await loadRosettaJob(activeJobId);
      refreshJobBundle(freshBundle);
    } catch (err) {
      setPageError(errorMessage(err, "重新翻译失败。"));
      if (runId) finishTranslationRun(runId);
    }
  }

  async function handleRetranslateAll() {
    if (!activeJobId || !activeSourceFileId) return;
    const retranslateTargetLang = activeTranslationFile?.targetLang ?? targetLang;
    setPageError(null);
    setPdfError(null);
    setSelectedBlockIds([]);

    let runId: string | null = null;

    try {
      if (sourceFile?.format === "pdf") {
        const tfBundle = await ensureRosettaTranslationFile(
          activeJobId,
          activeSourceFileId,
          retranslateTargetLang
        );
        setActiveTranslationFileBundle(tfBundle);
        await ensurePdf2zhReadyForTranslation();
        const provider = buildProvider();
        const pageCount = await countRosettaPdfPages(activeJobId, "source");
        const pageSelection = `1-${pageCount}`;
        runId = `run-pdf-all-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
        cancelRef.current = () => { void cancelRosettaTranslatedPdf(); };
        startTranslationRun({
          id: runId,
          jobId: activeJobId,
          sourceFileId: activeSourceFileId,
          translationFileId: tfBundle.translationFile.id,
          scope: "file",
          targetSegmentIds: [`pdf-pages:${pageSelection}`],
        });
        await translateRosettaPdfPages(activeJobId, {
          pageSelection,
          targetLang: retranslateTargetLang,
          rwkvBaseUrl: provider.baseUrl,
          providerEndpoint: provider.id === "rwkv-lightning-contents" ? provider.endpoint : undefined,
          providerInternalToken: provider.id === "rwkv-lightning-contents" ? provider.internalToken : undefined,
          providerBodyPassword: provider.id === "rwkv-lightning-contents" ? provider.bodyPassword : undefined,
          sourceLang,
          timeoutMs: rwkv.timeoutMs,
          force: true,
        });
        cancelRef.current = null;
        markTranslationRunCompleted(runId, [`pdf-pages:${pageSelection}`]);
        finishTranslationRun(runId);
        runId = null;
        const freshBundle = await loadRosettaJob(activeJobId);
        refreshJobBundle(freshBundle);
        const refreshedTranslation = freshBundle.translationFiles.find(
          (file) => file.id === tfBundle.translationFile.id,
        );
        if (refreshedTranslation) {
          setActiveTranslationFileBundle({
            translationFile: refreshedTranslation,
            segments: [],
          });
        }
        return;
      }

      const revisionBundle = await createRosettaTranslationRevision(
        activeJobId,
        activeSourceFileId,
        "file-retranslation",
        null
      );
      if (revisionBundle.segments.length > 0) {
        refreshJobBundle(revisionBundle);
      }

      const tfBundle = await ensureRosettaTranslationFile(
        activeJobId,
        activeSourceFileId,
        retranslateTargetLang
      );

      const targets = translationTargetsForStatuses({
        sourceSegments: previewSegments,
        translationSegments: tfBundle.segments,
        statuses: "all",
      });

      if (targets.length === 0) return;

      setActiveTranslationFileBundle(tfBundle);

      runId = `run-all-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      const [cancelPromise, cancelResolve] = buildCancelPair();
      cancelRef.current = cancelResolve;

      startTranslationRun({
        id: runId,
        jobId: activeJobId,
        sourceFileId: activeSourceFileId,
        translationFileId: tfBundle.translationFile.id,
        scope: "file",
        targetSegmentIds: targets.map((t) => t.id),
      });

      const result = await runTranslationBatches({
        batchSize: BATCH_SIZE,
        cancelPromise,
        jobId: activeJobId,
        provider: buildProvider(),
        request: {
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          providerPreference: rwkv.providerPreference,
          sourceLang,
          targetLang: retranslateTargetLang,
        },
        targets,
        translationFile: tfBundle.translationFile,
        onBatchCompleted: (ids) => markTranslationRunCompleted(runId!, ids),
        onBatchFailed: (ids) => markTranslationRunFailed(runId!, ids),
        onTranslationFileSaved: (saved) => {
          upsertTranslationFile(saved.translationFile);
          if (
            isViewingTranslationFile(
              activeJobId,
              activeSourceFileId,
              tfBundle.translationFile.id
            )
          ) {
            updateActiveTranslationSegments(saved.segments);
          }
        },
      });

      finishTranslationRun(runId!);
      cancelRef.current = null;

      if (result === "failed") {
        setPageError("翻译失败，请检查 API 配置或网络。");
      }

      const freshBundle = await loadRosettaJob(activeJobId);
      refreshJobBundle(freshBundle);
    } catch (err) {
      const msg = errorMessage(err, "");
      if (!msg.includes("已取消")) {
        if (sourceFile?.format === "pdf") {
          setPdfError(errorMessage(err, "重新翻译失败。"));
        } else {
          setPageError(errorMessage(err, "重新翻译失败。"));
        }
      }
      if (runId) finishTranslationRun(runId);
    }
  }

  function handleCancelTranslation() {
    cancelRef.current?.();
    cancelRef.current = null;
  }

  async function handleExport(kind: "translation" | "bilingual") {
    if (!activeJobId || !activeTranslationFileId || !activeSourceFileId || !activeDocument) return;

    const file = activeDocument.files.find((f) => f.id === activeSourceFileId);
    if (!file) return;

    const exportFmt = exportFormatForSource(file.format);
    const defaultName = defaultExportFilename(
      file.relativePath,
      file.format,
      activeTranslationFile?.targetLang ?? "zh-CN",
      kind
    );

    try {
      const targetPath = await pickRosettaExportPath(defaultName, exportFmt);
      if (!targetPath) return;
      if (file.format === "pdf") {
        // PDF v1 only ships single-language ("translation") export — the
        // translated PDF on disk is exactly what we'd hand the user. There's
        // no bilingual side-by-side renderer yet.
        if (kind === "bilingual") {
          setPageError("PDF 暂不支持双语对照导出。");
          return;
        }
        await exportRosettaTranslatedPdf(activeJobId, targetPath);
      } else {
        await exportRosettaTranslationFile(
          activeJobId,
          activeTranslationFileId,
          kind,
          targetPath
        );
      }
    } catch (err) {
      setPageError(errorMessage(err, "导出失败。"));
    }
  }

  function handleBlockSelect(blockId: string) {
    setSelectedBlockIds((current) =>
      current.includes(blockId)
        ? current.filter((id) => id !== blockId)
        : [...current, blockId]
    );
  }

  function sourceTextForEditing() {
    if (!activeDocument || !sourceFile) return "";
    return activeDocument.blocks
      .filter((block) => (block.fileId ?? "file-1") === sourceFile.id)
      .sort((left, right) => left.order - right.order)
      .map((block) => block.sourceText)
      .join("\n\n");
  }

  function startSourceEdit() {
    setPageError(null);
    setSourceDraft(sourceTextForEditing());
    setIsEditingSource(true);
  }

  async function saveSourceEdit() {
    if (!activeJobId || !sourceFile || isSavingSource) return;
    setIsSavingSource(true);
    setPageError(null);
    try {
      const bundle = await updateTxtSourceFile(activeJobId, sourceFile.id, sourceDraft);
      setActiveBundle(bundle);
      setSelectedBlockIds([]);
      setIsEditingSource(false);
      setSourceDraft("");
    } catch (err) {
      setPageError(errorMessage(err, "保存原文失败。"));
    } finally {
      setIsSavingSource(false);
    }
  }

  const hasActiveDocument = !!activeJobId && !!activeDocument;
  const canEditSource =
    !!sourceFile &&
    sourceFile.format === "txt" &&
    activeDocument?.files.length === 1 &&
    !isTranslating &&
    !isTranslationBusyElsewhere;

  return (
    <div className="flex h-full flex-col">
      {hasActiveDocument && activeJob ? (
        <>
          <WorkspaceTopbar
            job={activeJob}
            activeTranslationFile={activeTranslationFile}
            isTranslating={isTranslating}
            isTranslationBusyElsewhere={isTranslationBusyElsewhere}
            isRuntimeStarting={managedRuntimeStatus?.state === "starting"}
            isPdfEngineInstalling={pdf2zhRuntime.isInstalling}
            isPdfEngineUnavailable={pdfEngineUnavailable}
            pdfEngineUnavailableMessage={pdfEngineUnavailableMessage}
            isPdfEngineWarming={
              !pdfEngineUnavailable &&
              (pdf2zhWorkerStatus?.state === "starting" ||
                pdf2zhWorkerStatus?.state === "idle")
            }
            pdfEngineProgressMessage={pdfEngineProgressMessage}
            translatedCount={completedCount}
            totalCount={totalCount}
            runStartedAtMs={
              activeFileTranslationRun
                ? Number(activeFileTranslationRun.startedAt) || null
                : null
            }
            pdfProgress={pdfProgress}
            sourceLang={sourceLang}
            targetLang={targetLang}
            selectedBlockCount={selectedBlockIds.length}
            pdfSelectedPageCount={pdfSelectedPages.length}
            pdfPageCount={pdfPageCount}
            pdfForceRetranslate={pdfForceRetranslate}
            onPdfForceRetranslateChange={setPdfForceRetranslate}
            onSelectAllPages={() =>
              handlePdfSelectedPagesChange(
                Array.from({ length: pdfPageCount }, (_, i) => i + 1),
              )
            }
            onDeselectAllPages={() => handlePdfSelectedPagesChange([])}
            onSourceLangChange={handleSourceLangChange}
            onTargetLangChange={handleTargetLangChange}
            onTranslate={(lang, src) => void handleTranslate(lang, src)}
            onCancelTranslation={handleCancelTranslation}
            onExport={(kind) => void handleExport(kind)}
            onRetranslateSelected={() => void handleRetranslateSelected()}
            onClearSelection={() => setSelectedBlockIds([])}
            onRetranslateAll={() => void handleRetranslateAll()}
          />
          {pageError && (
            <div className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-xs text-destructive">
              {pageError}
            </div>
          )}
          <div className="min-h-0 flex-1 overflow-hidden">
            <DocumentPreview
              jobId={activeJobId}
              document={activeDocument}
              hoveredBlockId={hoveredBlockId}
              isTranslating={isTranslating}
              liveProgress={
                isTranslating ? { completed: completedCount, total: totalCount } : undefined
              }
              onBlockHover={setHoveredBlockId}
              onBlockLeave={() => setHoveredBlockId(null)}
              selectionEnabled={!isTranslating && !isEditingSource}
              selectedBlockIds={selectedBlockIds}
              onToggleBlockSelection={handleBlockSelect}
              sourceFile={sourceFile}
              sourceSegments={previewSegments}
              sourceEditing={isEditingSource}
              sourceEditText={sourceDraft}
              sourceEditSaving={isSavingSource}
              sourceEditEnabled={canEditSource}
              onSourceEditCancel={() => {
                setIsEditingSource(false);
                setSourceDraft("");
              }}
              onSourceEditChange={setSourceDraft}
              onSourceEditSave={() => void saveSourceEdit()}
              onSourceEditStart={startSourceEdit}
              translationFile={activeTranslationFile}
              translationSegments={translationSegments}
              pdfProgress={pdfProgress}
              pdfError={pdfError}
              pdfSelectedPages={pdfSelectedPages}
              onPdfPageCountChange={handlePdfPageCountChange}
              onPdfSelectedPagesChange={handlePdfSelectedPagesChange}
            />
          </div>
        </>
      ) : (
        <WorkspaceEmpty
          onImported={handleImported}
          isDraggingOver={isDraggingOver}
        />
      )}
    </div>
  );
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return fallback;
}

function formatPageSelection(pages: number[]) {
  const sorted = [...new Set(pages)].sort((a, b) => a - b);
  if (sorted.length === 0) return "";
  const ranges: string[] = [];
  let start = sorted[0];
  let previous = sorted[0];
  for (const page of sorted.slice(1)) {
    if (page === previous + 1) {
      previous = page;
      continue;
    }
    ranges.push(start === previous ? `${start}` : `${start}-${previous}`);
    start = page;
    previous = page;
  }
  ranges.push(start === previous ? `${start}` : `${start}-${previous}`);
  return ranges.join(",");
}

function pdfInstallProgressMessage(progress: Pdf2zhInstallProgress | null) {
  if (!progress) return null;
  if (progress.phase === "downloading") {
    const percent =
      progress.bytesTotal > 0
        ? Math.round((progress.bytesDone / progress.bytesTotal) * 100)
        : null;
    return percent == null ? "正在下载 PDF 引擎…" : `正在下载 PDF 引擎 ${percent}%`;
  }
  if (progress.phase === "verifying") return "正在校验 PDF 引擎…";
  if (progress.phase === "extracting") return "正在解压 PDF 引擎…";
  if (progress.phase === "preflight") return "正在准备 PDF 引擎…";
  return progress.message || null;
}
