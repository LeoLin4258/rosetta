import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ArrowRight,
  Download,
  FileText,
  Loader2,
  Play,
  RefreshCw,
  Square,
  Timer,
  Type,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
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
  isPausingTranslation?: boolean;
  isTranslationBusyElsewhere?: boolean;
  isRuntimeStarting: boolean;
  isRuntimeUnavailable?: boolean;
  runtimeUnavailableMessage?: string | null;
  isPdfEngineInstalling?: boolean;
  isPdfEngineUnavailable?: boolean;
  /// True while the persistent pdf2zh worker is paying its ~13 s torch
  /// import. Only meaningful for PDF jobs; disables the translate button so
  /// the user can't click before the engine is warm. The granular warmup
  /// progress is shown by the header badge, not here, to avoid duplication.
  isPdfEngineWarming?: boolean;
  pdfEngineProgressMessage?: string | null;
  pdfEngineUnavailableMessage?: string | null;
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
  split: "正在准备页面",
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

function TranslationRunIndicator({
  phaseLabel,
  pageLabel,
  countValue,
  countTitle,
  elapsedLabel,
}: {
  phaseLabel: string;
  pageLabel: string | null;
  countValue: ReactNode | null;
  countTitle: string;
  elapsedLabel: string;
}) {
  return (
    <div className="flex h-9 max-w-[min(38rem,64vw)] items-center gap-2 rounded-lg border border-border/70 bg-muted/30 px-2.5">
      <span className="relative flex size-2.5 shrink-0" aria-hidden="true">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-primary/25 motion-reduce:animate-none" />
        <span className="relative inline-flex size-2.5 rounded-full bg-primary/70" />
      </span>
      <span className="min-w-0 truncate !text-xs font-medium !text-foreground">
        {phaseLabel}
      </span>
      <div className="flex min-w-0 items-center gap-1.5 border-l border-border/70 pl-2">
        {pageLabel ? (
          <RunMetric title="当前页" icon={<FileText className="size-3" />}>
            {pageLabel}
          </RunMetric>
        ) : null}
        {countValue ? (
          <RunMetric title={countTitle} icon={<Type className="size-3" />}>
            {countValue}
          </RunMetric>
        ) : null}
        <RunMetric title="已用时间" icon={<Timer className="size-3" />}>
          <span className="rosetta-run-time-value">{elapsedLabel}</span>
        </RunMetric>
      </div>
    </div>
  );
}

function RunMetric({
  children,
  icon,
  title,
}: {
  children: ReactNode;
  icon: ReactNode;
  title: string;
}) {
  return (
    <span
      className="flex min-w-0 items-center justify-center gap-1 rounded-md bg-background/70 px-1.5 py-0.5 !text-xs tabular-nums !text-muted-foreground"
      title={title}
    >
      <span className="shrink-0 !text-muted-foreground/70">{icon}</span>
      <span className="truncate flex items-center justify-center">{children}</span>
    </span>
  );
}

function RollingTranslatedChars({ value }: { value: number }) {
  const formatted = Math.max(0, Math.floor(value)).toLocaleString();
  const contentRef = useRef<HTMLSpanElement | null>(null);
  const [contentWidth, setContentWidth] = useState<number | null>(null);

  useLayoutEffect(() => {
    const nextWidth = contentRef.current?.getBoundingClientRect().width ?? null;
    if (nextWidth == null) {
      return;
    }
    setContentWidth((current) => {
      const rounded = Math.ceil(nextWidth);
      return current === rounded ? current : rounded;
    });
  }, [formatted]);

  return (
    <span
      aria-label={`${formatted} 字`}
      className="rosetta-run-count-value"
      style={contentWidth == null ? undefined : { width: contentWidth }}
    >
      <span
        className="rosetta-run-count-content"
        aria-hidden="true"
        ref={contentRef}
      >
        <span className="rosetta-run-count-number">
          {formatted.split("").map((char, index) =>
            /\d/.test(char) ? (
              <RollingDigit digit={Number(char)} key={`${index}:digit`} />
            ) : (
              <span className="rosetta-run-count-separator" key={`${index}:${char}`}>
                {char}
              </span>
            )
          )}
        </span>
        <span className="rosetta-run-count-unit">字</span>
      </span>
    </span>
  );
}

