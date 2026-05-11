import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  CheckCircle2,
  Download,
  FilePlus,
  FileText,
  Folder,
  Languages,
  Play,
  RefreshCw,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
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
import { openTranslationPreviewWindow } from "../../lib/translationPreviewWindow";
import { translateRwkvTextsWithApi } from "../../lib/rwkvApi";
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

const BATCH_SIZE = 16;

const LANGUAGE_OPTIONS = [
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
  { value: "es", label: "Español" },
  { value: "ru", label: "Русский" },
  { value: "pt", label: "Português" },
  { value: "it", label: "Italiano" },
  { value: "vi", label: "Tiếng Việt" },
  { value: "id", label: "Bahasa Indonesia" },
];

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

  const [isLoading, setIsLoading] = useState(false);
  const [isTranslating, setIsTranslating] = useState(false);
  const [targetLang, setTargetLang] = useState("zh-CN");
  const [batchTargetLangs, setBatchTargetLangs] = useState<string[]>(["zh-CN"]);
  const [selectedSourceFileIds, setSelectedSourceFileIds] = useState<string[]>(
    []
  );
  const [selectedTranslationFileIds, setSelectedTranslationFileIds] = useState<
    string[]
  >([]);
  const loadRequestIdRef = useRef(0);

  const currentJobId = jobId ?? activeJobId ?? jobs[0]?.id ?? null;
  const activeJob = jobs.find((job) => job.id === currentJobId) ?? null;
  const isCurrentBundleLoaded = activeJobId === currentJobId && activeDocument != null;
  const document = isCurrentBundleLoaded ? activeDocument : null;
  const sourceFiles = document?.files ?? activeJob?.sourceFiles ?? [];
  const selectedSourceFileId =
    fileId ??
    (currentJobId ? activeSourceFileIdByJobId[currentJobId] : null) ??
    (activeJobId === currentJobId ? activeSourceFileId : null) ??
    sourceFiles[0]?.id ??
    null;
  const selectedSourceFile =
    sourceFiles.find((file) => file.id === selectedSourceFileId) ?? null;
  const selectedTranslationFile =
    translationFiles.find(
      (file) =>
        file.id === activeTranslationFileId &&
        file.sourceFileId === selectedSourceFileId
    ) ?? null;
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
  const rwkvConfigReady =
    rwkv.baseUrl.trim().length > 0 &&
    rwkv.endpoint.trim().length > 0 &&
    rwkv.internalToken.trim().length > 0 &&
    rwkv.bodyPassword.trim().length > 0 &&
    rwkv.timeoutMs > 0;
  const canTranslateSelectedTranslation =
    selectedTranslationFile != null &&
    (selectedTranslationFile.failedSegments > 0 ||
      selectedTranslationFile.completedSegments < selectedTranslationFile.segmentCount);
  const isSelectedTranslationComplete =
    selectedTranslationFile != null &&
    selectedTranslationFile.segmentCount > 0 &&
    selectedTranslationFile.completedSegments >= selectedTranslationFile.segmentCount &&
    selectedTranslationFile.failedSegments === 0;
  const selectedBatchCount =
    selectedSourceFileIds.length + selectedTranslationFileIds.length;

  useEffect(() => {
    if (!jobId && jobs.length > 0) {
      navigate(rosettaJobDefaultPath(jobs[0]), { replace: true });
    }
  }, [jobId, jobs, navigate]);

  useEffect(() => {
    if (!currentJobId) {
      setIsLoading(false);
      return;
    }

    const currentState = useRosettaStore.getState();
    if (
      currentState.activeJobId === currentJobId &&
      currentState.activeDocument
    ) {
      setIsLoading(false);
      return;
    }

    const requestId = loadRequestIdRef.current + 1;
    loadRequestIdRef.current = requestId;
    setIsLoading(true);
    void loadRosettaJob(currentJobId)
      .then((bundle) => {
        if (
          loadRequestIdRef.current !== requestId ||
          bundle.job.id !== currentJobId
        ) {
          return;
        }
        setActiveBundle(bundle);
      })
      .catch(console.error)
      .finally(() => {
        if (loadRequestIdRef.current === requestId) {
          setIsLoading(false);
        }
      });
  }, [currentJobId, setActiveBundle]);

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

  async function createTranslationFileForCurrentSource() {
    if (!currentJobId || !selectedSourceFile) {
      return null;
    }
    const bundle = await ensureRosettaTranslationFile(
      currentJobId,
      selectedSourceFile.id,
      targetLang
    );
    upsertTranslationFile(bundle.translationFile);
    setActiveJobSelection(
      currentJobId,
      selectedSourceFile.id,
      bundle.translationFile.id
    );
    refreshJobBundle(await loadRosettaJob(currentJobId));
    return bundle.translationFile;
  }

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

  function toggleSourceSelection(sourceFileId: string) {
    setSelectedSourceFileIds((current) =>
      current.includes(sourceFileId)
        ? current.filter((id) => id !== sourceFileId)
        : [...current, sourceFileId]
    );
  }

  function toggleTranslationSelection(translationFileId: string) {
    setSelectedTranslationFileIds((current) =>
      current.includes(translationFileId)
        ? current.filter((id) => id !== translationFileId)
        : [...current, translationFileId]
    );
  }

  async function translateCurrentTranslationFile() {
    if (!currentJobId) {
      return;
    }
    if (!selectedTranslationFile) {
      const created = await createTranslationFileForCurrentSource();
      if (!created) {
        return;
      }
      await translateTranslationFile(created);
      return;
    }
    await translateTranslationFile(selectedTranslationFile);
  }

  async function translateSelectedBatch() {
    if (!currentJobId || selectedBatchCount === 0) {
      return;
    }

    try {
      for (const translationFileId of selectedTranslationFileIds) {
        const translationFile = translationFiles.find(
          (file) => file.id === translationFileId
        );
        if (translationFile) {
          await translateTranslationFile(translationFile, "batch");
        }
      }

      for (const sourceFileId of selectedSourceFileIds) {
        for (const language of batchTargetLangs) {
          const ensured = await ensureRosettaTranslationFile(
            currentJobId,
            sourceFileId,
            language
          );
          upsertTranslationFile(ensured.translationFile);
          await translateTranslationFile(ensured.translationFile, "batch", ensured.segments);
        }
      }

      refreshJobBundle(await loadRosettaJob(currentJobId));
    } catch (error) {
      console.error(error);
    }
  }

  async function translateTranslationFile(
    translationFile: RosettaTranslationFile,
    scope: "file" | "batch" = "file",
    initialSegments?: TranslationSegment[]
  ) {
    if (!currentJobId || !document || !rwkvConfigReady || isTranslating) {
      return;
    }
    const sourceFile =
      document.files.find((file) => file.id === translationFile.sourceFileId) ??
      null;
    if (!sourceFile) {
      return;
    }

    const loadedSegments =
      initialSegments ??
      (await loadRosettaTranslationFile(currentJobId, translationFile.id)).segments;
    const sourceSegmentsForFile = sourceSegmentsByFileId.get(sourceFile.id) ?? [];
    const sourceById = new Map(
      sourceSegmentsForFile.map((segment) => [segment.id, segment])
    );
    const targets = loadedSegments.filter((segment) => {
      const source = sourceById.get(segment.sourceSegmentId);
      return (
        source != null &&
        source.sourceText.trim().length > 0 &&
        ["pending", "failed"].includes(segment.status)
      );
    });

    if (targets.length === 0) {
      return;
    }

    const runId = `run-${Date.now()}`;
    setIsTranslating(true);
    startTranslationRun({
      id: runId,
      jobId: currentJobId,
      sourceFileId: sourceFile.id,
      translationFileId: translationFile.id,
      scope,
      targetSegmentIds: targets.map((segment) => segment.sourceSegmentId),
    });

    let workingSegments = loadedSegments;
    let currentBatchSegmentIds: string[] = [];
    try {
      const orderedTargets = targets.sort((left, right) => {
        const leftSource = sourceById.get(left.sourceSegmentId);
        const rightSource = sourceById.get(right.sourceSegmentId);
        return (leftSource?.order ?? 0) - (rightSource?.order ?? 0);
      });

      for (const batch of chunkSegments(orderedTargets, BATCH_SIZE)) {
        currentBatchSegmentIds = batch.map((segment) => segment.sourceSegmentId);
        workingSegments = markSegmentsTranslating(
          workingSegments,
          currentBatchSegmentIds
        );

        const result = await translateRwkvTextsWithApi({
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          sourceLang: sourceFile.sourceLang ?? document.sourceLang,
          targetLang: translationFile.targetLang,
          sourceTexts: batch.map(
            (segment) => sourceById.get(segment.sourceSegmentId)?.sourceText ?? ""
          ),
        });

        if (!result.ok || result.translations.length !== batch.length) {
          const message = !result.ok
            ? result.message
            : `RWKV API 返回 ${result.translations.length} 条译文，但本批有 ${batch.length} 条文本。`;
          workingSegments = markSegmentsFailed(
            workingSegments,
            currentBatchSegmentIds,
            message
          );
          markTranslationRunFailed(runId, currentBatchSegmentIds);
          const saved = await saveRosettaTranslationSegments(
            currentJobId,
            translationFile.id,
            workingSegments
          );
          upsertTranslationFile(saved.translationFile);
          return;
        }

        workingSegments = markSegmentsDone(
          workingSegments,
          currentBatchSegmentIds,
          result.translations
        );
        markTranslationRunCompleted(runId, currentBatchSegmentIds);
        const saved = await saveRosettaTranslationSegments(
          currentJobId,
          translationFile.id,
          workingSegments
        );
        upsertTranslationFile(saved.translationFile);
      }
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "RWKV API 翻译调用失败。";
      if (currentBatchSegmentIds.length > 0) {
        workingSegments = markSegmentsFailed(
          workingSegments,
          currentBatchSegmentIds,
          message
        );
        markTranslationRunFailed(runId, currentBatchSegmentIds);
        const saved = await saveRosettaTranslationSegments(
          currentJobId,
          translationFile.id,
          workingSegments
        );
        upsertTranslationFile(saved.translationFile);
      }
    } finally {
      finishTranslationRun(runId);
      setIsTranslating(false);
    }
  }

  async function exportCurrentTranslationFile(kind: RosettaExportKind) {
    if (!currentJobId || !selectedTranslationFile) {
      return;
    }
    const sourceFile = selectedSourceFile;
    if (!sourceFile) {
      return;
    }
    const targetPath = await pickRosettaExportPath(
      defaultExportFilename(
        sourceFile.relativePath,
        sourceFile.format,
        selectedTranslationFile.targetLang,
        kind
      ),
      sourceFile.format
    );
    if (!targetPath) {
      return;
    }
    await exportRosettaTranslationFile(
      currentJobId,
      selectedTranslationFile.id,
      kind,
      targetPath
    );
    refreshJobBundle(await loadRosettaJob(currentJobId));
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
    <section className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-3 px-5 pb-5">
      <Card className="gap-0 py-0">
        <CardHeader className="flex-row items-center justify-between gap-4 border-b py-3">
          <div className="min-w-0">
            <CardTitle className="truncate text-base">
              {activeJob?.filename ?? "项目"}
            </CardTitle>
            <div className="mt-1 flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
              <span>{sourceFiles.length} 个源文件</span>
              <span>{translationFiles.length} 个译文文件</span>
              {selectedTranslationFile ? (
                <FileStateBadge
                  state={runAwareTranslationState(
                    selectedTranslationFile,
                    activeTranslationRun,
                    currentJobId
                  )}
                />
              ) : selectedSourceFile ? (
                <span className="truncate">{selectedSourceFile.relativePath}</span>
              ) : null}
            </div>
          </div>

          <div className="flex flex-wrap items-center justify-end gap-2">
            <LanguageSelect
              disabled={isLoading || isTranslating}
              onValueChange={setTargetLang}
              value={targetLang}
            />
            <Button
              disabled={!selectedSourceFile || isLoading || isTranslating}
              onClick={() => void createTranslationFileForCurrentSource()}
              type="button"
              variant="outline"
            >
              <FilePlus data-icon="inline-start" />
              新建译文
            </Button>
            <Button
              disabled={
                !rwkvConfigReady ||
                isLoading ||
                isTranslating ||
                (!selectedTranslationFile && !selectedSourceFile) ||
                (selectedTranslationFile != null && !canTranslateSelectedTranslation)
              }
              onClick={() => void translateCurrentTranslationFile()}
              type="button"
            >
              {selectedTranslationFile?.failedSegments ? (
                <RefreshCw data-icon="inline-start" />
              ) : (
                <Play data-icon="inline-start" />
              )}
              {selectedTranslationFile
                ? canTranslateSelectedTranslation
                  ? "翻译译文"
                  : "已完成"
                : "生成并翻译"}
            </Button>
            <Button
              disabled={!isSelectedTranslationComplete || isTranslating}
              onClick={() => void exportCurrentTranslationFile("translation")}
              type="button"
              variant="outline"
            >
              <Download data-icon="inline-start" />
              导出译文
            </Button>
            <Button
              disabled={!isSelectedTranslationComplete || isTranslating}
              onClick={() => void exportCurrentTranslationFile("bilingual")}
              type="button"
              variant="outline"
            >
              <Languages data-icon="inline-start" />
              导出双语
            </Button>
          </div>
        </CardHeader>

        <CardContent className="flex flex-wrap items-center justify-between gap-3 py-3">
          <div className="flex min-w-0 items-center gap-2 text-sm text-muted-foreground">
            <span>批量翻译</span>
            <span>{selectedBatchCount} 项已选择</span>
          </div>
          <div className="flex flex-wrap items-center justify-end gap-2">
            <ToggleGroup
              disabled={isTranslating}
              onValueChange={(values) => setBatchTargetLangs(values)}
              type="multiple"
              value={batchTargetLangs}
            >
              {LANGUAGE_OPTIONS.slice(0, 6).map((language) => (
                <ToggleGroupItem key={language.value} size="sm" value={language.value}>
                  {language.value}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <Button
              disabled={
                !rwkvConfigReady ||
                selectedBatchCount === 0 ||
                (selectedSourceFileIds.length > 0 && batchTargetLangs.length === 0) ||
                isTranslating
              }
              onClick={() => void translateSelectedBatch()}
              size="sm"
              type="button"
            >
              <Play data-icon="inline-start" />
              批量开始
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card className="min-h-0 gap-0 overflow-hidden py-0">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <div className="min-w-0">
            <h2 className="text-sm font-medium">项目文件</h2>
            <p className="mt-1 text-sm text-muted-foreground">
              左侧选择原文，右侧管理该原文的多语言译文。双击译文打开对照窗口。
            </p>
          </div>
        </div>
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(16rem,24rem)_minmax(0,1fr)] overflow-hidden">
          <SourceFileList
            onSelectSource={selectSourceFile}
            onToggleSourceSelection={toggleSourceSelection}
            selectedSourceFileId={selectedSourceFile?.id ?? null}
            selectedSourceFileIds={selectedSourceFileIds}
            sourceFiles={sourceFiles}
            sourceSegmentCountByFileId={sourceSegmentCountByFileId}
            translationFilesBySourceId={translationFilesBySourceId}
          />
          <TranslationFileTable
            activeTranslationRun={activeTranslationRun}
            currentJobId={currentJobId}
            onOpenTranslation={(translationFile) => {
              void openTranslationFile(translationFile);
            }}
            onSelectTranslation={selectTranslationFile}
            onToggleTranslationSelection={toggleTranslationSelection}
            selectedSourceFile={selectedSourceFile}
            selectedTranslationFileId={selectedTranslationFile?.id ?? null}
            selectedTranslationFileIds={selectedTranslationFileIds}
            translationFiles={
              selectedSourceFile
                ? translationFilesBySourceId.get(selectedSourceFile.id) ?? []
                : []
            }
          />
        </div>
      </Card>
    </section>
  );
}

function SourceFileList({
  onSelectSource,
  onToggleSourceSelection,
  selectedSourceFileId,
  selectedSourceFileIds,
  sourceFiles,
  sourceSegmentCountByFileId,
  translationFilesBySourceId,
}: {
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
    <aside className="min-h-0 overflow-auto border-r bg-muted/20">
      <div className="border-b px-3 py-2 text-sm font-medium">原文</div>
      <div className="flex flex-col gap-1 p-2">
        {sourceFiles.map((sourceFile) => {
          const selected = selectedSourceFileId === sourceFile.id;
          const translationFileCount =
            translationFilesBySourceId.get(sourceFile.id)?.length ?? 0;

          return (
            <div
              className={cn(
                "flex min-w-0 items-center gap-2 rounded-md px-2 py-2 text-left text-sm",
                selected
                  ? "bg-sidebar-accent text-sidebar-accent-foreground"
                  : "hover:bg-muted"
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
                  <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                    {sourceSegmentCountByFileId.get(sourceFile.id) ?? 0} 段 ·{" "}
                    {translationFileCount} 个译文
                  </span>
                </span>
              </button>
            </div>
          );
        })}
      </div>
    </aside>
  );
}

function TranslationFileTable({
  activeTranslationRun,
  currentJobId,
  onOpenTranslation,
  onSelectTranslation,
  onToggleTranslationSelection,
  selectedSourceFile,
  selectedTranslationFileId,
  selectedTranslationFileIds,
  translationFiles,
}: {
  activeTranslationRun: ActiveTranslationRun | null;
  currentJobId: string | null;
  onOpenTranslation: (translationFile: RosettaTranslationFile) => void;
  onSelectTranslation: (translationFile: RosettaTranslationFile) => void;
  onToggleTranslationSelection: (translationFileId: string) => void;
  selectedSourceFile: RosettaSourceFile | null;
  selectedTranslationFileId: string | null;
  selectedTranslationFileIds: string[];
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
    <section className="grid min-h-0 grid-rows-[auto_1fr]">
      <div className="border-b px-4 py-3">
        <h3 className="truncate text-sm font-medium">
          {selectedSourceFile.relativePath}
        </h3>
        <p className="mt-1 text-sm text-muted-foreground">
          {translationFiles.length} 个译文文件
        </p>
      </div>
      {translationFiles.length === 0 ? (
        <div className="flex min-h-0 items-center justify-center text-sm text-muted-foreground">
          当前原文还没有译文。
        </div>
      ) : (
        <div className="min-h-0 overflow-auto">
          <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="w-10" />
          <TableHead>译文</TableHead>
          <TableHead className="w-32">语言</TableHead>
          <TableHead className="w-32">状态</TableHead>
          <TableHead className="w-36 text-right">进度</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {translationFiles.map((translationFile) => {
                const selected = selectedTranslationFileId === translationFile.id;

                return (
                  <TableRow
                    className={cn("cursor-default", selected && "bg-muted/60")}
                    data-state={selected ? "selected" : undefined}
                    key={translationFile.id}
                    onClick={() => onSelectTranslation(translationFile)}
                    onDoubleClick={() => onOpenTranslation(translationFile)}
                  >
                    <TableCell onClick={(event) => event.stopPropagation()}>
                      <input
                        checked={selectedTranslationFileIds.includes(
                          translationFile.id
                        )}
                        onChange={() =>
                          onToggleTranslationSelection(translationFile.id)
                        }
                        title="加入批量翻译"
                        type="checkbox"
                      />
                    </TableCell>
                    <TableCell>
                      <div className="flex min-w-0 items-center gap-2">
                        <Languages className="size-4 shrink-0 text-muted-foreground" />
                        <span className="truncate">
                          {translationLabel(translationFile)}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {translationFile.targetLang}
                    </TableCell>
                    <TableCell>
                      <FileStateBadge
                        state={runAwareTranslationState(
                          translationFile,
                          activeTranslationRun,
                          currentJobId
                        )}
                      />
                    </TableCell>
                    <TableCell className="text-right text-muted-foreground">
                      {translationFile.completedSegments}/
                      {translationFile.segmentCount}
                    </TableCell>
                  </TableRow>
                );
        })}
      </TableBody>
    </Table>
        </div>
      )}
    </section>
  );
}

function LanguageSelect({
  disabled,
  onValueChange,
  value,
}: {
  disabled: boolean;
  onValueChange: (value: string) => void;
  value: string;
}) {
  return (
    <Select disabled={disabled} onValueChange={onValueChange} value={value}>
      <SelectTrigger aria-label="选择目标语言" size="sm">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {LANGUAGE_OPTIONS.map((language) => (
            <SelectItem key={language.value} value={language.value}>
              {language.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
}

function FileStateBadge({ state }: { state: string }) {
  if (state === "translated") {
    return (
      <Badge variant="secondary">
        <CheckCircle2 data-icon="inline-start" />
        已完成
      </Badge>
    );
  }
  if (state === "failed") {
    return <Badge variant="destructive">失败</Badge>;
  }
  if (state === "translating") {
    return <Badge variant="secondary">翻译中</Badge>;
  }
  return <Badge variant="outline">待翻译</Badge>;
}

function runAwareTranslationState(
  translationFile: RosettaTranslationFile,
  activeTranslationRun: ActiveTranslationRun | null,
  currentJobId: string | null
) {
  return activeTranslationRun?.jobId === currentJobId &&
    activeTranslationRun.translationFileId === translationFile.id
    ? "translating"
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

function chunkSegments<T>(segments: T[], size: number) {
  const chunks: T[][] = [];
  for (let index = 0; index < segments.length; index += size) {
    chunks.push(segments.slice(index, index + size));
  }
  return chunks;
}

function markSegmentsTranslating(
  segments: TranslationSegment[],
  segmentIds: string[]
) {
  const segmentIdSet = new Set(segmentIds);
  return segments.map((segment) =>
    segmentIdSet.has(segment.sourceSegmentId)
      ? {
          ...segment,
          status: "translating" as const,
          translatedText: undefined,
          error: undefined,
        }
      : segment
  );
}

function markSegmentsDone(
  segments: TranslationSegment[],
  segmentIds: string[],
  translations: string[]
) {
  const translationById = new Map(
    segmentIds.map((segmentId, index) => [segmentId, translations[index]])
  );
  return segments.map((segment) =>
    translationById.has(segment.sourceSegmentId)
      ? {
          ...segment,
          translatedText: translationById.get(segment.sourceSegmentId),
          status: "done" as const,
          error: undefined,
        }
      : segment
  );
}

function markSegmentsFailed(
  segments: TranslationSegment[],
  segmentIds: string[],
  error: string
) {
  const segmentIdSet = new Set(segmentIds);
  return segments.map((segment) =>
    segmentIdSet.has(segment.sourceSegmentId)
      ? {
          ...segment,
          status: "failed" as const,
          error,
        }
      : segment
  );
}

function defaultExportFilename(
  relativePath: string,
  format: "txt" | "markdown",
  targetLang: string,
  kind: RosettaExportKind
) {
  const extension = format === "markdown" ? "md" : "txt";
  const filename = relativePath.split(/[\\/]/).pop() ?? relativePath;
  const baseName = filename.replace(/\.(txt|md|markdown)$/i, "");
  const suffix = kind === "bilingual" ? `${targetLang}.bilingual` : targetLang;
  return `${baseName}.${suffix}.${extension}`;
}
