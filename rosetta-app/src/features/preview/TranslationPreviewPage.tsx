import { useEffect, useMemo, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { getCurrentWindow, type Theme } from "@tauri-apps/api/window";
import {
  AlertCircle,
  CheckCircle2,
  Clock3,
  Download,
  Languages,
  LoaderCircle,
  RefreshCw,
  Square,
} from "lucide-react";

import { DocumentPreview } from "./DocumentPreview";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  createRosettaTranslationRevision,
  exportRosettaTranslationFile,
  loadRosettaJob,
  loadRosettaTranslationFile,
  pickRosettaExportPath,
} from "@/lib/rosettaJobs";
import { defaultExportFilename, exportFormatForSource } from "@/lib/rosettaExport";
import { isRwkvConfigReady } from "@/lib/languages";
import { selectProvider } from "@/lib/providers";
import {
  isManagedRuntimeReady,
  useManagedRwkvRuntime,
} from "@/lib/useManagedRwkvRuntime";
import {
  translationProgressPercent,
} from "@/lib/translationSegments";
import {
  runTranslationBatches,
  translationTargetsForStatuses,
} from "@/lib/translationRunner";
import { cn } from "@/lib/utils";
import { useRosettaStore } from "@/store/useRosettaStore";
import type {
  AppThemeMode,
  RosettaExportKind,
  RosettaJobBundle,
  RosettaTranslationFile,
  RosettaTranslationFileBundle,
} from "@/types/rosetta";

const appWindow = getCurrentWindow();
const BATCH_SIZE = 16;
const LIVE_REFRESH_INTERVAL_MS = 1_000;

