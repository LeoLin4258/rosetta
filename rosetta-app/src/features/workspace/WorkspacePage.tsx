import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useNavigate } from "react-router-dom";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
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
  updateRosettaJobFileLanguages,
  updateTxtSourceFile,
} from "@/lib/rosettaJobs";
import { selectProvider } from "@/lib/providers";
import {
  isManagedRuntimeProfileReady,
  selectManagedRuntimeProfileStatus,
} from "@/lib/managedRuntimeSelection";
import {
  runTranslationBatches,
  translationTargetsForStatuses,
} from "@/lib/translationRunner";
import { textBatchSizeForProvider } from "@/lib/translationBatchPolicy";
import { defaultExportFilename, exportFormatForSource } from "@/lib/rosettaExport";
import { useRosettaStore } from "@/store/useRosettaStore";
import type { RosettaJobBundle } from "@/types/rosetta";
import { DocumentPreview } from "@/features/preview/DocumentPreview";
import {
  defaultPdfSelectedPages,
  normalizePdfPageNumbers,
  shouldConfirmLongPdfTranslation,
} from "@/lib/pdfPageSelectionPolicy";

import { WorkspaceEmpty } from "./WorkspaceEmpty";
import { WorkspaceTopbar } from "./WorkspaceTopbar";
import { usePdfV3RunControl } from "./usePdfV3RunControl";

const DEFAULT_SOURCE_LANG = "en";

