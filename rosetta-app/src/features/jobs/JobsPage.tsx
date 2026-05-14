import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  AlertCircle,
  ArrowRight,
  CheckCircle2,
  Clock3,
  Download,
  FilePlus,
  FileText,
  Folder,
  Languages,
  LoaderCircle,
  MoreVertical,
  Play,
  RefreshCw,
  Square,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  ensureRosettaTranslationFile,
  exportRosettaTranslationFile,
  loadRosettaJob,
  loadRosettaTranslationFile,
  pickRosettaExportPath,
  saveRosettaTranslationSegments,
} from "../../lib/rosettaJobs";
import { rosettaJobDefaultPath, rosettaJobFilePath } from "../../lib/rosettaRoutes";
import { resolveJobsPageSelection } from "../../lib/rosettaSelection";
import { defaultExportFilename, exportFormatForSource } from "../../lib/rosettaExport";
import {
  openSourcePreviewWindow,
  openTranslationPreviewWindow,
} from "../../lib/translationPreviewWindow";
import {
  LANGUAGE_OPTIONS,
  SOURCE_LANGUAGE_OPTIONS,
  isRwkvConfigReady,
} from "../../lib/languages";
import {
  translationProgressPercent,
} from "../../lib/translationSegments";
import {
  runTranslationBatches,
  translationTargetsForStatuses,
  type TranslationRunResult,
} from "../../lib/translationRunner";
import { selectProvider } from "../../lib/providers";
import { isManagedRuntimeReady } from "../../lib/useManagedRwkvRuntime";
import { cn } from "../../lib/utils";
import { useRosettaStore } from "../../store/useRosettaStore";
import type {
  ActiveTranslationRun,
  RosettaExportKind,
  RosettaSourceFile,
  RosettaTranslationFile,
  Segment,
  TranslationSegment,
} from "../../types/rosetta";

// Upper-bound hint for the batch scheduler. External-API (`rwkv-lightning-contents`)
// uses this verbatim — it has no GPU-slot concept and 16 has been the stable
// throughput tuning. The local rwkv-mobile sidecar treats it as a *hint*:
// Phase 6 has Rust query `/v1/batch/supported_batch_sizes` per run and clamp
// to the model's reported maximum, so this value can safely overshoot.
const BATCH_SIZE = 16;

