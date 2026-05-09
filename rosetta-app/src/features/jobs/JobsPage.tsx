import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  ArrowRight,
  CheckCircle2,
  Download,
  FilePlus,
  FileText,
  Languages,
  Play,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import { DocumentPreview } from "../preview/DocumentPreview";
import { useRosettaStore } from "../../store/useRosettaStore";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
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
import { translateRwkvTextsWithApi } from "../../lib/rwkvApi";
import {
  createRosettaTranslationRevision,
  deleteRosettaJobFile,
  exportRosettaJobFile,
  loadRosettaJob,
  pickRosettaExportPath,
  saveRosettaSegments,
  updateRosettaJobLanguages,
} from "../../lib/rosettaJobs";
import { cn } from "../../lib/utils";
import type { RosettaExportKind, Segment } from "../../types/rosetta";

const BATCH_SIZE = 16;
const LANGUAGE_OPTIONS = [
  { value: "en", label: "English" },
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
  const { jobId } = useParams();
  const navigate = useNavigate();
  const jobs = useRosettaStore((state) => state.jobs);
  const activeJobId = useRosettaStore((state) => state.activeJobId);
  const activeFileId = useRosettaStore((state) => state.activeFileId);
  const activeFileIdByJobId = useRosettaStore(
    (state) => state.activeFileIdByJobId
  );
  const activeDocument = useRosettaStore((state) => state.activeDocument);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const previewSegments = useRosettaStore((state) => state.previewSegments);
  const translationRevisions = useRosettaStore(
    (state) => state.translationRevisions
  );
  const activeTranslationRun = useRosettaStore(
    (state) => state.activeTranslationRun
  );
  const setJobList = useRosettaStore((state) => state.setJobList);
  const setActiveBundle = useRosettaStore((state) => state.setActiveBundle);
  const clearActiveJob = useRosettaStore((state) => state.clearActiveJob);
  const beginPreviewSegmentTranslation = useRosettaStore(
    (state) => state.beginPreviewSegmentTranslation
  );
  const completePreviewSegmentTranslation = useRosettaStore(
    (state) => state.completePreviewSegmentTranslation
  );
  const failPreviewSegmentTranslation = useRosettaStore(
    (state) => state.failPreviewSegmentTranslation
  );
  const preparePreviewSegmentRetranslation = useRosettaStore(
    (state) => state.preparePreviewSegmentRetranslation
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
  const [isSavingLanguages, setIsSavingLanguages] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [selectedRevisionId, setSelectedRevisionId] = useState("current");
  const [selectedBlockIds, setSelectedBlockIds] = useState<string[]>([]);
  const currentJobId = jobId ?? activeJobId ?? jobs[0]?.id ?? null;
  const activeJob = jobs.find((job) => job.id === currentJobId) ?? null;
  const isCurrentBundleLoaded = activeJobId === currentJobId && activeDocument != null;
  const currentDocument = isCurrentBundleLoaded ? activeDocument : null;
  const currentSegments = isCurrentBundleLoaded ? previewSegments : [];
  const selectedFileId = currentJobId
    ? activeFileIdByJobId[currentJobId] ??
      (activeJobId === currentJobId ? activeFileId : null)
    : null;
  const currentFile =
    currentDocument?.files.find((file) => file.id === selectedFileId) ??
    currentDocument?.files[0] ??
    activeJob?.sourceFiles?.find((file) => file.id === selectedFileId) ??
    activeJob?.sourceFiles?.[0] ??
    null;
  const currentFileSegments = useMemo(
    () =>
      currentSegments.filter((segment) =>
        currentFile ? (segment.fileId ?? "file-1") === currentFile.id : false
      ),
    [currentFile, currentSegments]
  );
  const sourceLang = currentDocument?.sourceLang ?? "en";
  const targetLang = currentDocument?.targetLang ?? activeJob?.targetLang ?? "zh-CN";
  const pendingSegments = useMemo(
    () =>
      currentFileSegments.filter(
        (segment) =>
          ["pending", "failed"].includes(segment.status) &&
          segment.sourceText.trim().length > 0
      ),
    [currentFileSegments]
  );
  const retranslationSegments = useMemo(
    () =>
      currentFileSegments.filter(
        (segment) =>
          ["done", "edited"].includes(segment.status) &&
          segment.sourceText.trim().length > 0
      ),
    [currentFileSegments]
  );
  const translatableFileSegments = useMemo(
    () =>
      currentFileSegments.filter(
        (segment) => segment.status !== "skipped" && segment.sourceText.trim()
      ),
    [currentFileSegments]
  );
  const selectedSegmentIds = useMemo(() => {
    const selectedBlockIdSet = new Set(selectedBlockIds);
    return currentFileSegments
      .filter(
        (segment) =>
          selectedBlockIdSet.has(segment.blockId) &&
          segment.status !== "skipped" &&
          segment.sourceText.trim()
      )
      .map((segment) => segment.id);
  }, [currentFileSegments, selectedBlockIds]);
  const selectedSegments = useMemo(() => {
    const selectedSegmentIdSet = new Set(selectedSegmentIds);
    return currentFileSegments.filter((segment) =>
      selectedSegmentIdSet.has(segment.id)
    );
  }, [currentFileSegments, selectedSegmentIds]);
  const failedSegments = useMemo(
    () =>
      currentFileSegments.filter(
        (segment) => segment.status === "failed" && segment.sourceText.trim()
      ),
    [currentFileSegments]
  );
  const isCurrentFileTranslating = currentFileSegments.some(
    (segment) => segment.status === "translating"
  );
  const currentFileTranslationRun =
    activeTranslationRun &&
    activeTranslationRun.jobId === currentJobId &&
    activeTranslationRun.fileId === currentFile?.id
      ? activeTranslationRun
      : null;
  const isCurrentFileTranslationRunActive = currentFileTranslationRun != null;
  const rwkvConfigReady =
    rwkv.baseUrl.trim().length > 0 &&
    rwkv.endpoint.trim().length > 0 &&
    rwkv.internalToken.trim().length > 0 &&
    rwkv.bodyPassword.trim().length > 0 &&
    rwkv.timeoutMs > 0;
  const translatedFileSegments = translatableFileSegments.filter((segment) =>
    ["done", "edited"].includes(segment.status)
  ).length;
  const skippedFileSegments = currentFileSegments.filter(
    (segment) => segment.status === "skipped"
  ).length;
  const fileProgress =
    currentFileTranslationRun
      ? Math.round(
          (currentFileTranslationRun.completedSegmentIds.length /
            Math.max(currentFileTranslationRun.targetSegmentIds.length, 1)) *
            100
        )
      : translatableFileSegments.length > 0
      ? Math.round((translatedFileSegments / translatableFileSegments.length) * 100)
      : 0;
  const currentFileState =
    isCurrentFileTranslating || isCurrentFileTranslationRunActive
    ? "翻译中"
    : failedSegments.length > 0
      ? "需要重试"
      : pendingSegments.length > 0
        ? "待翻译"
        : translatableFileSegments.length > 0
          ? "已完成"
          : "无内容";
  const primaryActionLabel = isCurrentFileTranslationRunActive
    ? "翻译中"
    : failedSegments.length > 0
      ? "重试失败"
      : pendingSegments.length > 0
        ? "开始翻译"
      : retranslationSegments.length > 0
          ? "重新翻译全文"
          : "已完成";
  const primaryTranslationSegments =
    failedSegments.length > 0
      ? failedSegments
      : pendingSegments.length > 0
        ? pendingSegments
        : retranslationSegments;
  const isCurrentFileExportable =
    currentFile != null &&
    translatedFileSegments > 0 &&
    translatableFileSegments.every((segment) =>
      ["done", "edited"].includes(segment.status)
    );
  const fileProgressText = currentFileTranslationRun
    ? `${currentFileTranslationRun.completedSegmentIds.length}/${currentFileTranslationRun.targetSegmentIds.length}`
    : translatableFileSegments.length > 0
      ? `${translatedFileSegments}/${translatableFileSegments.length}`
      : "0/0";
  const selectionEnabled =
    selectedRevisionId === "current" &&
    isCurrentFileExportable &&
    !isTranslating &&
    !isCurrentFileTranslationRunActive;
  const hasSelectedBlocks =
    selectedBlockIds.length > 0 && selectedRevisionId === "current";
  const mainActionLabel = hasSelectedBlocks ? "重翻选中" : primaryActionLabel;
  const mainActionDisabled = hasSelectedBlocks
    ? !rwkvConfigReady ||
      selectedSegments.length === 0 ||
      isTranslating ||
      isLoading
    : !rwkvConfigReady ||
      primaryTranslationSegments.length === 0 ||
      isTranslating ||
      isLoading;
  const mainActionVariant =
    hasSelectedBlocks || pendingSegments.length > 0 || failedSegments.length > 0
      ? "default"
      : "outline";
  useEffect(() => {
    if (!jobId && jobs.length > 0) {
      navigate(`/jobs/${jobs[0].id}`, { replace: true });
    }
  }, [jobId, jobs, navigate]);

  useEffect(() => {
    if (!currentJobId) {
      return;
    }
    if (activeJobId === currentJobId && activeDocument) {
      return;
    }

    setIsLoading(true);
    void loadRosettaJob(currentJobId)
      .then(setActiveBundle)
      .catch((error) => {
        console.error(error);
      })
      .finally(() => setIsLoading(false));
  }, [activeDocument, activeJobId, currentJobId, setActiveBundle]);

  useEffect(() => {
    setSelectedBlockIds([]);
    setSelectedRevisionId("current");
  }, [currentFile?.id]);

  async function translateSegments(
    targetSegments: Segment[],
    scope: "file" | "selection" | "retry-failed"
  ) {
    if (
      !currentJobId ||
      !currentFile ||
      !rwkvConfigReady ||
      targetSegments.length === 0 ||
      isTranslating
    ) {
      return;
    }

    setIsTranslating(true);
    startTranslationRun({
      id: `run-${Date.now()}`,
      jobId: currentJobId,
      fileId: currentFile.id,
      scope,
      targetSegmentIds: targetSegments.map((segment) => segment.id),
    });

    let currentBatchSegmentIds: string[] = [];
    try {
      const orderedSegments = [...targetSegments].sort(
        (left, right) => left.order - right.order
      );

      for (const batch of chunkSegments(orderedSegments, BATCH_SIZE)) {
        currentBatchSegmentIds = batch.map((segment) => segment.id);
        beginPreviewSegmentTranslation(currentBatchSegmentIds);

        const result = await translateRwkvTextsWithApi({
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          sourceLang,
          targetLang,
          sourceTexts: batch.map((segment) => segment.sourceText),
        });

        if (!result.ok) {
          const failed = failPreviewSegmentTranslation(
            currentBatchSegmentIds,
            result.message
          );
          markTranslationRunFailed(currentBatchSegmentIds);
          await saveRosettaSegments(currentJobId, failed).then(setActiveBundle);
          return;
        }

        if (result.translations.length !== batch.length) {
          const message = `RWKV API 返回 ${result.translations.length} 条译文，但本批有 ${batch.length} 条文本。`;
          const failed = failPreviewSegmentTranslation(
            currentBatchSegmentIds,
            message
          );
          markTranslationRunFailed(currentBatchSegmentIds);
          await saveRosettaSegments(currentJobId, failed).then(setActiveBundle);
          return;
        }

        const completed = completePreviewSegmentTranslation(
          currentBatchSegmentIds,
          result.translations
        );
        markTranslationRunCompleted(currentBatchSegmentIds);
        await saveRosettaSegments(currentJobId, completed).then(setActiveBundle);
      }
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "RWKV API 翻译调用失败。";
      if (currentBatchSegmentIds.length > 0) {
        const failed = failPreviewSegmentTranslation(currentBatchSegmentIds, message);
        markTranslationRunFailed(currentBatchSegmentIds);
        try {
          await saveRosettaSegments(currentJobId, failed).then(setActiveBundle);
        } catch {
          // Ignore secondary save errors so the original failure can surface.
        }
      }
    } finally {
      setIsTranslating(false);
      finishTranslationRun();
    }
  }

  async function translateCurrentFile() {
    if (!currentJobId || !currentFile || isTranslating) {
      return;
    }

    if (failedSegments.length > 0) {
      setSelectedBlockIds([]);
      setSelectedRevisionId("current");
      await translateSegments(failedSegments, "retry-failed");
      return;
    }

    if (pendingSegments.length > 0) {
      setSelectedBlockIds([]);
      setSelectedRevisionId("current");
      await translateSegments(pendingSegments, "file");
      return;
    }

    if (translatableFileSegments.length === 0) {
      return;
    }

    setSelectedBlockIds([]);
    setSelectedRevisionId("current");
    await createRosettaTranslationRevision(
      currentJobId,
      currentFile.id,
      "file-retranslation"
    ).then(setActiveBundle);
    const segmentIds = translatableFileSegments.map((segment) => segment.id);
    const preparedSegments = preparePreviewSegmentRetranslation(segmentIds);
    await saveRosettaSegments(currentJobId, preparedSegments).then(setActiveBundle);
    await translateSegments(translatableFileSegments, "file");
  }

  async function translateSelectedBlocks() {
    if (
      !currentJobId ||
      !currentFile ||
      selectedSegments.length === 0 ||
      isTranslating
    ) {
      return;
    }

    await createRosettaTranslationRevision(
      currentJobId,
      currentFile.id,
      "selection-retranslation",
      selectedBlockIds
    ).then(setActiveBundle);
    const preparedSegments = preparePreviewSegmentRetranslation(selectedSegmentIds);
    await saveRosettaSegments(currentJobId, preparedSegments).then(setActiveBundle);
    setSelectedRevisionId("current");
    setSelectedBlockIds([]);
    await translateSegments(selectedSegments, "selection");
  }

  async function exportCurrentFile(kind: RosettaExportKind) {
    if (!currentJobId || !currentFile || !isCurrentFileExportable) {
      return;
    }

    try {
      const targetPath = await pickRosettaExportPath(
        defaultExportFilename(currentFile.relativePath, currentFile.format, kind),
        currentFile.format
      );

      if (!targetPath) {
        return;
      }

      await exportRosettaJobFile(currentJobId, currentFile.id, kind, targetPath);
      setSelectedBlockIds([]);
      setActiveBundle(await loadRosettaJob(currentJobId));
    } catch (error) {
      console.error(error);
    }
  }

  async function updateLanguages(nextSourceLang: string, nextTargetLang: string) {
    if (!currentJobId || isTranslating || isSavingLanguages) {
      return;
    }

    setIsSavingLanguages(true);
    setSelectedBlockIds([]);
    setSelectedRevisionId("current");

    try {
      const bundle = await updateRosettaJobLanguages(
        currentJobId,
        nextSourceLang,
        nextTargetLang
      );
      setActiveBundle(bundle);
    } catch (error) {
      console.error(error);
    } finally {
      setIsSavingLanguages(false);
    }
  }

  async function deleteCurrentFile() {
    if (!currentJobId || !activeJob) {
      return;
    }

    const confirmed = window.confirm(
      currentFile
        ? activeJob.fileCount <= 1
          ? `删除当前文件“${currentFile.relativePath}”？这会移除整个项目。`
          : `删除当前文件“${currentFile.relativePath}”？`
        : `删除当前文件所在项目“${activeJob.filename}”？`
    );
    if (!confirmed) {
      return;
    }

    setIsDeleting(true);

    try {
      if (!currentFile) {
        return;
      }

      const result = await deleteRosettaJobFile(currentJobId, currentFile.id);
      setJobList(result.jobs);

      if (result.deletedJob) {
        clearActiveJob();
        navigate(result.jobs[0] ? `/jobs/${result.jobs[0].id}` : "/new");
        return;
      }

      setSelectedBlockIds([]);
      setSelectedRevisionId("current");
      if (result.bundle) {
        setActiveBundle(result.bundle);
      }
    } catch (error) {
      console.error(error);
    } finally {
      setIsDeleting(false);
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
    <section className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-4 px-6 py-5">
      <Card className="min-h-0 gap-0 py-0">
        <CardHeader className="border-b py-4">
          <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-center">
            <div className="flex min-w-0 gap-3">
              <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                <FileText />
              </div>
              <div className="min-w-0">
                <div className="mb-1 flex flex-wrap items-center gap-2">
                  <span className="text-sm text-muted-foreground">当前文件</span>
                  {currentFile ? (
                    <Badge variant="outline">{currentFile.format}</Badge>
                  ) : null}
                  <Badge
                    variant={
                      failedSegments.length > 0 ? "destructive" : "secondary"
                    }
                  >
                    {currentFileState}
                  </Badge>
                </div>
                <CardTitle className="truncate text-lg">
                  {currentFile?.relativePath ?? "加载文件中"}
                </CardTitle>
                <CardDescription className="mt-2 flex flex-wrap items-center gap-2">
                  <span>
                    {translatedFileSegments} / {translatableFileSegments.length} 段已翻译
                  </span>
                  <span>{fileProgress}%</span>
                  {pendingSegments.length > 0 ? (
                    <Badge variant="outline">{pendingSegments.length} 段待翻译</Badge>
                  ) : null}
                  {failedSegments.length > 0 ? (
                    <Badge variant="destructive">
                      {failedSegments.length} 段失败
                    </Badge>
                  ) : null}
                  {skippedFileSegments > 0 ? (
                    <Badge variant="outline">{skippedFileSegments} 段跳过</Badge>
                  ) : null}
                </CardDescription>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2 rounded-lg  xl:justify-end">
              <div className="flex items-center gap-1 rounded-md border bg-background p-1">
                <LanguageSelect
                  ariaLabel="选择原文语言"
                  disabled={isLoading || isTranslating || isSavingLanguages}
                  onValueChange={(value) => void updateLanguages(value, targetLang)}
                  value={sourceLang}
                />
                <ArrowRight className="text-muted-foreground" />
                <LanguageSelect
                  ariaLabel="选择目标译文语言"
                  disabled={isLoading || isTranslating || isSavingLanguages}
                  onValueChange={(value) => void updateLanguages(sourceLang, value)}
                  value={targetLang}
                />
              </div>
              {isSavingLanguages ? (
                <span className="text-xs text-muted-foreground">保存中</span>
              ) : null}
              {hasSelectedBlocks ? (
                <span className="px-1 text-sm text-muted-foreground">
                  已选 {selectedBlockIds.length} 段
                </span>
              ) : null}
              <Button
                disabled={mainActionDisabled}
                onClick={() =>
                  void (hasSelectedBlocks
                    ? translateSelectedBlocks()
                    : translateCurrentFile())
                }
                title={
                  rwkvConfigReady
                    ? hasSelectedBlocks
                      ? "重新翻译选中的段落"
                      : pendingSegments.length > 0
                        ? "翻译当前文件的待处理或失败段落"
                        : "重新翻译当前文件并保留旧译文历史"
                    : "请先在设置页完成 RWKV API 配置"
                }
                type="button"
                variant={mainActionVariant}
              >
                {hasSelectedBlocks || failedSegments.length > 0 ? (
                  <RefreshCw data-icon="inline-start" />
                ) : (
                  <Play data-icon="inline-start" />
                )}
                {mainActionLabel}
              </Button>
              {hasSelectedBlocks ? (
                <Button
                  disabled={isTranslating || isLoading}
                  onClick={() => setSelectedBlockIds([])}
                  size="icon"
                  title="取消段落选择"
                  type="button"
                  variant="ghost"
                >
                  <X />
                </Button>
              ) : null}
            </div>
          </div>
        </CardHeader>

        <CardContent className="grid gap-3 py-3">
          <div className="flex items-center gap-3">
            <div className="h-2 min-w-24 flex-1 overflow-hidden rounded-sm bg-muted">
              <div
                className={cn(
                  "h-full bg-primary transition-[width]",
                  isCurrentFileExportable && !isCurrentFileTranslationRunActive
                    ? "opacity-100"
                    : "opacity-80"
                )}
                style={{ width: `${fileProgress}%` }}
              />
            </div>
            {isCurrentFileExportable && !isCurrentFileTranslationRunActive ? (
              <Badge variant="secondary">
                <CheckCircle2 data-icon="inline-start" />
                完成
              </Badge>
            ) : null}
            <span
              className={cn(
                "shrink-0 text-xs",
                isCurrentFileExportable && !isCurrentFileTranslationRunActive
                  ? "font-medium text-foreground"
                  : "text-muted-foreground"
              )}
            >
              {fileProgressText}
            </span>
          </div>

          <div className="flex flex-wrap items-center justify-end gap-2">
              <Button
                disabled={!isCurrentFileExportable || isLoading || isTranslating}
                onClick={() => void exportCurrentFile("translation")}
                size="sm"
                title={
                  isCurrentFileExportable
                    ? "导出当前文件译文"
                    : "当前文件翻译完成后才能导出"
                }
                type="button"
                variant={isCurrentFileExportable ? "default" : "outline"}
              >
                <Download data-icon="inline-start" />
                导出译文
              </Button>
              <Button
                disabled={!isCurrentFileExportable || isLoading || isTranslating}
                onClick={() => void exportCurrentFile("bilingual")}
                size="sm"
                title={
                  isCurrentFileExportable
                    ? "导出当前文件双语对照"
                    : "当前文件翻译完成后才能导出"
                }
                type="button"
                variant="outline"
              >
                <Languages data-icon="inline-start" />
                导出双语
              </Button>
              <Button
                disabled={!activeJob || isDeleting || isTranslating || !currentFile}
                onClick={() => void deleteCurrentFile()}
                size="sm"
                type="button"
                variant="destructive"
              >
                <Trash2 data-icon="inline-start" />
                删除当前文件
              </Button>
          </div>
        </CardContent>
      </Card>

      <DocumentPreview
        currentFileId={selectedFileId}
        currentJobId={currentJobId}
        onRevisionChange={(revisionId) => {
          setSelectedRevisionId(revisionId);
          setSelectedBlockIds([]);
        }}
        onToggleBlockSelection={(blockId) => {
          if (!selectionEnabled) {
            return;
          }
          setSelectedBlockIds((blockIds) =>
            blockIds.includes(blockId)
              ? blockIds.filter((candidate) => candidate !== blockId)
              : [...blockIds, blockId]
          );
        }}
        selectedBlockIds={selectedBlockIds}
        selectedRevisionId={selectedRevisionId}
        selectionEnabled={selectionEnabled}
        translationRevisions={translationRevisions}
      />
    </section>
  );
}

function LanguageSelect({
  ariaLabel,
  disabled,
  onValueChange,
  value,
}: {
  ariaLabel: string;
  disabled: boolean;
  onValueChange: (value: string) => void;
  value: string;
}) {
  return (
    <Select disabled={disabled} onValueChange={onValueChange} value={value}>
      <SelectTrigger aria-label={ariaLabel} size="sm">
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

function chunkSegments(segments: Segment[], size: number) {
  const chunks: Segment[][] = [];
  for (let index = 0; index < segments.length; index += size) {
    chunks.push(segments.slice(index, index + size));
  }
  return chunks;
}

function defaultExportFilename(
  relativePath: string,
  format: "txt" | "markdown",
  kind: RosettaExportKind
) {
  const extension = format === "markdown" ? "md" : "txt";
  const filename = relativePath.split(/[\\/]/).pop() ?? relativePath;
  const baseName = filename.replace(/\.(txt|md|markdown)$/i, "");
  const suffix = kind === "bilingual" ? "bilingual" : "zh";
  return `${baseName}.${suffix}.${extension}`;
}