function RollingDigit({ digit }: { digit: number }) {
  const previousDigitRef = useRef(digit);
  const [previousDigit, setPreviousDigit] = useState<number | null>(null);

  useLayoutEffect(() => {
    if (previousDigitRef.current === digit) {
      return;
    }

    setPreviousDigit(previousDigitRef.current);
    previousDigitRef.current = digit;

    const timeout = window.setTimeout(() => {
      setPreviousDigit(null);
    }, 220);

    return () => window.clearTimeout(timeout);
  }, [digit]);

  return (
    <span
      className="rosetta-run-count-digit"
      data-rolling={previousDigit == null ? undefined : "true"}
    >
      {previousDigit == null ? null : (
        <span className="rosetta-run-count-digit-previous">
          {previousDigit}
        </span>
      )}
      <span className="rosetta-run-count-digit-current" key={digit}>
        {digit}
      </span>
    </span>
  );
}

export function WorkspaceTopbar({
  job,
  activeTranslationFile,
  isTranslating,
  isPausingTranslation = false,
  isTranslationBusyElsewhere = false,
  isRuntimeStarting,
  isRuntimeUnavailable = false,
  runtimeUnavailableMessage = null,
  isPdfEngineInstalling = false,
  isPdfEngineUnavailable = false,
  isPdfEngineWarming = false,
  pdfEngineProgressMessage = null,
  pdfEngineUnavailableMessage = null,
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
  const isPdf = job.format === "pdf";
  const sameLanguage = sourceLang === targetLang;
  const noPdfPagesSelected = isPdf && pdfSelectedPageCount === 0;
  const translateDisabled =
    sameLanguage ||
    noPdfPagesSelected ||
    isTranslationBusyElsewhere ||
    isRuntimeUnavailable ||
    (isPdf && isPdfEngineUnavailable) ||
    (isPdf && isPdfEngineWarming);
  const translateTitle = sameLanguage
    ? "原文与译文语言不能相同"
    : isTranslationBusyElsewhere
      ? "另一个文件正在翻译"
    : isRuntimeUnavailable
      ? (runtimeUnavailableMessage ?? "本地翻译模型尚未就绪")
    : isPdf && isPdfEngineUnavailable
      ? (pdfEngineUnavailableMessage ?? "PDF 组件未安装，请在设置中安装后再翻译。")
    : isPdf && isPdfEngineWarming
      ? "PDF 引擎预热中，请稍候"
    : noPdfPagesSelected
      ? "请选择页面"
      : undefined;
  const selectedPdfLabel =
    isPdf && pdfPageCount > 0 && pdfSelectedPageCount === pdfPageCount
      ? "全部"
      : "所选页";
  const pdfSelectionReady = isPdf && pdfSelectedPageCount > 0;
  const pageSelectionLabel =
    pdfPageCount > 0 ? `${pdfSelectedPageCount} / ${pdfPageCount} 页` : "等待页数";
  const runPhaseLabel = isPdf
    ? isPausingTranslation
      ? "正在停止"
      : pdfProgress
      ? PDF_PHASE_LABELS[pdfProgress.phase] ?? pdfProgress.phase
      : PDF_PHASE_LABELS.warmup
    : "翻译中";
  const runPageLabel =
    isPdf && pdfProgress?.currentPage != null && pdfProgress?.totalPages != null
      ? `${pdfProgress.currentPage}/${pdfProgress.totalPages} 页`
      : null;
  const runCountValue = isPdf
    ? pdfProgress?.translatedChars != null
      ? <RollingTranslatedChars value={pdfProgress.translatedChars} />
      : null
    : `${translatedCount}/${totalCount}`;

  return (
    <div className="border-b border-border/60 bg-background/95 px-5 py-3" data-window-no-drag>
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
          {isPdf ? (
            <div className="flex h-9 max-w-full items-center gap-2 rounded-lg border border-border/70 bg-muted/30 px-2.5">
              <span className="!text-xs font-medium !text-foreground">页面范围</span>
              <Badge
                variant={pdfSelectionReady ? "secondary" : "outline"}
                className="h-5 rounded-md px-1.5 font-normal tabular-nums"
              >
                {pageSelectionLabel}
              </Badge>
              <div className="flex items-center gap-1 border-l border-border/70 pl-1.5">
                <Button
                  size="xs"
                  variant="ghost"
                  className="h-6 px-1.5 !text-xs font-normal leading-none"
                  onClick={onSelectAllPages}
                  disabled={isTranslating || pdfSelectedPageCount === pdfPageCount}
                >
                  全选
                </Button>
                <Button
                  size="xs"
                  variant="ghost"
                  className="h-6 px-1.5 !text-xs font-normal leading-none"
                  onClick={onDeselectAllPages}
                  disabled={isTranslating || pdfSelectedPageCount === 0}
                >
                  清空
                </Button>
              </div>
              <label className="ml-1 flex h-6 cursor-pointer items-center gap-1.5 rounded-md px-1.5 !text-xs leading-none !text-muted-foreground transition-colors hover:bg-background/80 hover:!text-foreground has-disabled:cursor-not-allowed has-disabled:opacity-50">
                <input
                  type="checkbox"
                  checked={pdfForceRetranslate}
                  onChange={(e) => onPdfForceRetranslateChange?.(e.target.checked)}
                  disabled={isTranslating}
                  className="size-3 accent-primary"
                />
                强制重翻
              </label>
            </div>
          ) : selectedBlockCount > 0 ? (
            <div className="flex h-9 items-center gap-2 rounded-lg border border-border/70 bg-muted/30 px-2.5">
              <span className="!text-xs font-medium !text-foreground">已选段落</span>
              <Badge
                variant="secondary"
                className="h-5 rounded-md px-1.5 font-normal tabular-nums"
              >
                {selectedBlockCount} 段
              </Badge>
              <Button
                size="xs"
                variant="ghost"
                className="h-6 px-1.5 !text-xs font-normal leading-none"
                onClick={onClearSelection}
                disabled={isTranslating}
              >
                清空
              </Button>
            </div>
          ) : (
            <div className="flex h-9 items-center gap-2 rounded-lg border border-transparent px-2.5">
              <span className="!text-xs font-medium !text-foreground">整篇文档</span>
              <Badge variant="outline" className="h-5 rounded-md px-1.5 font-normal tabular-nums">
                {totalCount} 段
              </Badge>
            </div>
          )}
        </div>

        <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
          {isTranslating ? (
            <>
              <TranslationRunIndicator
                phaseLabel={runPhaseLabel}
                pageLabel={runPageLabel}
                countValue={runCountValue}
                countTitle={isPdf ? "已翻译字数" : "段落进度"}
                elapsedLabel={elapsedLabel}
              />
              {isPausingTranslation ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled
                  className="h-7 gap-1.5 px-2 !text-xs font-normal leading-none"
                >
                  <Loader2 className="size-3 animate-spin" />
                  正在停止
                </Button>
              ) : confirmingCancel ? (
                <div className="flex items-center gap-2">
                  <span className="!text-xs !text-muted-foreground/60">确认暂停？</span>
                  <button
                    type="button"
                    onClick={() => {
                      onCancelTranslation();
                      setConfirmingCancel(false);
                    }}
                    className="!text-xs !text-destructive/70 transition-colors hover:!text-destructive"
                  >
                    暂停
                  </button>
                  <button
                    type="button"
                    onClick={() => setConfirmingCancel(false)}
                    className="!text-xs !text-muted-foreground/40 transition-colors hover:!text-muted-foreground"
                  >
                    继续
                  </button>
                </div>
              ) : (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setConfirmingCancel(true)}
                  className="h-7 gap-1.5 px-2 !text-xs font-normal leading-none"
                >
                  <Square className="size-3" /> 暂停
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
                  className="h-7 gap-1.5 px-2 !text-xs font-normal leading-none"
                >
                  <Download className="size-3" /> 导出译文
                </Button>
              )}

              <div className="flex h-9 items-center gap-1 rounded-lg border border-border/70 bg-background px-1 shadow-xs">
                <Select value={sourceLang} onValueChange={onSourceLangChange}>
                  <SelectTrigger
                    aria-label="原文语言"
                    className="h-7 w-28 border-0 bg-transparent px-2 !text-xs shadow-none focus:ring-0"
                  >
                    <SelectValue placeholder="原文语言" />
                  </SelectTrigger>
                  <SelectContent>
                    {SOURCE_LANGS.map((lang) => (
                      <SelectItem key={lang.value} value={lang.value} className="!text-xs">
                        {lang.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <ArrowRight className="size-3.5 !text-muted-foreground/50" aria-hidden="true" />
                <Select value={targetLang} onValueChange={onTargetLangChange}>
                  <SelectTrigger
                    aria-label="译文语言"
                    className="h-7 w-28 border-0 bg-transparent px-2 !text-xs shadow-none focus:ring-0"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {TARGET_LANGS.map((lang) => (
                      <SelectItem key={lang.value} value={lang.value} className="!text-xs">
                        {lang.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              {isPdfEngineInstalling ? (
                <Button
                  size="sm"
                  disabled
                  className="h-7 gap-1.5 px-2 !text-xs font-normal leading-none"
                >
                  <Loader2 className="size-3 animate-spin" />
                  {pdfEngineProgressMessage ?? "正在准备 PDF 引擎…"}
                </Button>
              ) : isRuntimeStarting ? (
                <Button
                  size="sm"
                  disabled
                  className="h-7 gap-1.5 px-2 !text-xs font-normal leading-none"
                >
                  <Loader2 className="size-3 animate-spin" />
                  正在启动模型…
                </Button>
              ) : selectedBlockCount > 0 ? (
                <Button
                  size="sm"
                  disabled={translateDisabled}
                  onClick={onRetranslateSelected}
                  className="h-7 gap-1.5 px-2 !text-xs font-normal leading-none"
                  title={translateTitle}
                >
                  <RefreshCw className="size-3" />
                  重翻选中 {selectedBlockCount} 段
                </Button>
              ) : allTranslated ? (
                confirmingRetranslateAll ? (
                  <div className="flex items-center gap-2">
                    <span className="!text-xs !text-muted-foreground/60">
                      {isPdf ? `确认重翻${selectedPdfLabel}？` : "确认重翻全部？"}
                    </span>
                    <button
                      type="button"
                      onClick={() => {
                        if (translateDisabled) return;
                        if (isPdf) onRetranslateSelected();
                        else onRetranslateAll();
                        setConfirmingRetranslateAll(false);
                      }}
                      disabled={translateDisabled}
                      title={translateTitle}
                      className="!text-xs !text-destructive/70 transition-colors hover:!text-destructive disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:!text-destructive/70"
                    >
                      确定
                    </button>
                    <button
                      type="button"
                      onClick={() => setConfirmingRetranslateAll(false)}
                      className="!text-xs !text-muted-foreground/40 transition-colors hover:!text-muted-foreground"
                    >
                      取消
                    </button>
                  </div>
                ) : (
                  <Button
                    size="sm"
                    disabled={translateDisabled}
                    onClick={() => setConfirmingRetranslateAll(true)}
                    className="h-7 gap-1.5 px-2 !text-xs font-normal leading-none"
                    title={translateTitle}
                  >
                    <RefreshCw className="size-3" />
                    {isPdf ? `重翻${selectedPdfLabel}` : "重翻全部"}
                  </Button>
                )
              ) : (
                <Button
                  size="sm"
                  disabled={translateDisabled}
                  onClick={() => onTranslate(targetLang, sourceLang)}
                  className="h-7 gap-1.5 px-2 !text-xs font-normal leading-none"
                  title={translateTitle}
                >
                  <Play className="size-3" />
                  {isPdf ? `翻译${selectedPdfLabel}` : "翻译"}
                </Button>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
