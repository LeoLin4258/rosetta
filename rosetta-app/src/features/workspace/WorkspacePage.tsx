import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  createRosettaTranslationRevision,
  ensureRosettaTranslationFile,
  exportRosettaTranslationFile,
  importRosettaDocumentFromPath,
  importRosettaProjectFromDirectory,
  loadRosettaJob,
  pickRosettaExportPath,
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

import { WorkspaceEmpty } from "./WorkspaceEmpty";
import { WorkspaceTopbar } from "./WorkspaceTopbar";

const BATCH_SIZE = 16;

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

  const setActiveBundle = useRosettaStore((s) => s.setActiveBundle);
  const refreshJobBundle = useRosettaStore((s) => s.refreshJobBundle);
  const setActiveTranslationFileBundle = useRosettaStore((s) => s.setActiveTranslationFileBundle);
  const updateActiveTranslationSegments = useRosettaStore((s) => s.updateActiveTranslationSegments);
  const startTranslationRun = useRosettaStore((s) => s.startTranslationRun);
  const markTranslationRunCompleted = useRosettaStore((s) => s.markTranslationRunCompleted);
  const markTranslationRunFailed = useRosettaStore((s) => s.markTranslationRunFailed);
  const finishTranslationRun = useRosettaStore((s) => s.finishTranslationRun);

  const [isDraggingOver, setIsDraggingOver] = useState(false);
  const [pageError, setPageError] = useState<string | null>(null);
  const [selectedBlockIds, setSelectedBlockIds] = useState<string[]>([]);

  // Source language: "auto" = let the model infer; otherwise explicit code
  const [sourceLang, setSourceLang] = useState<string>(
    activeDocument?.sourceLang ?? "auto"
  );

  const cancelRef = useRef<(() => void) | null>(null);

  const activeJob = jobs.find((j) => j.id === activeJobId) ?? null;
  const activeTranslationFile =
    translationFiles.find((f) => f.id === activeTranslationFileId) ?? null;
  const sourceFile =
    activeDocument?.files.find((f) => f.id === activeSourceFileId) ??
    activeDocument?.files[0] ??
    null;
  const isTranslating = !!activeTranslationRun;

  const completedCount = activeTranslationRun?.completedSegmentIds.length ?? 0;
  const totalCount = activeTranslationRun?.targetSegmentIds.length ?? 0;

  // Reset selected blocks and source lang when active document changes
  useEffect(() => {
    setSelectedBlockIds([]);
    setSourceLang(activeDocument?.sourceLang ?? "auto");
  }, [activeDocument?.id, activeDocument?.sourceLang]);

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
        setPageError(err instanceof Error ? err.message : "导入失败");
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

  async function handleTranslate(targetLang: string, srcLang: string) {
    if (!activeJobId || !activeSourceFileId) return;
    setPageError(null);
    setSelectedBlockIds([]);

    try {
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

      const runId = `run-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
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
          sourceLang: srcLang && srcLang !== "auto" ? srcLang : undefined,
          targetLang,
        },
        targets,
        translationFile: tfBundle.translationFile,
        onBatchCompleted: (ids) => markTranslationRunCompleted(runId, ids),
        onBatchFailed: (ids) => markTranslationRunFailed(runId, ids),
        onTranslationFileSaved: (saved) =>
          updateActiveTranslationSegments(saved.segments),
      });

      finishTranslationRun(runId);
      cancelRef.current = null;

      if (result === "failed") {
        setPageError("翻译失败，请检查 API 配置或网络。");
      }

      // Use refreshJobBundle (not setActiveBundle) to preserve translation segments
      const freshBundle = await loadRosettaJob(activeJobId);
      refreshJobBundle(freshBundle);
    } catch (err) {
      setPageError(err instanceof Error ? err.message : "翻译出错。");
      if (activeTranslationRun) finishTranslationRun(activeTranslationRun.id);
    }
  }

  async function handleRetranslateSelected() {
    if (!activeJobId || !activeSourceFileId || selectedBlockIds.length === 0) return;
    const targetLang = activeTranslationFile?.targetLang ?? defaultTargetLang;
    setPageError(null);

    try {
      // Reset the selected blocks' segments to pending via a revision
      const revisionBundle = await createRosettaTranslationRevision(
        activeJobId,
        activeSourceFileId,
        "selection-retranslation",
        selectedBlockIds
      );
      refreshJobBundle(revisionBundle);

      const tfBundle = await ensureRosettaTranslationFile(
        activeJobId,
        activeSourceFileId,
        targetLang
      );
      setActiveTranslationFileBundle(tfBundle);

      const blockSegments = revisionBundle.segments.filter(
        (s) => selectedBlockIds.includes(s.blockId) && s.sourceText.trim()
      );
      const targets = translationTargetsForStatuses({
        sourceSegments: blockSegments,
        translationSegments: tfBundle.segments,
        statuses: "all",
      });

      if (targets.length === 0) return;

      const runId = `run-sel-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
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
          sourceLang: sourceLang || undefined,
          targetLang,
        },
        targets,
        translationFile: tfBundle.translationFile,
        onBatchCompleted: (ids) => markTranslationRunCompleted(runId, ids),
        onBatchFailed: (ids) => markTranslationRunFailed(runId, ids),
        onTranslationFileSaved: (saved) =>
          updateActiveTranslationSegments(saved.segments),
      });

      finishTranslationRun(runId);
      cancelRef.current = null;
      setSelectedBlockIds([]);

      const freshBundle = await loadRosettaJob(activeJobId);
      refreshJobBundle(freshBundle);
    } catch (err) {
      setPageError(err instanceof Error ? err.message : "重新翻译失败。");
      if (activeTranslationRun) finishTranslationRun(activeTranslationRun.id);
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
      await exportRosettaTranslationFile(
        activeJobId,
        activeTranslationFileId,
        kind,
        targetPath
      );
    } catch (err) {
      setPageError(err instanceof Error ? err.message : "导出失败。");
    }
  }

  function handleBlockSelect(blockId: string) {
    setSelectedBlockIds((current) =>
      current.includes(blockId)
        ? current.filter((id) => id !== blockId)
        : [...current, blockId]
    );
  }

  const hasActiveDocument = !!activeJobId && !!activeDocument;

  return (
    <div className="flex h-full flex-col">
      {hasActiveDocument && activeJob ? (
        <>
          <WorkspaceTopbar
            job={activeJob}
            activeTranslationFile={activeTranslationFile}
            isTranslating={isTranslating}
            translatedCount={completedCount}
            totalCount={totalCount}
            sourceLang={sourceLang}
            selectedBlockCount={selectedBlockIds.length}
            onSourceLangChange={setSourceLang}
            onTranslate={(lang, src) => void handleTranslate(lang, src)}
            onCancelTranslation={handleCancelTranslation}
            onExport={(kind) => void handleExport(kind)}
            onRetranslateSelected={() => void handleRetranslateSelected()}
          />
          {pageError && (
            <div className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-xs text-destructive">
              {pageError}
            </div>
          )}
          <div className="min-h-0 flex-1 overflow-hidden">
            <DocumentPreview
              document={activeDocument}
              selectionEnabled={!isTranslating}
              selectedBlockIds={selectedBlockIds}
              onToggleBlockSelection={handleBlockSelect}
              sourceFile={sourceFile}
              sourceSegments={previewSegments}
              translationFile={activeTranslationFile}
              translationSegments={translationSegments}
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
