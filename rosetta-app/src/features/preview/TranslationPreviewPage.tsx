import { useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { getCurrentWindow, type Theme } from "@tauri-apps/api/window";
import { Download, Languages, RefreshCw } from "lucide-react";

import { DocumentPreview } from "./DocumentPreview";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  exportRosettaTranslationFile,
  loadRosettaJob,
  loadRosettaTranslationFile,
  pickRosettaExportPath,
  saveRosettaTranslationSegments,
} from "@/lib/rosettaJobs";
import { translateRwkvTextsWithApi } from "@/lib/rwkvApi";
import { cn } from "@/lib/utils";
import { useRosettaStore } from "@/store/useRosettaStore";
import type {
  AppThemeMode,
  RosettaExportKind,
  RosettaJobBundle,
  RosettaSourceDocumentFormat,
  RosettaTranslationFileBundle,
  TranslationSegment,
} from "@/types/rosetta";

const appWindow = getCurrentWindow();
const BATCH_SIZE = 16;

export function TranslationPreviewPage() {
  const { jobId, translationFileId } = useParams();
  const themeMode = useRosettaStore((state) => state.themeMode);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const [systemPrefersDark, setSystemPrefersDark] = useState(true);
  const [jobBundle, setJobBundle] = useState<RosettaJobBundle | null>(null);
  const [translationBundle, setTranslationBundle] =
    useState<RosettaTranslationFileBundle | null>(null);
  const [hoveredBlockId, setHoveredBlockId] = useState<string | null>(null);
  const [selectedBlockIds, setSelectedBlockIds] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isRetranslating, setIsRetranslating] = useState(false);
  const [error, setError] = useState<string | null>(null);
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
  const rwkvConfigReady =
    rwkv.baseUrl.trim().length > 0 &&
    rwkv.endpoint.trim().length > 0 &&
    rwkv.internalToken.trim().length > 0 &&
    rwkv.bodyPassword.trim().length > 0 &&
    rwkv.timeoutMs > 0;
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

    const targets = selectedSourceSegments.sort(
      (left, right) => left.order - right.order
    );
    const targetIds = new Set(targets.map((segment) => segment.id));
    let workingSegments = markTranslationSegmentsTranslating(
      translationBundle.segments,
      targetIds
    );

    setIsRetranslating(true);
    setTranslationBundle({
      translationFile,
      segments: workingSegments,
    });

    try {
      for (const batch of chunkSegments(targets, BATCH_SIZE)) {
        const result = await translateRwkvTextsWithApi({
          baseUrl: rwkv.baseUrl,
          endpoint: rwkv.endpoint,
          internalToken: rwkv.internalToken,
          bodyPassword: rwkv.bodyPassword,
          timeoutMs: rwkv.timeoutMs,
          sourceLang: sourceFile.sourceLang ?? jobBundle.document.sourceLang,
          targetLang: translationFile.targetLang,
          sourceTexts: batch.map((segment) => segment.sourceText),
        });
        const batchIds = batch.map((segment) => segment.id);

        if (!result.ok || result.translations.length !== batch.length) {
          const message = !result.ok
            ? result.message
            : `RWKV API 返回 ${result.translations.length} 条译文，但本批有 ${batch.length} 条文本。`;
          workingSegments = markTranslationSegmentsFailed(
            workingSegments,
            batchIds,
            message
          );
          setTranslationBundle(
            await saveRosettaTranslationSegments(
              jobId,
              translationFile.id,
              workingSegments
            )
          );
          return;
        }

        workingSegments = markTranslationSegmentsDone(
          workingSegments,
          batchIds,
          result.translations
        );
        setTranslationBundle(
          await saveRosettaTranslationSegments(
            jobId,
            translationFile.id,
            workingSegments
          )
        );
      }
      setSelectedBlockIds([]);
    } catch (retranslateError) {
      setError(
        retranslateError instanceof Error
          ? retranslateError.message
          : "选中段落重翻失败。"
      );
    } finally {
      setIsRetranslating(false);
    }
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
            disabled={!canRetranslate}
            onClick={() => void retranslateSelectedBlocks()}
            size="sm"
            type="button"
            variant="outline"
          >
            <RefreshCw data-icon="inline-start" />
            {isRetranslating
              ? "重翻中"
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

      <main className="min-h-0 flex-1 p-4 bg-[#f3f1e9] dark:bg-stone-900">
        {isLoading ? (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
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

function markTranslationSegmentsTranslating(
  segments: TranslationSegment[],
  sourceSegmentIds: Set<string>
) {
  return segments.map((segment) =>
    sourceSegmentIds.has(segment.sourceSegmentId)
      ? {
          ...segment,
          translatedText: undefined,
          status: "translating" as const,
          error: undefined,
        }
      : segment
  );
}

function markTranslationSegmentsDone(
  segments: TranslationSegment[],
  sourceSegmentIds: string[],
  translations: string[]
) {
  const translationById = new Map(
    sourceSegmentIds.map((segmentId, index) => [segmentId, translations[index]])
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

function markTranslationSegmentsFailed(
  segments: TranslationSegment[],
  sourceSegmentIds: string[],
  error: string
) {
  const segmentIds = new Set(sourceSegmentIds);
  return segments.map((segment) =>
    segmentIds.has(segment.sourceSegmentId)
      ? {
          ...segment,
          status: "failed" as const,
          error,
        }
      : segment
  );
}

function chunkSegments<T>(segments: T[], size: number) {
  const chunks: T[][] = [];
  for (let index = 0; index < segments.length; index += size) {
    chunks.push(segments.slice(index, index + size));
  }
  return chunks;
}

function resolveIsDark(themeMode: AppThemeMode, systemPrefersDark: boolean) {
  return themeMode === "system" ? systemPrefersDark : themeMode === "dark";
}

function defaultExportFilename(
  relativePath: string,
  format: RosettaSourceDocumentFormat,
  targetLang: string,
  kind: RosettaExportKind
) {
  const extension = format === "markdown" ? "md" : "txt";
  const filename = relativePath.split(/[\\/]/).pop() ?? relativePath;
  const baseName = filename.replace(/\.(txt|md|markdown|pdf)$/i, "");
  const suffix = kind === "bilingual" ? `${targetLang}.bilingual` : targetLang;
  return `${baseName}.${suffix}.${extension}`;
}

function exportFormatForSource(
  format: RosettaSourceDocumentFormat
): "txt" | "markdown" {
  return format === "markdown" ? "markdown" : "txt";
}