export function TranslationPreviewPage() {
  const { jobId, translationFileId } = useParams();
  const themeMode = useRosettaStore((state) => state.themeMode);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const managedRuntime = useManagedRwkvRuntime();
  const managedRuntimeStatus = managedRuntime.status;
  const [systemPrefersDark, setSystemPrefersDark] = useState(true);
  const [jobBundle, setJobBundle] = useState<RosettaJobBundle | null>(null);
  const [translationBundle, setTranslationBundle] =
    useState<RosettaTranslationFileBundle | null>(null);
  const [hoveredBlockId, setHoveredBlockId] = useState<string | null>(null);
  const [selectedBlockIds, setSelectedBlockIds] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isRetranslating, setIsRetranslating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const retranslationCancelRef = useRef<(() => void) | null>(null);
  const isDark = resolveIsDark(themeMode, systemPrefersDark);

  const translationFile = translationBundle?.translationFile ?? null;
  const sourceFile =
    jobBundle?.document.files.find(
      (file) => file.id === translationFile?.sourceFileId
    ) ?? null;
  const sourceSegments = useMemo(
    () =>
      jobBundle?.segments.filter(
        (segment) => (segment.fileId ?? "file-1") === (sourceFile?.id ?? "")
      ) ?? [],
    [jobBundle?.segments, sourceFile?.id]
  );
  const selectedSourceSegments = useMemo(() => {
    const blockIds = new Set(selectedBlockIds);
    return sourceSegments.filter(
      (segment) =>
        blockIds.has(segment.blockId) && segment.sourceText.trim().length > 0
    );
  }, [selectedBlockIds, sourceSegments]);
  const canExport =
    translationFile != null &&
    translationFile.segmentCount > 0 &&
    translationFile.completedSegments >= translationFile.segmentCount &&
    translationFile.failedSegments === 0;
  const rwkvConfigReady = isRwkvConfigReady(
    rwkv,
    isManagedRuntimeReady(managedRuntimeStatus)
  );
  const canRetranslate =
    jobId != null &&
    jobBundle != null &&
    sourceFile != null &&
    translationFile != null &&
    translationBundle != null &&
    selectedSourceSegments.length > 0 &&
    rwkvConfigReady &&
    !isRetranslating;

  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    function syncSystemTheme() {
      setSystemPrefersDark(mediaQuery.matches);
    }

    syncSystemTheme();
    mediaQuery.addEventListener("change", syncSystemTheme);

    return () => mediaQuery.removeEventListener("change", syncSystemTheme);
  }, []);

  useEffect(() => {
    const windowTheme: Theme | null = themeMode === "system" ? null : themeMode;

    void appWindow.setTheme(windowTheme).catch(() => {
      // Plain browser dev mode does not expose the Tauri window API.
    });
  }, [themeMode]);

  useEffect(() => {
    if (!jobId || !translationFileId) {
      setError("缺少预览参数。");
      setIsLoading(false);
      return;
    }

    let cancelled = false;
    setIsLoading(true);
    setError(null);

    void Promise.all([
      loadRosettaJob(jobId),
      loadRosettaTranslationFile(jobId, translationFileId),
    ])
      .then(([loadedJob, loadedTranslation]) => {
        if (cancelled) {
          return;
        }
        setJobBundle(loadedJob);
        setTranslationBundle(loadedTranslation);
        setSelectedBlockIds([]);
        setHoveredBlockId(null);
        const source = loadedJob.document.files.find(
          (file) => file.id === loadedTranslation.translationFile.sourceFileId
        );
        void appWindow
          .setTitle(
            `${source?.relativePath ?? source?.filename ?? "译文"} · ${
              loadedTranslation.translationFile.targetLang
            }`
          )
          .catch(() => {});
      })
      .catch((loadError) => {
        if (cancelled) {
          return;
        }
        setError(
          loadError instanceof Error
            ? loadError.message
            : "译文预览加载失败。"
        );
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [jobId, translationFileId]);

  useEffect(() => {
    if (!jobId || !translationFileId || isRetranslating) {
      return;
    }

    const currentJobId = jobId;
    const currentTranslationFileId = translationFileId;
    let cancelled = false;

    async function refreshTranslation() {
      try {
        const nextBundle = await loadRosettaTranslationFile(
          currentJobId,
          currentTranslationFileId
        );
        if (!cancelled) {
          setTranslationBundle(nextBundle);
        }
      } catch {
        // Keep the current preview visible when a transient refresh fails.
      }
    }

    const interval = window.setInterval(() => {
      void refreshTranslation();
    }, LIVE_REFRESH_INTERVAL_MS);

    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [isRetranslating, jobId, translationFileId]);

  async function exportTranslation(kind: RosettaExportKind) {
    if (!jobId || !translationFile || !sourceFile) {
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
    await exportRosettaTranslationFile(jobId, translationFile.id, kind, targetPath);
    setTranslationBundle(await loadRosettaTranslationFile(jobId, translationFile.id));
  }

  function toggleBlockSelection(blockId: string) {
    setSelectedBlockIds((current) =>
      current.includes(blockId)
        ? current.filter((id) => id !== blockId)
        : [...current, blockId]
    );
  }

  async function retranslateSelectedBlocks() {
    if (
      !canRetranslate ||
      !jobId ||
      !jobBundle ||
      !sourceFile ||
      !translationFile ||
      !translationBundle
    ) {
      return;
    }

    const targets = translationTargetsForStatuses({
      sourceSegments: selectedSourceSegments,
      translationSegments: translationBundle.segments,
      statuses: "all",
    });
    if (targets.length === 0) {
      setError("选中的段落没有可重翻的文本。");
      return;
    }

    let cancelCurrentRun: (() => void) | null = null;
    const cancelled = new Promise<"stopped">((resolve) => {
      cancelCurrentRun = () => resolve("stopped");
    });

    setIsRetranslating(true);
    retranslationCancelRef.current = cancelCurrentRun;

    try {
      const revisionBundle = await createRosettaTranslationRevision(
        jobId,
        sourceFile.id,
        "selection-retranslation",
        selectedBlockIds
      );
      setJobBundle(revisionBundle);

      const result = await runTranslationBatches({
        batchSize: BATCH_SIZE,
        cancelPromise: cancelled,
        jobId,
        onTranslationFileSaved: setTranslationBundle,
        provider: selectProvider({
          config: rwkv,
          override:
            rwkv.providerPreference === "local"
              ? managedRuntimeStatus?.profile?.providerId
              : "rwkv-lightning-contents",
          managedRuntimeReady: isManagedRuntimeReady(managedRuntimeStatus),
          managedRuntimeBaseUrl:
            managedRuntimeStatus?.process.baseUrl ?? undefined,
          managedRuntimeProviderId: managedRuntimeStatus?.profile?.providerId,
        }),
        request: {
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          providerPreference: rwkv.providerPreference,
          sourceLang: sourceFile.sourceLang ?? jobBundle.document.sourceLang,
          targetLang: translationFile.targetLang,
        },
        targets,
        translationFile,
      });

      if (result === "completed") {
        setSelectedBlockIds([]);
      }
    } catch (retranslateError) {
      setError(
        retranslateError instanceof Error
          ? retranslateError.message
          : "选中段落重翻失败。"
      );
    } finally {
      setIsRetranslating(false);
      if (retranslationCancelRef.current === cancelCurrentRun) {
        retranslationCancelRef.current = null;
      }
    }
  }

  function stopRetranslation() {
    retranslationCancelRef.current?.();
  }

  return (
    <div
      className={cn(
        "flex h-screen flex-col bg-background text-foreground",
        isDark && "dark"
      )}
    >
      <header className="flex h-14 shrink-0 items-center justify-between border-b px-4 bg-[#f3f1e9] dark:bg-stone-900">
        <div className="min-w-0">
          <h1 className="truncate text-base font-semibold">
            {sourceFile?.relativePath ?? "译文预览"}
          </h1>
          <div className="mt-0.5 flex items-center gap-2 text-sm text-muted-foreground">
            {translationFile ? (
              <>
                <Badge variant="outline">{translationFile.targetLang}</Badge>
                <span>
                  {translationFile.completedSegments}/{translationFile.segmentCount} 段
                </span>
              </>
            ) : null}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button
            disabled={!canRetranslate && !isRetranslating}
            onClick={() =>
              isRetranslating
                ? stopRetranslation()
                : void retranslateSelectedBlocks()
            }
            size="sm"
            type="button"
            variant="outline"
          >
            {isRetranslating ? (
              <Square data-icon="inline-start" />
            ) : (
              <RefreshCw data-icon="inline-start" />
            )}
            {isRetranslating
              ? "停止"
              : selectedSourceSegments.length > 0
                ? `重翻 ${selectedBlockIds.length} 段`
                : "重翻选中"}
          </Button>
          <Button
            disabled={!canExport}
            onClick={() => void exportTranslation("translation")}
            size="sm"
            type="button"
            variant="outline"
          >
            <Download data-icon="inline-start" />
            导出译文
          </Button>
          <Button
            disabled={!canExport}
            onClick={() => void exportTranslation("bilingual")}
            size="sm"
            type="button"
            variant="outline"
          >
            <Languages data-icon="inline-start" />
            导出双语
          </Button>
        </div>
      </header>
      <TranslationStatusBar translationFile={translationFile} />

      <main className="min-h-0 flex-1 p-4 bg-[#f3f1e9] dark:bg-stone-900">
        {isLoading ? (
          <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
            <LoaderCircle className="size-4 animate-spin" />
            正在加载译文预览...
          </div>
        ) : error ? (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            {error}
          </div>
        ) : (
          <DocumentPreview
            document={jobBundle?.document ?? null}
            hoveredBlockId={hoveredBlockId}
            onBlockHover={setHoveredBlockId}
            onBlockLeave={() => setHoveredBlockId(null)}
            onToggleBlockSelection={toggleBlockSelection}
            selectedBlockIds={selectedBlockIds}
            selectionEnabled={!isRetranslating}
            sourceFile={sourceFile}
            sourceSegments={sourceSegments}
            translationFile={translationFile}
            translationSegments={translationBundle?.segments ?? []}
          />
        )}
      </main>
    </div>
  );
}

function TranslationStatusBar({
  translationFile,
}: {
  translationFile: RosettaTranslationFile | null;
}) {
  if (!translationFile) {
    return (
      <div className="flex h-10 shrink-0 items-center border-b bg-background px-4 text-sm text-muted-foreground">
        等待译文状态
      </div>
    );
  }

  const progressPercent = translationProgressPercent(translationFile);
  const remainingSegments = Math.max(
    translationFile.segmentCount -
      translationFile.completedSegments -
      translationFile.failedSegments,
    0
  );

  return (
    <div className="flex h-10 shrink-0 items-center justify-between gap-4 border-b bg-background px-4">
      <div className="flex min-w-0 items-center gap-3">
        <TranslationStateBadge state={translationFile.status} />
        <div className="flex items-center gap-3 text-sm text-muted-foreground">
          <span className="tabular-nums">
            {translationFile.completedSegments}/{translationFile.segmentCount} 段
          </span>
          {translationFile.failedSegments > 0 ? (
            <span className="tabular-nums">
              失败 {translationFile.failedSegments}
            </span>
          ) : null}
          {translationFile.status === "translating" ? (
            <span className="tabular-nums">剩余 {remainingSegments}</span>
          ) : null}
        </div>
      </div>
      <div className="flex min-w-0 flex-1 items-center justify-end gap-3">
        <div className="h-1.5 w-48 overflow-hidden rounded-full bg-muted">
          <div
            className={cn(
              "h-full rounded-full",
              translationFile.status === "failed"
                ? "bg-destructive"
                : translationFile.status === "translated"
                  ? "bg-primary"
                  : "bg-primary/80"
            )}
            style={{ width: `${progressPercent}%` }}
          />
        </div>
        <span className="w-10 text-right text-xs tabular-nums text-muted-foreground">
          {progressPercent}%
        </span>
      </div>
    </div>
  );
}

function TranslationStateBadge({
  state,
}: {
  state: RosettaTranslationFile["status"];
}) {
  if (state === "translated") {
    return (
      <Badge variant="outline">
        <CheckCircle2 data-icon="inline-start" />
        已完成
      </Badge>
    );
  }
  if (state === "failed") {
    return (
      <Badge variant="destructive">
        <AlertCircle data-icon="inline-start" />
        翻译失败
      </Badge>
    );
  }
  if (state === "translating") {
    return (
      <Badge variant="outline">
        <LoaderCircle className="animate-spin" data-icon="inline-start" />
        翻译中
      </Badge>
    );
  }
  return (
    <Badge className="text-muted-foreground" variant="outline">
      <Clock3 data-icon="inline-start" />
      待翻译
    </Badge>
  );
}

function resolveIsDark(themeMode: AppThemeMode, systemPrefersDark: boolean) {
  return themeMode === "system" ? systemPrefersDark : themeMode === "dark";
}