type PendingLongPdfTranslation = {
  pages: number[];
  pageCount: number;
  targetLang: string;
  sourceLang: string;
};

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
  const defaultTargetLang = useRosettaStore((s) => s.defaultTargetLang);
  const langByJobId = useRosettaStore((s) => s.langByJobId);
  const setJobLangs = useRosettaStore((s) => s.setJobLangs);
  const navigate = useNavigate();

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
  const [pendingLongPdfTranslation, setPendingLongPdfTranslation] =
    useState<PendingLongPdfTranslation | null>(null);
  const [hoveredBlockId, setHoveredBlockId] = useState<string | null>(null);
  const [isEditingSource, setIsEditingSource] = useState(false);
  const [sourceDraft, setSourceDraft] = useState("");
  const [isSavingSource, setIsSavingSource] = useState(false);
  const cancelRef = useRef<(() => void) | null>(null);

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
  const isPdfJob = sourceFile?.format === "pdf";
  const pdfV3Control = usePdfV3RunControl({
    jobId: activeJobId,
    targetLanguage: targetLang,
    enabled: isPdfJob,
  });
  const activeFileTranslationRun =
    activeTranslationRun &&
    activeTranslationRun.jobId === activeJobId &&
    activeTranslationRun.sourceFileId === activeSourceFileId
      ? activeTranslationRun
      : null;
  const pdfV3RunIsActive =
    pdfV3Control.operation === "creating" ||
    pdfV3Control.status?.state === "running" ||
    pdfV3Control.status?.state === "cancelling";
  const isTranslating = isPdfJob
    ? pdfV3RunIsActive
    : !!activeFileTranslationRun;
  const isTranslationBusyElsewhere =
    !!activeTranslationRun && (isPdfJob || !activeFileTranslationRun);

  const completedCount = isPdfJob
    ? pdfV3Control.completedPages
    : activeFileTranslationRun?.completedSegmentIds.length ?? 0;
  const totalCount = isPdfJob
    ? pdfV3Control.status?.summary.requestedPages ?? 0
    : activeFileTranslationRun?.targetSegmentIds.length ?? 0;
  const pdfDisplayedSelectedPageCount =
    pdfV3Control.status && !pdfV3Control.runIsTerminal
      ? pdfV3Control.status.summary.requestedPages
      : pdfSelectedPages.length;

  // Reset block selection when switching documents
  useEffect(() => {
    setSelectedBlockIds([]);
    setPdfPageCount(0);
    setPdfSelectedPages([]);
    setPendingLongPdfTranslation(null);
    setIsEditingSource(false);
    setSourceDraft("");
  }, [activeDocument?.id]);

  useEffect(() => {
    setIsEditingSource(false);
    setSourceDraft("");
    setPendingLongPdfTranslation(null);
  }, [activeSourceFileId]);

  const handlePdfPageCountChange = useCallback((count: number) => {
    setPdfPageCount(count);
  }, []);

  const handlePdfSelectedPagesChange = useCallback((pages: number[]) => {
    setPdfSelectedPages(pages);
  }, []);

  const selectedRuntimeStatus = selectManagedRuntimeProfileStatus(
    managedRuntimeStatus,
    rwkv.managedRuntimeProfileId
  );
  const managedRuntimeReady = isManagedRuntimeProfileReady(selectedRuntimeStatus);
  const selectedProvider = selectProvider({
    config: rwkv,
    managedRuntimeReady,
    managedRuntimeProviderId: selectedRuntimeStatus?.profile.providerId,
    managedRuntimeBaseUrl: selectedRuntimeStatus?.process.baseUrl ?? undefined,
    managedRuntimeEndpoint:
      selectedRuntimeStatus?.profile.batchChatPath ?? undefined,
  });
  const localRuntimeRequired = rwkv.providerPreference === "local";
  const localRuntimeUnavailable = localRuntimeRequired && !managedRuntimeReady;
  const localRuntimeStarting =
    localRuntimeUnavailable &&
    selectedRuntimeStatus?.state !== "failed" &&
    selectedRuntimeStatus?.state !== "unsupported";
  const localRuntimeUnavailableMessage =
    selectedRuntimeStatus?.state === "failed"
      ? summarizeRuntimeUnavailableMessage(
          selectedRuntimeStatus.process.lastError ?? selectedRuntimeStatus.message
        )
      : selectedRuntimeStatus?.state === "unsupported"
        ? selectedRuntimeStatus.message
        : "本地翻译模型正在启动，请稍候。";

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
    return selectedProvider;
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

  async function requestPdfPageTranslation(
    pages: number[],
    targetLangOverride: string,
    sourceLangOverride: string,
    pageCountOverride = pdfPageCount,
  ) {
    const normalizedPages = normalizePdfPageNumbers(pages, pageCountOverride);
    if (normalizedPages.length === 0) {
      setPdfError("请选择要翻译的页面。");
      return;
    }

    if (shouldConfirmLongPdfTranslation(normalizedPages.length)) {
      setPendingLongPdfTranslation({
        pages: normalizedPages,
        pageCount:
          pageCountOverride > 0
            ? pageCountOverride
            : Math.max(...normalizedPages),
        targetLang: targetLangOverride,
        sourceLang: sourceLangOverride,
      });
      return;
    }

    await handleTranslatePdfPages(
      formatPageSelection(normalizedPages),
      targetLangOverride,
      sourceLangOverride,
    );
  }

  function runPendingLongPdfTranslation(pages: number[]) {
    const pending = pendingLongPdfTranslation;
    if (!pending) return;
    const normalizedPages = normalizePdfPageNumbers(pages, pending.pageCount);
    if (normalizedPages.length === 0) {
      setPendingLongPdfTranslation(null);
      setPdfError("请选择要翻译的页面。");
      return;
    }
    setPendingLongPdfTranslation(null);
    void handleTranslatePdfPages(
      formatPageSelection(normalizedPages),
      pending.targetLang,
      pending.sourceLang,
    );
  }

  function runPendingLongPdfPreviewPages() {
    const pending = pendingLongPdfTranslation;
    if (!pending) return;
    const previewPages = defaultPdfSelectedPages(pending.pageCount);
    setPdfSelectedPages(previewPages);
    runPendingLongPdfTranslation(previewPages);
  }

  async function handleTranslate(targetLang: string, srcLang: string) {
    if (!activeJobId || !activeSourceFileId) return;
    setPageError(null);
    setPdfError(null);
    setSelectedBlockIds([]);

    // Declared outside try so the catch block can always call finishTranslationRun.
    let runId: string | null = null;

    try {
      if (sourceFile?.format === "pdf") {
        const selectedPages =
          pdfSelectedPages.length > 0
            ? pdfSelectedPages
            : pdfPageCount > 0
              ? Array.from({ length: pdfPageCount }, (_, index) => index + 1)
              : [];
        if (selectedPages.length === 0) {
          setPdfError("请选择要翻译的页面。");
          return;
        }
        await requestPdfPageTranslation(
          selectedPages,
          targetLang,
          srcLang,
          pdfPageCount,
        );
        return;
      }

      const tfBundle = await ensureRosettaTranslationFile(
        activeJobId,
        activeSourceFileId,
        targetLang
      );
      setActiveTranslationFileBundle(tfBundle);

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

      const provider = buildProvider();
      const result = await runTranslationBatches({
        batchSize: textBatchSizeForProvider(provider),
        cancelPromise,
        jobId: activeJobId,
        provider,
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
    targetLangOverride = targetLang,
    sourceLangOverride = sourceLang,
  ) {
    if (!activeJobId || !activeSourceFileId) return null;
    const pageTargetLang = targetLangOverride;
    setPageError(null);
    setPdfError(null);
    setSelectedBlockIds([]);

    try {
      const languageBundle = await updateRosettaJobFileLanguages(
        activeJobId,
        activeSourceFileId,
        normalizeSourceLang(sourceLangOverride),
        pageTargetLang,
      );
      refreshJobBundle(languageBundle);
      const tfBundle = await ensureRosettaTranslationFile(
        activeJobId,
        activeSourceFileId,
        pageTargetLang,
      );
      setActiveTranslationFileBundle(tfBundle);
      return await pdfV3Control.create(pageSelection);
    } catch (err) {
      console.error("[pdf-v3] failed to create run", err);
      const msg = errorMessage(err, "");
      if (!msg.includes("已取消")) {
        setPdfError(errorMessage(err, "无法启动 PDF v3 翻译。"));
      }
      return null;
    }
  }

  async function handleRetranslateSelected() {
    if (!activeJobId || !activeSourceFileId) return;
    const retranslateTargetLang = activeTranslationFile?.targetLang ?? targetLang;
    setPageError(null);

    if (sourceFile?.format === "pdf") {
      if (pdfSelectedPages.length === 0) {
        setPdfError("请选择要重新翻译的页面。");
        return;
      }
      await requestPdfPageTranslation(
        pdfSelectedPages,
        retranslateTargetLang,
        sourceLang,
        pdfPageCount,
      );
      return;
    }

    if (selectedBlockIds.length === 0) return;

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

      const provider = buildProvider();
      await runTranslationBatches({
        batchSize: textBatchSizeForProvider(provider),
        cancelPromise,
        jobId: activeJobId,
        provider,
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
        const pageCount = await countRosettaPdfPages(activeJobId, "source");
        if (pageCount <= 0) {
          setPdfError("无法读取 PDF 页数，请重新导入后再试。");
          return;
        }
        await requestPdfPageTranslation(
          Array.from({ length: pageCount }, (_, index) => index + 1),
          retranslateTargetLang,
          sourceLang,
          pageCount,
        );
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

      const provider = buildProvider();
      const result = await runTranslationBatches({
        batchSize: textBatchSizeForProvider(provider),
        cancelPromise,
        jobId: activeJobId,
        provider,
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

  async function handlePdfV3Control(
    action: () => Promise<unknown>,
    fallback: string,
  ) {
    setPdfError(null);
    try {
      await action();
    } catch (err) {
      setPdfError(errorMessage(err, fallback));
    }
  }

  async function handleExport(kind: "translation" | "bilingual") {
    if (!activeJobId || !activeTranslationFileId || !activeSourceFileId || !activeDocument) return;

    const file = activeDocument.files.find((f) => f.id === activeSourceFileId);
    if (!file) return;
    if (file.format === "pdf" && pdfV3Control.status) {
      setPageError("PDF v3 导出尚未接入，当前不会回退到旧版 PDF 产物。");
      return;
    }

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
        await exportRosettaTranslatedPdf(
          activeJobId,
          targetPath,
          activeTranslationFile?.targetLang ?? targetLang
        );
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
  const pendingLongPdfPreviewPageCount = pendingLongPdfTranslation
    ? defaultPdfSelectedPages(pendingLongPdfTranslation.pageCount).length
    : 0;

  return (
    <div className="flex h-full flex-col">
      {hasActiveDocument && activeJob ? (
        <>
          <WorkspaceTopbar
            job={activeJob}
            activeTranslationFile={activeTranslationFile}
            isTranslating={isTranslating}
            isTranslationBusyElsewhere={isTranslationBusyElsewhere}
            isRuntimeStarting={localRuntimeStarting}
            isRuntimeUnavailable={localRuntimeUnavailable}
            runtimeUnavailableMessage={localRuntimeUnavailableMessage}
            translatedCount={completedCount}
            totalCount={totalCount}
            runStartedAtMs={
              activeFileTranslationRun
                ? Number(activeFileTranslationRun.startedAt) || null
                : null
            }
            pdfV3RunStatus={pdfV3Control.status}
            pdfV3ControlOperation={pdfV3Control.operation}
            pdfV3CanRecover={pdfV3Control.canRecover}
            pdfV3IsDiscovering={pdfV3Control.isDiscovering}
            pdfV3DiscoveryError={pdfV3Control.discoveryError}
            sourceLang={sourceLang}
            targetLang={targetLang}
            selectedBlockCount={selectedBlockIds.length}
            pdfSelectedPageCount={pdfDisplayedSelectedPageCount}
            pdfPageCount={pdfPageCount}
            onSelectAllPages={() =>
              handlePdfSelectedPagesChange(
                Array.from({ length: pdfPageCount }, (_, i) => i + 1),
              )
            }
            onSelectPreviewPages={() =>
              handlePdfSelectedPagesChange(defaultPdfSelectedPages(pdfPageCount))
            }
            onDeselectAllPages={() => handlePdfSelectedPagesChange([])}
            onSourceLangChange={handleSourceLangChange}
            onTargetLangChange={handleTargetLangChange}
            onTranslate={(lang, src) => void handleTranslate(lang, src)}
            onCancelTranslation={handleCancelTranslation}
            onPausePdfV3Run={() =>
              void handlePdfV3Control(pdfV3Control.pause, "暂停 PDF 翻译失败。")
            }
            onResumePdfV3Run={() =>
              void handlePdfV3Control(pdfV3Control.resume, "恢复 PDF 翻译失败。")
            }
            onCancelPdfV3Run={() =>
              void handlePdfV3Control(pdfV3Control.cancel, "停止 PDF 翻译失败。")
            }
            onRecoverPdfV3Run={() =>
              void handlePdfV3Control(pdfV3Control.recover, "接管 PDF 翻译失败。")
            }
            onExport={(kind) => void handleExport(kind)}
            onRetranslateSelected={() => void handleRetranslateSelected()}
            onClearSelection={() => setSelectedBlockIds([])}
            onRetranslateAll={() => void handleRetranslateAll()}
            onOpenRuntimeSettings={() => navigate("/settings?panel=local-runtime")}
          />
          <AlertDialog
            open={pendingLongPdfTranslation != null}
            onOpenChange={(open) => {
              if (!open) setPendingLongPdfTranslation(null);
            }}
          >
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  翻译 {pendingLongPdfTranslation?.pages.length ?? 0} 页？
                </AlertDialogTitle>
                <AlertDialogDescription className="leading-6">
                  这个 PDF 共 {pendingLongPdfTranslation?.pageCount ?? 0} 页。Rosetta 会按页在后台处理，
                  期间可以暂停、恢复或停止。建议先翻译前 {pendingLongPdfPreviewPageCount} 页确认版面效果。
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>取消</AlertDialogCancel>
                <AlertDialogAction
                  variant="outline"
                  onClick={runPendingLongPdfPreviewPages}
                >
                  先翻译前 {pendingLongPdfPreviewPageCount} 页
                </AlertDialogAction>
                <AlertDialogAction
                  onClick={() =>
                    runPendingLongPdfTranslation(
                      pendingLongPdfTranslation?.pages ?? [],
                    )
                  }
                >
                  继续翻译 {pendingLongPdfTranslation?.pages.length ?? 0} 页
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
          {(pageError || (isPdfJob ? pdfError ?? pdfV3Control.error : null)) && (
            <div className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-xs text-destructive">
              {pageError ?? pdfError ?? pdfV3Control.error}
            </div>
          )}
          <div className="min-h-0 flex-1 overflow-hidden">
            <DocumentPreview
              jobId={activeJobId}
              document={activeDocument}
              hoveredBlockId={hoveredBlockId}
              isTranslating={isTranslating}
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
              pdfError={pdfError ?? pdfV3Control.error}
              pdfV3RunStatus={pdfV3Control.status}
              pdfV3IsDiscovering={pdfV3Control.isDiscovering}
              pdfV3DiscoveryError={pdfV3Control.discoveryError}
              pdfV3ControlOperation={pdfV3Control.operation}
              pdfSelectedPages={pdfSelectedPages}
              onPdfPageCountChange={handlePdfPageCountChange}
              onPdfSelectedPagesChange={handlePdfSelectedPagesChange}
              onRetryPdfV3Page={(pageNumber) =>
                void handlePdfV3Control(
                  () => pdfV3Control.retryPage(pageNumber),
                  `重试第 ${pageNumber} 页失败。`,
                )
              }
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

function summarizeRuntimeUnavailableMessage(message: string | null | undefined) {
  if (!message) {
    return "本地模型启动失败，请到设置页修复本地运行时。";
  }
  if (
    message.includes("Windows 无法连接") ||
    message.includes("loopback") ||
    message.includes("127.0.0.1")
  ) {
    return "Windows 拦住了本机连接，请到设置页点击“修复连接并重试”。";
  }
  if (message.includes("在 45 秒内未就绪") || message.includes("timed out")) {
    return "本地模型启动后没有响应，请到设置页检查本地运行时。";
  }
  return "本地模型启动失败，请到设置页检查本地运行时。";
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
