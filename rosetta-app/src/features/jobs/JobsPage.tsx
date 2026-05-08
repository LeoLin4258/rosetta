import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  Download,
  FilePlus,
  Languages,
  Play,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { SegmentPreviewList } from "../preview/SegmentPreviewList";
import { useRosettaStore } from "../../store/useRosettaStore";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { translateRwkvTextsWithApi } from "../../lib/rwkvApi";
import {
  deleteRosettaJob,
  exportRosettaJob,
  loadRosettaJob,
  pickRosettaExportPath,
  saveRosettaSegments,
} from "../../lib/rosettaJobs";
import type {
  RosettaExportKind,
  RosettaExportResult,
  RwkvTranslationApiTranslateResult,
  Segment,
} from "../../types/rosetta";

const BATCH_SIZE = 16;

export function JobsPage() {
  const { jobId } = useParams();
  const navigate = useNavigate();
  const jobs = useRosettaStore((state) => state.jobs);
  const activeJobId = useRosettaStore((state) => state.activeJobId);
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
  const [isDeleting, setIsDeleting] = useState(false);
  const [translationResult, setTranslationResult] =
    useState<RwkvTranslationApiTranslateResult | null>(null);
  const [exportResult, setExportResult] = useState<RosettaExportResult | null>(
    null
  );
  const [pageError, setPageError] = useState<string | null>(null);
  const currentJobId = jobId ?? activeJobId ?? jobs[0]?.id ?? null;
  const activeJob = jobs.find((job) => job.id === currentJobId) ?? null;
  const pendingSegments = useMemo(
    () =>
      previewSegments.filter(
        (segment) =>
          ["pending", "failed"].includes(segment.status) &&
          segment.sourceText.trim().length > 0
      ),
    [previewSegments]
  );
  const failedSegments = useMemo(
    () =>
      previewSegments.filter(
        (segment) => segment.status === "failed" && segment.sourceText.trim()
      ),
    [previewSegments]
  );
  const incompleteSegments = previewSegments.filter((segment) =>
    ["pending", "failed", "translating"].includes(segment.status)
  ).length;
  const rwkvConfigReady =
    rwkv.baseUrl.trim().length > 0 &&
    rwkv.endpoint.trim().length > 0 &&
    rwkv.internalToken.trim().length > 0 &&
    rwkv.bodyPassword.trim().length > 0 &&
    rwkv.timeoutMs > 0;

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

    try {
      const orderedSegments = [...targetSegments].sort(
        (left, right) => left.order - right.order
      );

      for (const batch of chunkSegments(orderedSegments, BATCH_SIZE)) {
        const segmentIds = batch.map((segment) => segment.id);
        beginPreviewSegmentTranslation(segmentIds);

        const result = await translateRwkvTextsWithApi({
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          sourceTexts: batch.map((segment) => segment.sourceText),
        });

        setTranslationResult(result);

        if (!result.ok) {
          const failed = failPreviewSegmentTranslation(segmentIds, result.message);
          await saveRosettaSegments(currentJobId, failed).then(setActiveBundle);
          setPageError(result.message);
          return;
        }

        if (result.translations.length !== batch.length) {
          const message = `RWKV API 返回 ${result.translations.length} 条译文，但本批有 ${batch.length} 条文本。`;
          const failed = failPreviewSegmentTranslation(segmentIds, message);
          await saveRosettaSegments(currentJobId, failed).then(setActiveBundle);
          setPageError(message);
          return;
        }

        const completed = completePreviewSegmentTranslation(
          segmentIds,
          result.translations
        );
        await saveRosettaSegments(currentJobId, completed).then(setActiveBundle);
      }
    } catch (error) {
      setPageError(
        error instanceof Error ? error.message : "RWKV API 翻译调用失败。"
      );
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
      const targetPath = await pickRosettaExportPath(
        defaultExportFilename(activeJob.filename, activeJob.format, kind),
        activeJob.format
      );

      if (!targetPath) {
        return;
      }

      const result = await exportRosettaJob(currentJobId, kind, targetPath);
      setExportResult(result);
      setActiveBundle(await loadRosettaJob(currentJobId));
    } catch (error) {
      setPageError(error instanceof Error ? error.message : "导出失败。");
    }
  }

  async function deleteCurrentJob() {
    if (!currentJobId || !activeJob) {
      return;
    }

    const confirmed = window.confirm(`删除项目“${activeJob.filename}”？`);
    if (!confirmed) {
      return;
    }

    setIsDeleting(true);
    setPageError(null);

    try {
      const nextJobs = await deleteRosettaJob(currentJobId);
      setJobList(nextJobs);
      clearActiveJob();
      navigate(nextJobs[0] ? `/jobs/${nextJobs[0].id}` : "/new");
    } catch (error) {
      setPageError(error instanceof Error ? error.message : "删除项目失败。");
    } finally {
      setIsDeleting(false);
    }
  }

  if (jobs.length === 0) {
    return (
      <section className="mx-auto flex max-w-3xl flex-col gap-4 px-6 py-10">
        <div className="rounded-lg border bg-card p-8 text-center">
          <h2 className="text-lg font-semibold">还没有项目</h2>
          <p className="mt-2 text-sm text-muted-foreground">
            导入一个 TXT 或 Markdown 文件开始翻译。
          </p>
          <Button asChild className="mt-5" type="button">
            <Link to="/new">
              <FilePlus data-icon="inline-start" />
              新项目
            </Link>
          </Button>
        </div>
      </section>
    );
  }

  return (
    <section className="grid min-h-full grid-rows-[auto_1fr] gap-6 px-6 py-6">
      <div className="overflow-hidden rounded-lg border bg-card">
        <div className="flex items-start justify-between gap-4 border-b px-4 py-3">
          <div className="flex flex-col gap-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium">
                {activeJob?.filename ?? "加载项目中"}
              </span>
              {activeJob ? <Badge variant="outline">{activeJob.format}</Badge> : null}
              {activeJob ? <Badge variant="secondary">{activeJob.status}</Badge> : null}
              {incompleteSegments > 0 ? (
                <Badge variant="outline">包含未完成段落</Badge>
              ) : null}
            </div>
            <p className="text-sm text-muted-foreground">
              {activeJob
                ? `${activeJob.completedSegments} / ${activeJob.segmentCount} 已完成，${activeJob.failedSegments} 失败`
                : "正在读取本机项目缓存。"}
            </p>
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
              title={
                rwkvConfigReady
                  ? "翻译全部待处理或失败段落"
                  : "请先在设置页填写 RWKV API token 和 body password"
              }
              type="button"
            >
              <Play data-icon="inline-start" />
              {isTranslating ? "翻译中" : `翻译全部 ${pendingSegments.length}`}
            </Button>
            <Button
              disabled={failedSegments.length === 0 || isTranslating || isLoading}
              onClick={() => void translateSegments(failedSegments)}
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
              type="button"
              variant="outline"
            >
              <Download data-icon="inline-start" />
              导出译文
            </Button>
            <Button
              disabled={!activeJob || isLoading}
              onClick={() => void exportCurrentJob("bilingual")}
              type="button"
              variant="outline"
            >
              <Languages data-icon="inline-start" />
              导出双语
            </Button>
            <Button
              disabled={!activeJob || isDeleting || isTranslating}
              onClick={() => void deleteCurrentJob()}
              type="button"
              variant="outline"
            >
              <Trash2 data-icon="inline-start" />
              删除
            </Button>
          </div>
        </div>

        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="px-4">文件</TableHead>
              <TableHead>状态</TableHead>
              <TableHead>进度</TableHead>
              <TableHead>失败</TableHead>
              <TableHead>更新时间</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {jobs.map((job) => (
              <TableRow
                className="cursor-pointer"
                key={job.id}
                onClick={() => navigate(`/jobs/${job.id}`)}
              >
                <TableCell className="px-4 font-medium">{job.filename}</TableCell>
                <TableCell>
                  <Badge variant="secondary">{job.status}</Badge>
                </TableCell>
                <TableCell>
                  {job.completedSegments} / {job.segmentCount}
                </TableCell>
                <TableCell>{job.failedSegments}</TableCell>
                <TableCell className="text-muted-foreground">
                  {formatTimestamp(job.updatedAt)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>

        {translationResult || exportResult || pageError ? (
          <div className="flex flex-col gap-2 border-t px-4 py-3 text-sm">
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
                <span>{exportResult.bytesWritten} bytes</span>
              </div>
            ) : null}
            {pageError ? <p className="text-destructive">{pageError}</p> : null}
          </div>
        ) : null}
      </div>

      <SegmentPreviewList />
    </section>
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

function formatTimestamp(value: string) {
  const numeric = Number(value);
  const date = Number.isFinite(numeric) ? new Date(numeric) : new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString();
}