export function JobsPage() {
  const { fileId, jobId } = useParams();
  const navigate = useNavigate();
  const jobs = useRosettaStore((state) => state.jobs);
  const activeJobId = useRosettaStore((state) => state.activeJobId);
  const activeSourceFileId = useRosettaStore((state) => state.activeSourceFileId);
  const activeTranslationFileId = useRosettaStore(
    (state) => state.activeTranslationFileId
  );
  const activeSourceFileIdByJobId = useRosettaStore(
    (state) => state.activeSourceFileIdByJobId
  );
  const activeTranslationFileIdBySourceKey = useRosettaStore(
    (state) => state.activeTranslationFileIdBySourceKey
  );
  const activeDocument = useRosettaStore((state) => state.activeDocument);
  const previewSegments = useRosettaStore((state) => state.previewSegments);
  const translationFiles = useRosettaStore((state) => state.translationFiles);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const activeTranslationRun = useRosettaStore(
    (state) => state.activeTranslationRun
  );
  const setActiveBundle = useRosettaStore((state) => state.setActiveBundle);
  const refreshJobBundle = useRosettaStore((state) => state.refreshJobBundle);
  const setActiveJobSelection = useRosettaStore(
    (state) => state.setActiveJobSelection
  );
  const upsertTranslationFile = useRosettaStore(
    (state) => state.upsertTranslationFile
  );
  const startTranslationRun = useRosettaStore((state) => state.startTranslationRun);
  const markTranslationRunCompleted = useRosettaStore(
    (state) => state.markTranslationRunCompleted
  );
  const markTranslationRunFailed = useRosettaStore(
    (state) => state.markTranslationRunFailed
  );
  const finishTranslationRun = useRosettaStore(
    (state) => state.finishTranslationRun
  );

  const [isTranslating, setIsTranslating] = useState(false);
  const [batchSourceLang, setBatchSourceLang] = useState("en");
  const [batchTargetLangs, setBatchTargetLangs] = useState<string[]>(["zh-CN"]);
  const [selectedSourceFileIds, setSelectedSourceFileIds] = useState<string[]>(
    []
  );
  const [queuedTranslationFileIds, setQueuedTranslationFileIds] = useState<
    string[]
  >([]);
  const [pageError, setPageError] = useState<string | null>(null);
  const loadRequestIdRef = useRef(0);
  const activeTranslationCancelRef = useRef<(() => void) | null>(null);

  const {
    currentJobId,
    document,
    selectedSourceFile,
    selectedSourceFileId,
    selectedTranslationFile,
    sourceFiles,
  } = resolveJobsPageSelection({
    activeDocument,
    activeJobId,
    activeSourceFileId,
    activeSourceFileIdByJobId,
    activeTranslationFileId,
    activeTranslationFileIdBySourceKey,
    jobs,
    routeJobId: jobId,
    routeSourceFileId: fileId,
    translationFiles,
  });
  const translationFilesBySourceId = useMemo(
    () => groupTranslationFilesBySource(translationFiles),
    [translationFiles]
  );
  const sourceSegmentsByFileId = useMemo(
    () => groupSourceSegmentsByFile(previewSegments),
    [previewSegments]
  );
  const sourceSegmentCountByFileId = useMemo(
    () => countSourceSegmentsByFile(previewSegments),
    [previewSegments]
  );
  const managedRuntimeStatus = useRosettaStore(
    (state) => state.managedRuntime.status
  );
  const managedRuntimeReady = isManagedRuntimeReady(managedRuntimeStatus);
  // Translation can proceed when *either* a configured external API is ready
  // or the local managed sidecar reports `state: ready`. `selectProvider`
  // (see below) decides which one each batch actually flows through.
  const rwkvConfigReady = managedRuntimeReady || isRwkvConfigReady(rwkv);
  const selectedBatchCount = selectedSourceFileIds.length;

  useEffect(() => {
    if (!jobId && jobs.length > 0) {
      navigate(rosettaJobDefaultPath(jobs[0]), { replace: true });
    }
  }, [jobId, jobs, navigate]);

  useEffect(() => {
    if (!currentJobId) {
      return;
    }

    const currentState = useRosettaStore.getState();
    if (
      currentState.activeJobId === currentJobId &&
      currentState.activeDocument
    ) {
      return;
    }

    const requestId = loadRequestIdRef.current + 1;
    loadRequestIdRef.current = requestId;
    void loadRosettaJob(currentJobId)
      .then(recoverStaleTranslationRuns)
      .then((bundle) => {
        if (
          loadRequestIdRef.current !== requestId ||
          bundle.job.id !== currentJobId
        ) {
          return;
        }
        setPageError(null);
        setActiveBundle(bundle);
      })
      .catch((error) => {
        setPageError(
          error instanceof Error ? error.message : "项目加载失败。"
        );
      });
  }, [currentJobId, setActiveBundle]);

  async function recoverStaleTranslationRuns(
    bundle: Awaited<ReturnType<typeof loadRosettaJob>>
  ) {
    const staleFiles = bundle.translationFiles.filter(
      (translationFile) => translationFile.status === "translating"
    );
    if (staleFiles.length === 0) {
      return bundle;
    }

    for (const translationFile of staleFiles) {
      const translationBundle = await loadRosettaTranslationFile(
        bundle.job.id,
        translationFile.id
      );
      const recoveredSegments = translationBundle.segments.map((segment) =>
        segment.status === "translating"
          ? {
              ...segment,
              status: "pending" as const,
              error: undefined,
            }
          : segment
      );
      await saveRosettaTranslationSegments(
        bundle.job.id,
        translationFile.id,
        recoveredSegments
      );
    }

    return loadRosettaJob(bundle.job.id);
  }

  useEffect(() => {
    if (!currentJobId || !selectedSourceFileId) {
      return;
    }
    if (!fileId) {
      navigate(rosettaJobFilePath(currentJobId, selectedSourceFileId), {
        replace: true,
      });
    }
    setActiveJobSelection(
      currentJobId,
      selectedSourceFileId,
      selectedTranslationFile?.id ?? null
    );
  }, [
    currentJobId,
    fileId,
    navigate,
    selectedSourceFileId,
    selectedTranslationFile?.id,
    setActiveJobSelection,
  ]);

  useEffect(() => {
    const sourceLang = selectedSourceFile?.sourceLang ?? document?.sourceLang;
    if (
      sourceLang &&
      SOURCE_LANGUAGE_OPTIONS.some((language) => language.value === sourceLang)
    ) {
      setBatchSourceLang(sourceLang);
    }
  }, [document?.sourceLang, selectedSourceFile?.sourceLang]);

  function selectSourceFile(sourceFile: RosettaSourceFile) {
    if (!currentJobId) {
      return;
    }
    setActiveJobSelection(currentJobId, sourceFile.id, null);
    navigate(rosettaJobFilePath(currentJobId, sourceFile.id));
  }

  function selectTranslationFile(translationFile: RosettaTranslationFile) {
    if (!currentJobId) {
      return;
    }
    setActiveJobSelection(
      currentJobId,
      translationFile.sourceFileId,
      translationFile.id
    );
    navigate(rosettaJobFilePath(currentJobId, translationFile.sourceFileId));
  }

  async function openTranslationFile(translationFile: RosettaTranslationFile) {
    if (!currentJobId) {
      return;
    }
    const sourceFile =
      sourceFiles.find((file) => file.id === translationFile.sourceFileId) ??
      null;
    selectTranslationFile(translationFile);
    await openTranslationPreviewWindow({
      jobId: currentJobId,
      sourceFilename: sourceFile?.relativePath ?? sourceFile?.filename ?? "源文件",
      translationFile,
    });
  }

  async function openSourceFile(sourceFile: RosettaSourceFile) {
    if (!currentJobId) {
      return;
    }
    selectSourceFile(sourceFile);
    await openSourcePreviewWindow({
      jobId: currentJobId,
      sourceFileId: sourceFile.id,
      sourceFilename: sourceFile.relativePath,
    });
  }

  function toggleSourceSelection(sourceFileId: string) {
    setSelectedSourceFileIds((current) =>
      current.includes(sourceFileId)
        ? current.filter((id) => id !== sourceFileId)
        : [...current, sourceFileId]
    );
  }

  async function translateSelectedBatch() {
    if (
      !currentJobId ||
      selectedSourceFileIds.length === 0 ||
      batchTargetLangs.length === 0
    ) {
      return;
    }

    setPageError(null);
    try {
      const visibleSourceFileId = selectedSourceFileIds.includes(
        selectedSourceFileId ?? ""
      )
        ? selectedSourceFileId
        : selectedSourceFileIds[0];
      const visibleSourceFile = sourceFiles.find(
        (file) => file.id === visibleSourceFileId
      );
      if (visibleSourceFile) {
        selectSourceFile(visibleSourceFile);
      }

      const translationQueue: Array<{
        translationFile: RosettaTranslationFile;
        segments: TranslationSegment[];
      }> = [];

      for (const sourceFileId of selectedSourceFileIds) {
        for (const language of batchTargetLangs) {
          const ensured = await ensureRosettaTranslationFile(
            currentJobId,
            sourceFileId,
            language
          );
          upsertTranslationFile(ensured.translationFile);
          translationQueue.push({
            translationFile: ensured.translationFile,
            segments: ensured.segments,
          });
          setQueuedTranslationFileIds((current) =>
            current.includes(ensured.translationFile.id)
              ? current
              : [...current, ensured.translationFile.id]
          );
        }
      }

      for (const queued of translationQueue) {
        const result = await translateTranslationFile(
          queued.translationFile,
          "batch",
          queued.segments,
          batchSourceLang
        );
        setQueuedTranslationFileIds((current) =>
          current.filter((id) => id !== queued.translationFile.id)
        );
        if (result === "stopped") {
          break;
        }
      }

      if (translationQueue.length > 0) {
        setQueuedTranslationFileIds((current) =>
          current.filter(
            (id) =>
              !translationQueue.some(
                (queued) => queued.translationFile.id === id
              )
          )
        );
      }

      if (currentJobId) {
        refreshJobBundle(await loadRosettaJob(currentJobId));
      }
      setSelectedSourceFileIds([]);
    } catch (error) {
      setPageError(
        error instanceof Error ? error.message : "批量翻译启动失败。"
      );
    }
  }

  async function translateTranslationFile(
    translationFile: RosettaTranslationFile,
    scope: "file" | "batch" = "file",
    initialSegments?: TranslationSegment[],
    sourceLangOverride?: string
  ): Promise<TranslationRunResult> {
    if (!currentJobId || !document || !rwkvConfigReady || isTranslating) {
      return "noop";
    }
    const sourceFile =
      document.files.find((file) => file.id === translationFile.sourceFileId) ??
      null;
    if (!sourceFile) {
      return "noop";
    }

    const loadedSegments =
      initialSegments ??
      (await loadRosettaTranslationFile(currentJobId, translationFile.id)).segments;
    const sourceSegmentsForFile = sourceSegmentsByFileId.get(sourceFile.id) ?? [];
    const targets = translationTargetsForStatuses({
      sourceSegments: sourceSegmentsForFile,
      translationSegments: loadedSegments,
      statuses: ["pending", "failed"],
    });

    if (targets.length === 0) {
      return "noop";
    }

    const runId = `run-${Date.now()}`;
    let cancelCurrentRun: (() => void) | null = null;
    const cancelled = new Promise<"stopped">((resolve) => {
      cancelCurrentRun = () => resolve("stopped");
    });
    setIsTranslating(true);
    activeTranslationCancelRef.current = cancelCurrentRun;
    startTranslationRun({
      id: runId,
      jobId: currentJobId,
      sourceFileId: sourceFile.id,
      translationFileId: translationFile.id,
      scope,
      targetSegmentIds: targets.map((segment) => segment.id),
    });

    // Pick the provider per-run so that toggling the managed runtime on/off
    // inside Settings flips subsequent translations without a page reload.
    const provider = selectProvider({
      config: {
        baseUrl: rwkv.baseUrl,
        endpoint: rwkv.endpoint,
        internalToken: rwkv.internalToken,
        bodyPassword: rwkv.bodyPassword,
        timeoutMs: rwkv.timeoutMs,
      },
      managedRuntimeReady,
      managedRuntimeBaseUrl: managedRuntimeStatus?.process.baseUrl ?? undefined,
    });

    try {
      return await runTranslationBatches({
        // `BATCH_SIZE` is a hint; the local sidecar provider clamps it
        // against `/v1/batch/supported_batch_sizes` in Rust (Phase 6), the
        // external API provider uses it verbatim.
        batchSize: BATCH_SIZE,
        cancelPromise: cancelled,
        jobId: currentJobId,
        onBatchCompleted: (segmentIds) =>
          markTranslationRunCompleted(runId, segmentIds),
        onBatchFailed: (segmentIds) =>
          markTranslationRunFailed(runId, segmentIds),
        onTranslationFileSaved: (bundle) =>
          upsertTranslationFile(bundle.translationFile),
        provider,
        request: {
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          sourceLang:
            sourceLangOverride ?? sourceFile.sourceLang ?? document.sourceLang,
          targetLang: translationFile.targetLang,
        },
        targets,
        translationFile,
      });
    } finally {
      finishTranslationRun(runId);
      setIsTranslating(false);
      if (activeTranslationCancelRef.current === cancelCurrentRun) {
        activeTranslationCancelRef.current = null;
      }
    }
  }

  function stopActiveTranslation() {
    activeTranslationCancelRef.current?.();
  }

  async function exportTranslationFile(
    translationFile: RosettaTranslationFile,
    kind: RosettaExportKind
  ) {
    if (!currentJobId) {
      return;
    }
    setPageError(null);
    const sourceFile =
      sourceFiles.find((file) => file.id === translationFile.sourceFileId) ??
      null;
    if (!sourceFile) {
      return;
    }
    const targetPath = await pickRosettaExportPath(
      defaultExportFilename(
        sourceFile.relativePath,
        sourceFile.format,
        translationFile.targetLang,
        kind
      ),
      exportFormatForSource(sourceFile.format)
    );
    if (!targetPath) {
      return;
    }
    try {
      await exportRosettaTranslationFile(
        currentJobId,
        translationFile.id,
        kind,
        targetPath
      );
      refreshJobBundle(await loadRosettaJob(currentJobId));
    } catch (error) {
      setPageError(error instanceof Error ? error.message : "导出失败。");
    }
  }

  if (jobs.length === 0) {
    return (
      <section className="mx-auto flex max-w-3xl flex-col gap-4 px-6 py-10">
        <Card>
          <CardContent className="flex flex-col items-center gap-4 py-8 text-center">
            <div>
              <h2 className="text-lg font-semibold">还没有项目</h2>
              <p className="mt-2 text-sm text-muted-foreground">
                导入 TXT 或 Markdown 文件开始翻译。
              </p>
            </div>
            <Button asChild type="button">
              <Link to="/new">
                <FilePlus data-icon="inline-start" />
                新项目
              </Link>
            </Button>
          </CardContent>
        </Card>
      </section>
    );
  }

  return (
    <section className="grid h-full min-h-0 grid-rows-[auto_auto_1fr] bg-background">
      <header className="border-b bg-background">
        <div className="flex min-h-12 items-center justify-between gap-4 px-5 pt-2 pb-8">
          <div className="flex min-w-0 items-center gap-5 text-sm text-muted-foreground">
            <span>
              <strong className="mr-1 text-foreground">{sourceFiles.length}</strong>
              原文文件
            </span>
            <span>
              <strong className="mr-1 text-foreground">
                {translationFiles.length}
              </strong>
              译文文件
            </span>
          </div>
          <div className="flex min-w-0 items-center gap-3 rounded-lg border bg-white dark:bg-stone-900 px-2 py-1 shadow-xs">
            <span className="whitespace-nowrap text-sm text-muted-foreground">
              批量：{selectedBatchCount} 个原文已选择
            </span>
            <div className="h-6 w-px bg-border" />
            <div className="flex items-center gap-2">
              {/* <span className="text-sm text-muted-foreground">原文</span> */}
              <Select
                disabled={isTranslating}
                onValueChange={setBatchSourceLang}
                value={batchSourceLang}
              >
                <SelectTrigger aria-label="选择原文语言" size="sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {SOURCE_LANGUAGE_OPTIONS.map((language) => (
                      <SelectItem key={language.value} value={language.value}>
                        {language.label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <div className="flex items-center gap-2">
              {/* <span className="text-sm text-muted-foreground">译文</span> */}
              <ArrowRight className="size-4" />
              <ToggleGroup
                disabled={isTranslating}
                onValueChange={(values) => setBatchTargetLangs(values)}
                type="multiple"
                value={batchTargetLangs}
              >
                {LANGUAGE_OPTIONS.slice(0, 6).map((language) => (
                  <ToggleGroupItem
                    key={language.value}
                    size="sm"
                    value={language.value}
                  >
                    {language.value}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>
            <Button
              disabled={
                !rwkvConfigReady ||
                selectedBatchCount === 0 ||
                batchTargetLangs.length === 0 ||
                isTranslating
              }
              onClick={() => void translateSelectedBatch()}
              size="sm"
              type="button"
            >
              <Play data-icon="inline-start" />
              创建并翻译
            </Button>
          </div>
        </div>
      </header>
      {pageError ? (
        <div className="border-b border-destructive/20 bg-destructive/5 px-5 py-2 text-sm text-destructive">
          {pageError}
        </div>
      ) : null}

      <div className="min-h-0 overflow-hidden">
        <ResizablePanelGroup
          className="h-full min-h-0 overflow-hidden"
          orientation="horizontal"
        >
          {/* TODO */}
          {/*Replace hardcoded color */}
          <ResizablePanel defaultSize="32%" maxSize="45%" minSize="22%" className="bg-[#fcfbf8] dark:bg-stone-800">
            <SourceFileList
              onOpenSource={(sourceFile) => {
                void openSourceFile(sourceFile);
              }}
              onSelectSource={selectSourceFile}
              onToggleSourceSelection={toggleSourceSelection}
              selectedSourceFileId={selectedSourceFile?.id ?? null}
              selectedSourceFileIds={selectedSourceFileIds}
              sourceFiles={sourceFiles}
              sourceSegmentCountByFileId={sourceSegmentCountByFileId}
              translationFilesBySourceId={translationFilesBySourceId}
            />
          </ResizablePanel>
          <ResizableHandle withHandle />

          {/* TODO */}
          {/*Replace hardcoded color */}
          <ResizablePanel defaultSize="68%" minSize="45%" className="bg-white dark:bg-stone-900">
            <TranslationFileTable
              activeTranslationRun={activeTranslationRun}
              currentJobId={currentJobId}
              isTranslating={isTranslating}
              onOpenTranslation={(translationFile) => {
                void openTranslationFile(translationFile);
              }}
              onExportTranslation={(translationFile, kind) => {
                void exportTranslationFile(translationFile, kind);
              }}
              onSelectTranslation={selectTranslationFile}
              onTranslateTranslation={(translationFile) => {
                void translateTranslationFile(translationFile);
              }}
              onStopTranslation={stopActiveTranslation}
              queuedTranslationFileIds={queuedTranslationFileIds}
              rwkvConfigReady={rwkvConfigReady}
              selectedSourceFile={selectedSourceFile}
              selectedTranslationFileId={selectedTranslationFile?.id ?? null}
              translationFiles={
                selectedSourceFile
                  ? translationFilesBySourceId.get(selectedSourceFile.id) ?? []
                  : []
              }
            />
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>
    </section>
  );
}

function SourceFileList({
  onOpenSource,
  onSelectSource,
  onToggleSourceSelection,
  selectedSourceFileId,
  selectedSourceFileIds,
  sourceFiles,
  sourceSegmentCountByFileId,
  translationFilesBySourceId,
}: {
  onOpenSource: (sourceFile: RosettaSourceFile) => void;
  onSelectSource: (sourceFile: RosettaSourceFile) => void;
  onToggleSourceSelection: (sourceFileId: string) => void;
  selectedSourceFileId: string | null;
  selectedSourceFileIds: string[];
  sourceFiles: RosettaSourceFile[];
  sourceSegmentCountByFileId: Map<string, number>;
  translationFilesBySourceId: Map<string, RosettaTranslationFile[]>;
}) {
  if (sourceFiles.length === 0) {
    return (
      <div className="flex h-48 items-center justify-center text-sm text-muted-foreground">
        当前项目没有源文件。
      </div>
    );
  }

  return (
    <aside className="grid h-full min-h-0 grid-rows-[auto_1fr]  bg-muted/20">
      <div className="flex h-12 items-center justify-between border-b px-3">
        <span className="text-xs font-medium text-muted-foreground">
          原文文件
        </span>
        <Button aria-label="源文件列表选项" size="icon-xs" type="button" variant="ghost">
          <MoreVertical />
        </Button>
      </div>
      <ScrollArea className="min-h-0">
        <div className="flex flex-col">
          {sourceFiles.map((sourceFile) => {
            const selected = selectedSourceFileId === sourceFile.id;
            const translationFileCount =
              translationFilesBySourceId.get(sourceFile.id)?.length ?? 0;

            return (
              <div
                className={cn(
                  "flex min-w-0 items-center gap-3 border-b px-3 py-3 text-left text-sm",
                  selected ? "bg-muted" : "hover:bg-muted/50"
                )}
                key={sourceFile.id}
              >
                <input
                  checked={selectedSourceFileIds.includes(sourceFile.id)}
                  className="shrink-0"
                  onChange={() => onToggleSourceSelection(sourceFile.id)}
                  title="加入批量翻译"
                  type="checkbox"
                />
                <button
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                  onClick={() => onSelectSource(sourceFile)}
                  onDoubleClick={() => onOpenSource(sourceFile)}
                  type="button"
                >
                  {sourceFile.relativePath.includes("/") ||
                    sourceFile.relativePath.includes("\\") ? (
                    <Folder className="size-4 shrink-0 text-muted-foreground" />
                  ) : (
                    <FileText className="size-4 shrink-0 text-muted-foreground" />
                  )}
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-medium">
                      {sourceFile.relativePath}
                    </span>
                    <span className="mt-1 block truncate text-xs text-muted-foreground">
                      {sourceSegmentCountByFileId.get(sourceFile.id) ?? 0} 段 ·{" "}
                      {translationFileCount} 个译文
                    </span>
                  </span>
                </button>
              </div>
            );
          })}
        </div>
      </ScrollArea>
    </aside>
  );
}

function TranslationFileTable({
  activeTranslationRun,
  currentJobId,
  isTranslating,
  onExportTranslation,
  onOpenTranslation,
  onSelectTranslation,
  onStopTranslation,
  onTranslateTranslation,
  queuedTranslationFileIds,
  rwkvConfigReady,
  selectedSourceFile,
  selectedTranslationFileId,
  translationFiles,
}: {
  activeTranslationRun: ActiveTranslationRun | null;
  currentJobId: string | null;
  isTranslating: boolean;
  onExportTranslation: (
    translationFile: RosettaTranslationFile,
    kind: RosettaExportKind
  ) => void;
  onOpenTranslation: (translationFile: RosettaTranslationFile) => void;
  onSelectTranslation: (translationFile: RosettaTranslationFile) => void;
  onStopTranslation: () => void;
  onTranslateTranslation: (translationFile: RosettaTranslationFile) => void;
  queuedTranslationFileIds: string[];
  rwkvConfigReady: boolean;
  selectedSourceFile: RosettaSourceFile | null;
  selectedTranslationFileId: string | null;
  translationFiles: RosettaTranslationFile[];
}) {
  if (!selectedSourceFile) {
    return (
      <section className="flex min-h-0 items-center justify-center text-sm text-muted-foreground">
        选择一个原文文件。
      </section>
    );
  }

  return (
    <section className="grid h-full min-h-0 grid-rows-[auto_1fr] ">
      <div className="flex min-h-32 items-start justify-between gap-4 border-b px-6 py-6">
        <div className="min-w-0">
          <h2 className="truncate text-lg font-semibold">
            {selectedSourceFile.relativePath}
          </h2>
          <p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
            选择一个目标语言来查看细节或管理单个译文任务。双击原文或译文文件打开预览窗口。
          </p>
        </div>
        <TranslationSyncBadge
          activeTranslationRun={activeTranslationRun}
          currentJobId={currentJobId}
          queuedTranslationFileIds={queuedTranslationFileIds}
          translationFiles={translationFiles}
        />
      </div>
      {translationFiles.length === 0 ? (
        <div className="flex min-h-0 items-center justify-center text-sm text-muted-foreground">
          当前原文还没有译文。
        </div>
      ) : (
        <ScrollArea className="min-h-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="h-11 px-6 text-xs font-medium text-muted-foreground">
                  译文
                </TableHead>
                <TableHead className="h-11 w-36 text-xs font-medium text-muted-foreground">
                  语言
                </TableHead>
                <TableHead className="h-11 w-44 text-xs font-medium text-muted-foreground">
                  状态
                </TableHead>
                <TableHead className="h-11 w-56 text-xs font-medium text-muted-foreground">
                  进度
                </TableHead>
                <TableHead className="h-11 w-80 text-right text-xs font-medium text-muted-foreground">
                  操作
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {translationFiles.map((translationFile) => {
                const selected = selectedTranslationFileId === translationFile.id;
                const state = runAwareTranslationState(
                  translationFile,
                  activeTranslationRun,
                  currentJobId,
                  queuedTranslationFileIds
                );
                const isCurrentRun =
                  activeTranslationRun?.jobId === currentJobId &&
                  activeTranslationRun.translationFileId === translationFile.id;
                const canTranslate =
                  rwkvConfigReady &&
                  !isTranslating &&
                  state !== "queued" &&
                  state !== "translating" &&
                  (translationFile.failedSegments > 0 ||
                    translationFile.completedSegments <
                    translationFile.segmentCount);
                const canExport =
                  !isTranslating &&
                  translationFile.segmentCount > 0 &&
                  translationFile.completedSegments >=
                  translationFile.segmentCount &&
                  translationFile.failedSegments === 0;
                const progressPercent = translationProgressPercent(translationFile);

                return (
                  <TableRow
                    className={cn(
                      "h-15 cursor-default",
                      selected && "bg-muted/40"
                    )}
                    data-state={selected ? "selected" : undefined}
                    key={translationFile.id}
                    onClick={() => onSelectTranslation(translationFile)}
                    onDoubleClick={() => onOpenTranslation(translationFile)}
                  >
                    <TableCell className="px-6">
                      <div className="flex min-w-0 items-center gap-2">
                        <Languages className="shrink-0 text-muted-foreground" />
                        <span className="truncate font-medium">
                          {translationLabel(translationFile)}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {translationFile.targetLang}
                    </TableCell>
                    <TableCell>
                      <FileStateBadge
                        state={state}
                      />
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-3">
                        <ProgressBar percent={progressPercent} state={state} />
                        <span className="w-14 text-xs text-muted-foreground">
                          {translationFile.completedSegments}/
                          {translationFile.segmentCount}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <div
                        className="flex justify-end"
                        onClick={(event) => event.stopPropagation()}
                      >
                        <Button
                          className="rounded-r-none"
                          onClick={() => onOpenTranslation(translationFile)}
                          size="sm"
                          type="button"
                          variant="outline"
                        >
                          打开
                        </Button>
                        <Button
                          className="rounded-none border-l-0"
                          disabled={!canTranslate && !isCurrentRun}
                          onClick={() =>
                            isCurrentRun
                              ? onStopTranslation()
                              : onTranslateTranslation(translationFile)
                          }
                          size="sm"
                          type="button"
                          variant="outline"
                        >
                          {isCurrentRun ? (
                            <Square data-icon="inline-start" />
                          ) : translationFile.failedSegments ? (
                            <RefreshCw data-icon="inline-start" />
                          ) : (
                            <Play data-icon="inline-start" />
                          )}
                          {isCurrentRun
                            ? "停止"
                            : state === "translating"
                              ? "翻译中"
                            : state === "queued"
                              ? "排队中"
                              : canTranslate
                                ? "翻译"
                                : "已完成"}
                        </Button>
                        <Button
                          className="rounded-none border-l-0"
                          disabled={!canExport}
                          onClick={() =>
                            onExportTranslation(translationFile, "translation")
                          }
                          size="sm"
                          type="button"
                          variant="outline"
                        >
                          <Download data-icon="inline-start" />
                          导出
                        </Button>
                        <Button
                          className="rounded-l-none border-l-0"
                          disabled={!canExport}
                          onClick={() =>
                            onExportTranslation(translationFile, "bilingual")
                          }
                          size="sm"
                          type="button"
                          variant="outline"
                        >
                          双语
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </ScrollArea>
      )}
    </section>
  );
}

function TranslationSyncBadge({
  activeTranslationRun,
  currentJobId,
  queuedTranslationFileIds,
  translationFiles,
}: {
  activeTranslationRun: ActiveTranslationRun | null;
  currentJobId: string | null;
  queuedTranslationFileIds: string[];
  translationFiles: RosettaTranslationFile[];
}) {
  const syncingCount = translationFiles.filter((translationFile) => {
    const state = runAwareTranslationState(
      translationFile,
      activeTranslationRun,
      currentJobId,
      queuedTranslationFileIds
    );
    return state === "queued" || state === "translating";
  }).length;

  if (syncingCount > 0) {
    return (
      <Badge
        className="border-sky-600/25 bg-sky-600/10 text-sky-700 dark:text-sky-400"
        variant="outline"
      >
        <LoaderCircle className="animate-spin" data-icon="inline-start" />
        {syncingCount} 个译文同步中
      </Badge>
    );
  }

  return (
    <Badge
      className="border-emerald-600/25 bg-emerald-600/10 text-emerald-700 dark:text-emerald-400"
      variant="outline"
    >
      {translationFiles.length} 个译文
    </Badge>
  );
}

function ProgressBar({
  percent,
  state,
}: {
  percent: number;
  state: string;
}) {
  return (
    <div className="h-1.5 w-32 overflow-hidden rounded-full bg-muted">
      <div
        className={cn(
          "h-full rounded-full",
          state === "failed"
            ? "bg-destructive"
            : state === "queued" || state === "translating"
              ? "bg-primary"
              : "bg-emerald-500"
        )}
        style={{ width: `${percent}%` }}
      />
    </div>
  );
}

function FileStateBadge({ state }: { state: string }) {
  if (state === "translated") {
    return (
      <Badge
        className="border-emerald-600/25 bg-emerald-600/10 text-emerald-700 dark:text-emerald-400"
        variant="outline"
      >
        <CheckCircle2 data-icon="inline-start" />
        已完成
      </Badge>
    );
  }
  if (state === "failed") {
    return (
      <Badge variant="destructive">
        <AlertCircle data-icon="inline-start" />
        失败
      </Badge>
    );
  }
  if (state === "translating") {
    return (
      <Badge
        className="border-sky-600/25 bg-sky-600/10 text-sky-700 dark:text-sky-400"
        variant="outline"
      >
        <LoaderCircle className="animate-spin" data-icon="inline-start" />
        翻译中
      </Badge>
    );
  }
  if (state === "queued") {
    return (
      <Badge
        className="border-amber-600/25 bg-amber-600/10 text-amber-700 dark:text-amber-400"
        variant="outline"
      >
        <Clock3 data-icon="inline-start" />
        排队中
      </Badge>
    );
  }
  return (
    <Badge className="text-muted-foreground" variant="outline">
      待翻译
    </Badge>
  );
}

function runAwareTranslationState(
  translationFile: RosettaTranslationFile,
  activeTranslationRun: ActiveTranslationRun | null,
  currentJobId: string | null,
  queuedTranslationFileIds: string[] = []
) {
  if (
    activeTranslationRun?.jobId === currentJobId &&
    activeTranslationRun.translationFileId === translationFile.id
  ) {
    return "translating";
  }
  return queuedTranslationFileIds.includes(translationFile.id)
    ? "queued"
    : translationFile.status;
}

function translationLabel(translationFile: RosettaTranslationFile) {
  const language = LANGUAGE_OPTIONS.find(
    (option) => option.value === translationFile.targetLang
  );
  return language?.label ?? translationFile.targetLang;
}

function groupTranslationFilesBySource(translationFiles: RosettaTranslationFile[]) {
  const grouped = new Map<string, RosettaTranslationFile[]>();
  for (const translationFile of translationFiles) {
    const group = grouped.get(translationFile.sourceFileId);
    if (group) {
      group.push(translationFile);
    } else {
      grouped.set(translationFile.sourceFileId, [translationFile]);
    }
  }
  for (const group of grouped.values()) {
    group.sort((left, right) => left.targetLang.localeCompare(right.targetLang));
  }
  return grouped;
}

function groupSourceSegmentsByFile(segments: Segment[]) {
  const grouped = new Map<string, Segment[]>();
  for (const segment of segments) {
    const fileId = segment.fileId ?? "file-1";
    const group = grouped.get(fileId);
    if (group) {
      group.push(segment);
    } else {
      grouped.set(fileId, [segment]);
    }
  }
  return grouped;
}

function countSourceSegmentsByFile(segments: { fileId?: string | null }[]) {
  const grouped = new Map<string, number>();
  for (const segment of segments) {
    const fileId = segment.fileId ?? "file-1";
    grouped.set(fileId, (grouped.get(fileId) ?? 0) + 1);
  }
  return grouped;
}
