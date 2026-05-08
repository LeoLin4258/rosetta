import { useMemo, useState } from "react";
import { Play, RefreshCw } from "lucide-react";
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
import type { RwkvTranslationApiTranslateResult } from "../../types/rosetta";

export function JobsPage() {
  const jobs = useRosettaStore((state) => state.jobs);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const previewSegments = useRosettaStore((state) => state.previewSegments);
  const beginPreviewSegmentTranslation = useRosettaStore(
    (state) => state.beginPreviewSegmentTranslation
  );
  const completePreviewSegmentTranslation = useRosettaStore(
    (state) => state.completePreviewSegmentTranslation
  );
  const failPreviewSegmentTranslation = useRosettaStore(
    (state) => state.failPreviewSegmentTranslation
  );
  const createDemoJob = useRosettaStore((state) => state.createDemoJob);
  const [isTranslating, setIsTranslating] = useState(false);
  const [translationResult, setTranslationResult] =
    useState<RwkvTranslationApiTranslateResult | null>(null);
  const [translationError, setTranslationError] = useState<string | null>(null);
  const translatableSegments = useMemo(
    () =>
      previewSegments.filter(
        (segment) =>
          ["pending", "failed"].includes(segment.status) &&
          segment.sourceText.trim().length > 0
      ),
    [previewSegments]
  );
  const rwkvConfigReady =
    rwkv.baseUrl.trim().length > 0 &&
    rwkv.endpoint.trim().length > 0 &&
    rwkv.internalToken.trim().length > 0 &&
    rwkv.bodyPassword.trim().length > 0 &&
    rwkv.timeoutMs > 0;
  const canTranslate =
    rwkvConfigReady && translatableSegments.length > 0 && !isTranslating;

  async function translatePendingSegments() {
    const targetSegments = translatableSegments;
    if (!canTranslate || targetSegments.length === 0) {
      return;
    }

    const segmentIds = targetSegments.map((segment) => segment.id);

    setIsTranslating(true);
    setTranslationError(null);
    setTranslationResult(null);
    beginPreviewSegmentTranslation(segmentIds);

    try {
      const result = await translateRwkvTextsWithApi({
        baseUrl: rwkv.baseUrl,
        endpoint: rwkv.endpoint,
        internalToken: rwkv.internalToken,
        bodyPassword: rwkv.bodyPassword,
        timeoutMs: rwkv.timeoutMs,
        sourceTexts: targetSegments.map((segment) => segment.sourceText),
      });

      setTranslationResult(result);

      if (!result.ok) {
        failPreviewSegmentTranslation(segmentIds);
        setTranslationError(result.message);
        return;
      }

      if (result.translations.length !== targetSegments.length) {
        failPreviewSegmentTranslation(segmentIds);
        setTranslationError(
          `RWKV API 返回 ${result.translations.length} 条译文，但本次请求有 ${targetSegments.length} 条文本。`
        );
        return;
      }

      completePreviewSegmentTranslation(segmentIds, result.translations);
    } catch (error) {
      failPreviewSegmentTranslation(segmentIds);
      setTranslationError(
        error instanceof Error ? error.message : "RWKV API 翻译调用失败。"
      );
    } finally {
      setIsTranslating(false);
    }
  }

  return (
    <section className="grid min-h-full grid-rows-[auto_1fr] gap-6 px-6 py-6">
      <div className="overflow-hidden rounded-lg border bg-card">
        <div className="flex items-start justify-between gap-4 border-b px-4 py-3">
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="font-medium">翻译任务</span>
              <Badge variant="outline">RWKV API</Badge>
            </div>
            <p className="text-sm text-muted-foreground">
              当前使用已配置的 /v1/chat/completions 非流式 batch API。
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button
              disabled={isTranslating}
              onClick={() => {
                createDemoJob();
                setTranslationResult(null);
                setTranslationError(null);
              }}
              title="重置演示任务"
              type="button"
              variant="outline"
            >
              <RefreshCw data-icon="inline-start" />
              重置演示
            </Button>
            <Button
              disabled={!canTranslate}
              onClick={() => void translatePendingSegments()}
              title={
                rwkvConfigReady
                  ? "翻译待处理段落"
                  : "请先在设置页填写 RWKV API token 和 body password"
              }
              type="button"
            >
              <Play data-icon="inline-start" />
              {isTranslating
                ? "翻译中"
                : `翻译待处理 ${translatableSegments.length}`}
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
              <TableRow key={job.id}>
                <TableCell className="px-4 font-medium">{job.filename}</TableCell>
                <TableCell>
                  <Badge variant="secondary">{job.status}</Badge>
                </TableCell>
                <TableCell>
                  {job.completedSegments} / {job.segmentCount}
                </TableCell>
                <TableCell>{job.failedSegments}</TableCell>
                <TableCell className="text-muted-foreground">
                  {new Date(job.updatedAt).toLocaleString()}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        {translationResult || translationError ? (
          <div className="flex flex-col gap-2 border-t px-4 py-3 text-sm">
            {translationResult ? (
              <div className="flex flex-wrap items-center gap-2 text-muted-foreground">
                <Badge variant={translationResult.ok ? "secondary" : "outline"}>
                  {translationResult.ok ? "成功" : "失败"}
                </Badge>
                <span>{translationResult.message}</span>
                <span>status: {translationResult.statusCode ?? "none"}</span>
                <span>latency: {translationResult.latencyMs} ms</span>
              </div>
            ) : null}
            {translationError ? (
              <p className="text-destructive">{translationError}</p>
            ) : null}
          </div>
        ) : null}
      </div>

      <SegmentPreviewList />
    </section>
  );
}
