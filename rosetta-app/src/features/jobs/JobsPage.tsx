import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  Download,
  FilePlus,
  FileText,
  Languages,
  Play,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { DocumentPreview } from "../preview/DocumentPreview";
import { useRosettaStore } from "../../store/useRosettaStore";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
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
import { Separator } from "@/components/ui/separator";
import { translateRwkvTextsWithApi } from "../../lib/rwkvApi";
import {
  deleteRosettaJobFile,
  exportRosettaJob,
  exportRosettaJobToDirectory,
  loadRosettaJob,
  pickRosettaExportDirectory,
  pickRosettaExportPath,
  saveRosettaSegments,
  updateRosettaJobLanguages,
} from "../../lib/rosettaJobs";
import type {
  RosettaExportKind,
  RosettaExportResult,
  RwkvTranslationApiTranslateResult,
  Segment,
} from "../../types/rosetta";

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
  const [isLoading, setIsLoading] = useState(false);
  const [isTranslating, setIsTranslating] = useState(false);
  const [isSavingLanguages, setIsSavingLanguages] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [translationResult, setTranslationResult] =
    useState<RwkvTranslationApiTranslateResult | null>(null);
  const [exportResult, setExportResult] = useState<RosettaExportResult | null>(
    null
  );
  const [pageError, setPageError] = useState<string | null>(null);
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
  const failedSegments = useMemo(
    () =>
      currentFileSegments.filter(
        (segment) => segment.status === "failed" && segment.sourceText.trim()
      ),
    [currentFileSegments]
  );
  const incompleteSegments = currentFileSegments.filter((segment) =>
    ["pending", "failed", "translating"].includes(segment.status)
  ).length;
  const rwkvConfigReady =
    rwkv.baseUrl.trim().length > 0 &&
    rwkv.endpoint.trim().length > 0 &&
    rwkv.internalToken.trim().length > 0 &&
    rwkv.bodyPassword.trim().length > 0 &&
    rwkv.timeoutMs > 0;
  const isDirectoryProject =
    activeJob != null &&
    (activeJob.sourceKind === "directory" || activeJob.fileCount > 1);

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
    setPageError(null);
    void loadRosettaJob(currentJobId)
      .then(setActiveBundle)
      .catch((error) => {
        setPageError(
          error instanceof Error ? error.message : "无法加载这个项目。"
        );
      })
      .finally(() => setIsLoading(false));
  }, [activeDocument, activeJobId, currentJobId, setActiveBundle]);

  async function translateSegments(targetSegments: Segment[]) {
    if (
      !currentJobId ||
      !rwkvConfigReady ||
      targetSegments.length === 0 ||
      isTranslating
    ) {
      return;
    }

    setIsTranslating(true);
    setPageError(null);
    setExportResult(null);
    setTranslationResult(null);

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

        setTranslationResult(result);

        if (!result.ok) {
          const failed = failPreviewSegmentTranslation(
            currentBatchSegmentIds,
            result.message
          );
          await saveRosettaSegments(currentJobId, failed).then(setActiveBundle);
          setPageError(result.message);
          return;
        }

        if (result.translations.length !== batch.length) {
          const message = `RWKV API 返回 ${result.translations.length} 条译文，但本批有 ${batch.length} 条文本。`;
          const failed = failPreviewSegmentTranslation(
            currentBatchSegmentIds,
            message
          );
          await saveRosettaSegments(currentJobId, failed).then(setActiveBundle);
          setPageError(message);
          return;
        }

        const completed = completePreviewSegmentTranslation(
          currentBatchSegmentIds,
          result.translations
        );
        await saveRosettaSegments(currentJobId, completed).then(setActiveBundle);
      }
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "RWKV API 翻译调用失败。";
      setPageError(message);
      if (currentBatchSegmentIds.length > 0) {
        const failed = failPreviewSegmentTranslation(currentBatchSegmentIds, message);
        try {
          await saveRosettaSegments(currentJobId, failed).then(setActiveBundle);
        } catch {
          // Ignore secondary save errors so the original failure can surface.
        }
      }
    } finally {
      setIsTranslating(false);
    }
  }

  async function exportCurrentJob(kind: RosettaExportKind) {
    if (!currentJobId || !activeJob) {
      return;
    }

    setPageError(null);
    setExportResult(null);

    try {
      const targetPath = isDirectoryProject
        ? await pickRosettaExportDirectory()
        : await pickRosettaExportPath(
            defaultExportFilename(activeJob.filename, activeJob.format, kind),
            activeJob.format
          );

      if (!targetPath) {
        return;
      }

      const result = isDirectoryProject
        ? await exportRosettaJobToDirectory(currentJobId, kind, targetPath)
        : await exportRosettaJob(currentJobId, kind, targetPath);
      setExportResult(result);
      setActiveBundle(await loadRosettaJob(currentJobId));
    } catch (error) {
      setPageError(error instanceof Error ? error.message : "导出失败。");
    }
  }

  async function updateLanguages(nextSourceLang: string, nextTargetLang: string) {
    if (!currentJobId || isTranslating || isSavingLanguages) {
      return;
    }

    setIsSavingLanguages(true);
    setPageError(null);
    setTranslationResult(null);
    setExportResult(null);

    try {
      const bundle = await updateRosettaJobLanguages(
        currentJobId,
        nextSourceLang,
        nextTargetLang
      );
      setActiveBundle(bundle);
    } catch (error) {
      setPageError(
        error instanceof Error ? error.message : "无法保存语言设置。"
      );
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
    setPageError(null);

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

      if (result.bundle) {
        setActiveBundle(result.bundle);
      }
    } catch (error) {
      setPageError(error instanceof Error ? error.message : "删除文件失败。");
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
    <section className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-6 px-6 py-6">
      <Card className="min-h-0 py-0">
        <CardHeader className="border-b py-4">
          <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div className="min-w-0 flex-1">
              <Breadcrumb>
                <BreadcrumbList className="text-xs">
                  <BreadcrumbItem>
                    <BreadcrumbPage>{activeJob?.filename ?? "任务"}</BreadcrumbPage>
                  </BreadcrumbItem>
                  {currentFile ? (
                    <>
                      <BreadcrumbSeparator />
                      <BreadcrumbItem>
                        <BreadcrumbPage>{currentFile.relativePath}</BreadcrumbPage>
                      </BreadcrumbItem>
                    </>
                  ) : null}
                </BreadcrumbList>
              </Breadcrumb>

              <CardTitle className="truncate text-xl">
                {activeJob?.filename ?? "加载项目中"}
              </CardTitle>

              <CardDescription className="flex flex-wrap items-center gap-2 pt-2">
                {activeJob ? <Badge variant="outline">{activeJob.format}</Badge> : null}
                {activeJob?.sourceKind === "directory" ? (
                  <Badge variant="outline">{activeJob.fileCount} 个文件</Badge>
                ) : null}
                {activeJob ? <Badge variant="secondary">{activeJob.status}</Badge> : null}
                {incompleteSegments > 0 ? (
                  <Badge variant="outline">包含未完成段落</Badge>
                ) : null}
                <span className="text-muted-foreground">
                  {activeJob
                    ? `${activeJob.completedSegments} / ${activeJob.segmentCount} 已完成，${activeJob.failedSegments} 失败`
                    : "正在读取本机项目缓存。"}
                </span>
              </CardDescription>
            </div>

            <div className="flex flex-wrap items-center justify-end gap-2">
              <Button
                disabled={
                  !rwkvConfigReady ||
                  pendingSegments.length === 0 ||
                  isTranslating ||
                  isLoading
                }
                onClick={() => void translateSegments(pendingSegments)}
                size="sm"
                title={
                  rwkvConfigReady
                    ? "翻译当前文件的待处理或失败段落"
                    : "请先在设置页填写 RWKV API token 和 body password"
                }
                type="button"
              >
                <Play data-icon="inline-start" />
                {isTranslating ? "翻译中" : `翻译当前文件 ${pendingSegments.length}`}
              </Button>
              <Button
                disabled={failedSegments.length === 0 || isTranslating || isLoading}
                onClick={() => void translateSegments(failedSegments)}
                size="sm"
                title="重试失败段落"
                type="button"
                variant="outline"
              >
                <RefreshCw data-icon="inline-start" />
                重试失败
              </Button>
              <Button
                disabled={!activeJob || isLoading}
                onClick={() => void exportCurrentJob("translation")}
                size="sm"
                type="button"
                variant="outline"
              >
                <Download data-icon="inline-start" />
                {isDirectoryProject ? "导出项目译文" : "导出译文"}
              </Button>
              <Button
                disabled={!activeJob || isLoading}
                onClick={() => void exportCurrentJob("bilingual")}
                size="sm"
                type="button"
                variant="outline"
              >
                <Languages data-icon="inline-start" />
                {isDirectoryProject ? "导出项目双语" : "导出双语"}
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
          </div>
        </CardHeader>

        <CardContent className="grid gap-4 py-4">
          <div className="grid gap-3 rounded-xl border bg-muted/30 p-4">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="secondary">当前文件</Badge>
              <span className="truncate font-medium text-foreground">
                {currentFile?.relativePath ?? "未选择文件"}
              </span>
            </div>

            <div className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
              <FileText className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0 truncate">
                {currentFile
                  ? `${currentFileSegments.filter((segment) => segment.status === "done").length} / ${currentFileSegments.length} 已完成`
                  : "没有可显示的文件。"}
              </span>
              {incompleteSegments > 0 ? <Badge variant="outline">含未完成段落</Badge> : null}
            </div>

            {activeJob ? (
              <div className="flex flex-wrap items-center gap-2">
                <LanguageSelect
                  disabled={isLoading || isTranslating || isSavingLanguages}
                  label="原文语言"
                  onValueChange={(value) => void updateLanguages(value, targetLang)}
                  value={sourceLang}
                />
                <span className="text-sm text-muted-foreground">→</span>
                <LanguageSelect
                  disabled={isLoading || isTranslating || isSavingLanguages}
                  label="译文语言"
                  onValueChange={(value) => void updateLanguages(sourceLang, value)}
                  value={targetLang}
                />
                {isSavingLanguages ? (
                  <span className="text-xs text-muted-foreground">保存中</span>
                ) : null}
              </div>
            ) : null}
          </div>

          {translationResult || exportResult || pageError ? <Separator /> : null}

          {translationResult || exportResult || pageError ? (
            <div className="flex flex-col gap-2 text-sm">
              {translationResult ? (
                <div className="flex flex-wrap items-center gap-2 text-muted-foreground">
                  <Badge variant={translationResult.ok ? "secondary" : "outline"}>
                    {translationResult.ok ? "翻译批次成功" : "翻译批次失败"}
                  </Badge>
                  <span>{translationResult.message}</span>
                  <span>status: {translationResult.statusCode ?? "none"}</span>
                  <span>latency: {translationResult.latencyMs} ms</span>
                </div>
              ) : null}
              {exportResult ? (
                <div className="flex flex-wrap items-center gap-2 text-muted-foreground">
                  <Badge variant="secondary">导出完成</Badge>
                  <span>{exportResult.targetPath}</span>
                  <span>{exportResult.filesWritten} files</span>
                  <span>{exportResult.bytesWritten} bytes</span>
                </div>
              ) : null}
              {pageError ? <p className="text-destructive">{pageError}</p> : null}
            </div>
          ) : null}
        </CardContent>
      </Card>

      <DocumentPreview currentFileId={selectedFileId} currentJobId={currentJobId} />
    </section>
  );
}

function LanguageSelect({
  disabled,
  label,
  onValueChange,
  value,
}: {
  disabled: boolean;
  label: string;
  onValueChange: (value: string) => void;
  value: string;
}) {
  return (
    <label className="flex items-center gap-2 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <Select disabled={disabled} onValueChange={onValueChange} value={value}>
        <SelectTrigger size="sm">
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
    </label>
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
  filename: string,
  format: "txt" | "markdown",
  kind: RosettaExportKind
) {
  const extension = format === "markdown" ? "md" : "txt";
  const baseName = filename.replace(/\.(txt|md|markdown)$/i, "");
  const suffix = kind === "bilingual" ? "bilingual" : "zh";
  return `${baseName}.${suffix}.${extension}`;
}
