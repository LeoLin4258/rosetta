import { useEffect, useRef, useState } from "react";
import { Download, Loader2, Play, RefreshCw, Square, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { RosettaJobSummary, RosettaTranslationFile } from "@/types/rosetta";

const TARGET_LANGS = [
  { value: "zh-CN", label: "简体中文" },
  { value: "en", label: "英文" },
];

const SOURCE_LANGS = [
  { value: "zh-CN", label: "简体中文" },
  { value: "en", label: "英文" },
];

type WorkspaceTopbarProps = {
  job: RosettaJobSummary;
  activeTranslationFile: RosettaTranslationFile | null;
  isTranslating: boolean;
  isTranslationBusyElsewhere?: boolean;
  isRuntimeStarting: boolean;
  isPdfEngineInstalling?: boolean;
  pdfEngineProgressMessage?: string | null;
  translatedCount: number;
  totalCount: number;
  /// Epoch ms when the active run started. Anchors the elapsed timer so it
  /// survives unmount/remount (file switches) during a long run.
  runStartedAtMs?: number | null;
  pdfProgress?: {
    phase: string;
    percent: number | null;
    currentPage: number | null;
    totalPages: number | null;
    translatedChars?: number | null;
  } | null;
  sourceLang: string;
  targetLang: string;
  selectedBlockCount: number;
  pdfSelectedPageCount?: number;
  pdfPageCount?: number;
  pdfForceRetranslate?: boolean;
  onPdfForceRetranslateChange?: (force: boolean) => void;
  onSelectAllPages?: () => void;
  onDeselectAllPages?: () => void;
  onSourceLangChange: (lang: string) => void;
  onTargetLangChange: (lang: string) => void;
  onTranslate: (targetLang: string, sourceLang: string) => void;
  onCancelTranslation: () => void;
  onExport: (kind: "translation" | "bilingual") => void;
  onRetranslateSelected: () => void;
  onClearSelection: () => void;
  onRetranslateAll: () => void;
};

/// Map the backend's `phase` enum to a user-facing label. `warmup` is the
/// new phase emitted before pdf2zh.py actually starts writing stdout —
/// covers shim launch, role-set HTTP, and pdf2zh subprocess spawn. Without
/// it the UI used to sit silently on "翻译中" for the whole startup gap,
/// which is the biggest contributor to the "feels frozen" perception.
const PDF_PHASE_LABELS: Record<string, string> = {
  warmup: "准备翻译引擎",
  parse: "解析版面",
  translate: "翻译中",
  render: "生成 PDF",
};

/// Format milliseconds as `mm:ss`. Used by the topbar's "翻译中 · 00:23"
/// elapsed timer — even when pdf2zh.py is silent for tens of seconds (Python
/// multiprocessing pool startup, first MLX batch's prefill, etc.), this
/// counter keeps moving so the UI never looks frozen.
function formatElapsed(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`;
}

/// Hook: track elapsed ms while `isActive`. Anchored to `startedAtMs` (the
/// run's persisted start timestamp) when available, so remounting this
/// component mid-run — e.g. switching files and coming back — doesn't reset
/// the counter to 00:00. Falls back to mount time when no anchor is given.
function useElapsedSince(isActive: boolean, startedAtMs?: number | null): number {
  const fallbackStartRef = useRef<number | null>(null);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    if (!isActive) {
      fallbackStartRef.current = null;
      setElapsed(0);
      return;
    }
    const anchor = startedAtMs ?? (fallbackStartRef.current ??= Date.now());
    setElapsed(Math.max(0, Date.now() - anchor));
    const interval = setInterval(() => {
      setElapsed(Math.max(0, Date.now() - anchor));
    }, 1000);
    return () => clearInterval(interval);
  }, [isActive, startedAtMs]);

  return elapsed;
}

export function WorkspaceTopbar({
  job,
  activeTranslationFile,
  isTranslating,
  isTranslationBusyElsewhere = false,
  isRuntimeStarting,
  isPdfEngineInstalling = false,
  pdfEngineProgressMessage = null,
  translatedCount,
  totalCount,
  runStartedAtMs = null,
  pdfProgress = null,
  sourceLang,
  targetLang,
  selectedBlockCount,
  pdfSelectedPageCount = 0,
  pdfPageCount = 0,
  pdfForceRetranslate = false,
  onPdfForceRetranslateChange,
  onSelectAllPages,
  onDeselectAllPages,
  onSourceLangChange,
  onTargetLangChange,
  onTranslate,
  onCancelTranslation,
  onExport,
  onRetranslateSelected,
  onClearSelection,
  onRetranslateAll,
}: WorkspaceTopbarProps) {
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const [confirmingRetranslateAll, setConfirmingRetranslateAll] = useState(false);
  // Elapsed timer for the "翻译中 · 00:23" display. Starts the moment
  // `isTranslating` flips true (= user clicked translate) and stops when it
  // flips false. Independent of whether pdf2zh has emitted any progress
  // event yet — the whole point is to keep moving during the silent gap.
  const elapsedMs = useElapsedSince(isTranslating, runStartedAtMs);
  const elapsedLabel = formatElapsed(elapsedMs);

  const hasTranslation =
    activeTranslationFile &&
    (job.format === "pdf" ||
      activeTranslationFile.completedSegments > 0);
  const allTranslated =
    !!activeTranslationFile &&
    (job.format === "pdf"
      ? activeTranslationFile.status === "translated"
      : activeTranslationFile.segmentCount > 0 &&
        activeTranslationFile.completedSegments >= activeTranslationFile.segmentCount);
  const progressPercent =
    totalCount > 0 ? Math.round((translatedCount / totalCount) * 100) : 0;
  const isPdf = job.format === "pdf";
  const sameLanguage = sourceLang === targetLang;
  const noPdfPagesSelected = isPdf && pdfSelectedPageCount === 0;
  const translateDisabled =
    sameLanguage || noPdfPagesSelected || isTranslationBusyElsewhere;
  const translateTitle = sameLanguage
    ? "原文与译文语言不能相同"
    : isTranslationBusyElsewhere
      ? "另一个文件正在翻译"
    : noPdfPagesSelected
      ? "请选择页面"
      : undefined;
  const selectedPdfLabel =
    isPdf && pdfPageCount > 0 && pdfSelectedPageCount === pdfPageCount
      ? "全部"
      : "所选页";

  return (
    <div className="flex items-center justify-between border-b border-border/40 px-6 py-2.5">
      {isPdf ? (
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium text-foreground">页面</span>
          <span className="text-xs text-muted-foreground">
            已选 {pdfSelectedPageCount} / {pdfPageCount} 页
          </span>
          <Button
            size="sm"
            variant="ghost"
            className="h-7 px-2 text-xs"
            onClick={onSelectAllPages}
            disabled={isTranslating}
          >
            全选
          </Button>
          <Button
            size="sm"
            variant="ghost"
            className="h-7 px-2 text-xs"
            onClick={onDeselectAllPages}
            disabled={isTranslating}
          >
            取消选择
          </Button>
          <label className="flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground">
            <input
              type="checkbox"
              checked={pdfForceRetranslate}
              onChange={(e) => onPdfForceRetranslateChange?.(e.target.checked)}
              disabled={isTranslating}
            />
            强制重翻
          </label>
        </div>
      ) : (
        <div />
      )}
      <div className="flex shrink-0 items-center gap-2">
        {isTranslating ? (
          <>
            <Loader2 className="size-3.5 animate-spin text-muted-foreground/50" />
            <span className="text-xs tabular-nums text-muted-foreground/60">
              {isPdf ? (
                <>
                  {/*
                    PDF layout: "[phase label] · 第 X/Y 页 · 已翻译 N 字 · 00:23 · 45%"
                    Sections are separated by " · " and any of them can be
                    absent. Page numbers track pdf2zh's live tqdm output; the
                    character counter comes from the RWKV shim and updates as
                    each batch returns, so the bar visibly moves even between
                    page boundaries. Before the first progress event lands we
                    show 准备翻译引擎 instead of the misleading segment-count
                    fallback (PDF runs aren't segment-based). The elapsed
                    timer always shows because we tick locally.
                  */}
                  {pdfProgress
                    ? PDF_PHASE_LABELS[pdfProgress.phase] ?? pdfProgress.phase
                    : PDF_PHASE_LABELS.warmup}
                  {pdfProgress?.currentPage != null &&
                    pdfProgress?.totalPages != null &&
                    ` · 第 ${pdfProgress.currentPage}/${pdfProgress.totalPages} 页`}
                  {pdfProgress?.translatedChars
                    ? ` · 已翻译 ${pdfProgress.translatedChars.toLocaleString()} 字`
                    : ""}
                  {" · "}
                  {elapsedLabel}
                  {pdfProgress?.percent != null ? ` · ${pdfProgress.percent}%` : ""}
                </>
              ) : (
                <>
                  {translatedCount} / {totalCount} · {progressPercent}% · {elapsedLabel}
                </>
              )}
            </span>
            {confirmingCancel ? (
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground/60">确认取消？</span>
                <button
                  type="button"
                  onClick={() => {
                    onCancelTranslation();
                    setConfirmingCancel(false);
                  }}
                  className="text-xs text-destructive/70 transition-colors hover:text-destructive"
                >
                  确定
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmingCancel(false)}
                  className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground"
                >
                  继续
                </button>
              </div>
            ) : (
              <Button
                size="sm"
                variant="outline"
                onClick={() => setConfirmingCancel(true)}
                className="gap-1.5"
              >
                <Square className="size-3" /> 取消
              </Button>
            )}
          </>
        ) : (
          <>
            {hasTranslation && (
              <Button
                size="sm"
                variant="outline"
                onClick={() => onExport("translation")}
                className="gap-1.5"
              >
                <Download className="size-3.5" /> 导出译文
              </Button>
            )}

            {/* Source language */}
            <Select value={sourceLang} onValueChange={onSourceLangChange}>
              <SelectTrigger className="h-8 w-28 text-xs">
                <SelectValue placeholder="原文语言" />
              </SelectTrigger>
              <SelectContent>
                {SOURCE_LANGS.map((lang) => (
                  <SelectItem key={lang.value} value={lang.value} className="text-xs">
                    {lang.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* Target language + translate */}
            <Select value={targetLang} onValueChange={onTargetLangChange}>
              <SelectTrigger className="h-8 w-24 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TARGET_LANGS.map((lang) => (
                  <SelectItem key={lang.value} value={lang.value} className="text-xs">
                    {lang.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {isPdfEngineInstalling ? (
              <Button size="sm" disabled className="gap-1.5">
                <Loader2 className="size-3.5 animate-spin" />
                {pdfEngineProgressMessage ?? "正在准备 PDF 引擎…"}
              </Button>
            ) : isRuntimeStarting ? (
              <Button size="sm" disabled className="gap-1.5">
                <Loader2 className="size-3.5 animate-spin" />
                正在启动模型…
              </Button>
            ) : selectedBlockCount > 0 ? (
              <div className="flex items-center gap-1">
                <Button
                  size="sm"
                  disabled={translateDisabled}
                  onClick={onRetranslateSelected}
                  className="gap-1.5"
                  title={translateTitle}
                >
                  <RefreshCw className="size-3.5" />
                  重翻选中 {selectedBlockCount} 段
                </Button>
                <button
                  type="button"
                  onClick={onClearSelection}
                  className="flex size-5 items-center justify-center rounded text-muted-foreground/50 transition-colors hover:text-muted-foreground"
                  title="取消选中"
                >
                  <X className="size-3.5" />
                </button>
              </div>
            ) : allTranslated ? (
              confirmingRetranslateAll ? (
                <div className="flex items-center gap-2">
                  <span className="text-xs text-muted-foreground/60">
                    {isPdf ? `确认重翻${selectedPdfLabel}？` : "确认重翻全部？"}
                  </span>
                  <button
                    type="button"
                    onClick={() => {
                      if (isPdf) onTranslate(targetLang, sourceLang);
                      else onRetranslateAll();
                      setConfirmingRetranslateAll(false);
                    }}
                    className="text-xs text-destructive/70 transition-colors hover:text-destructive"
                  >
                    确定
                  </button>
                  <button
                    type="button"
                    onClick={() => setConfirmingRetranslateAll(false)}
                    className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground"
                  >
                    取消
                  </button>
                </div>
              ) : (
                <Button
                  size="sm"
                  disabled={translateDisabled}
                  onClick={() => setConfirmingRetranslateAll(true)}
                  className="gap-1.5"
                  title={translateTitle}
                >
                  <RefreshCw className="size-3.5" />
                  {isPdf ? `重翻${selectedPdfLabel}` : "重翻全部"}
                </Button>
              )
            ) : (
              <Button
                size="sm"
                disabled={translateDisabled}
                onClick={() => onTranslate(targetLang, sourceLang)}
                className="gap-1.5"
                title={translateTitle}
              >
                <Play className="size-3.5" />
                {isPdf ? `翻译${selectedPdfLabel}` : "翻译"}
              </Button>
            )}
          </>
        )}
      </div>
    </div>
  );
}
