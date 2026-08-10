import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import {
  ArrowRight,
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronUp,
  Download,
  FileText,
  Loader2,
  Play,
  RefreshCw,
  Square,
  Timer,
  Type,
} from "lucide-react";

import {
  AnimatedWidth,
  useMeasuredContentWidth,
} from "@/components/animated-width";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { pdfPageSelectionLabel } from "@/lib/pdfPageSelectionPolicy";
import type {
  PdfMarkdownComponentStatus,
  PdfMarkdownExtractionStatus,
  PdfMarkdownInstallProgress,
} from "@/lib/rosettaJobs";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type {
  RosettaJobSummary,
  RosettaTranslationFile,
  RosettaTranslationOutputFormat,
} from "@/types/rosetta";

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
  selectedOutputFormat: RosettaTranslationOutputFormat;
  pdfMarkdownComponentStatus?: PdfMarkdownComponentStatus | null;
  pdfMarkdownInstallProgress?: PdfMarkdownInstallProgress | null;
  pdfMarkdownExtractionStatus?: PdfMarkdownExtractionStatus | null;
  pdfMarkdownError?: string | null;
  isPdfMarkdownInstalling?: boolean;
  isPdfMarkdownStartingExtraction?: boolean;
  isTranslating: boolean;
  isPausingTranslation?: boolean;
  isTranslationBusyElsewhere?: boolean;
  isRuntimeStarting: boolean;
  isRuntimeUnavailable?: boolean;
  runtimeUnavailableMessage?: string | null;
  isPdfEngineInstalling?: boolean;
  isPdfEngineUnavailable?: boolean;
  /// True while the persistent pdf2zh worker is paying Python import and ONNX
  /// layout warmup. Only meaningful for PDF jobs; disables the translate button
  /// so the user can't click before the engine is warm. The granular warmup
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
    completedPages?: number | null;
    translatedChars?: number | null;
  } | null;
  sourceLang: string;
  targetLang: string;
  selectedBlockCount: number;
  pdfSelectedPageCount?: number;
  pdfSelectedPages?: number[];
  pdfPageCount?: number;
  pdfCurrentPage?: number;
  pdfForceRetranslate?: boolean;
  onPdfForceRetranslateChange?: (force: boolean) => void;
  onPdfNavigate?: (pageNumber: number) => void;
  onPdfSelectRange?: (range: string) => void;
  onSelectAllPages?: () => void;
  onDeselectAllPages?: () => void;
  onSourceLangChange: (lang: string) => void;
  onTargetLangChange: (lang: string) => void;
  onOutputFormatChange?: (format: RosettaTranslationOutputFormat) => void;
  onInstallPdfMarkdown?: () => void;
  onRepairPdfMarkdown?: () => void;
  onCancelPdfMarkdownInstall?: () => void;
  onStartPdfMarkdownExtraction?: () => void;
  onCancelPdfMarkdownExtraction?: () => void;
  onTranslate: (targetLang: string, sourceLang: string) => void;
  onCancelTranslation: () => void;
  onExport: (kind: "translation" | "bilingual") => void;
  onRetranslateSelected: () => void;
  onClearSelection: () => void;
  onRetranslateAll: () => void;
  onOpenRuntimeSettings?: () => void;
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

const topbarPanelClass =
  "flex h-8 max-w-full items-center gap-1.5 rounded-lg border border-border/60 bg-card/80 px-2 text-xs text-muted-foreground shadow-none";
const topbarButtonClass =
  "h-8 gap-1.5 rounded-lg px-2.5 !text-xs font-normal leading-none transition-[width,background-color,border-color,color,opacity,transform] duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]";
const topbarGhostButtonClass =
  "h-6 rounded-md px-1.5 !text-xs font-normal leading-none !text-muted-foreground hover:bg-muted/70 hover:!text-foreground";

function TopbarBadge({
  children,
  className,
  variant = "outline",
}: {
  children: ReactNode;
  className?: string;
  variant?: "default" | "secondary" | "destructive" | "outline" | "ghost" | "link";
}) {
  const { contentRef, widthStyle } = useMeasuredContentWidth<HTMLSpanElement>();

  return (
    <Badge
      variant={variant}
      className={cn(
        "h-5 overflow-hidden rounded-md px-0 font-normal tabular-nums transition-[width,background-color,border-color,color] duration-200 ease-[cubic-bezier(0.22,1,0.36,1)] motion-reduce:transition-none",
        variant === "secondary" && "bg-muted text-foreground",
        className,
      )}
      style={widthStyle}
    >
      <span
        ref={contentRef}
        className="flex w-max flex-none items-center justify-center px-1.5"
      >
        {children}
      </span>
    </Badge>
  );
}

function TopbarDivider() {
  return <span className="h-4 w-px shrink-0 bg-border/70" aria-hidden="true" />;
}

function TopbarConfirm({
  label,
  confirmLabel,
  cancelLabel = "取消",
  destructive = false,
  disabled = false,
  title,
  onConfirm,
  onCancel,
}: {
  label: string;
  confirmLabel: string;
  cancelLabel?: string;
  destructive?: boolean;
  disabled?: boolean;
  title?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div className={cn(topbarPanelClass, "gap-2 px-2.5")}>
      <span className="whitespace-nowrap !text-xs !text-muted-foreground">{label}</span>
      <button
        type="button"
        onClick={onConfirm}
        disabled={disabled}
        title={title}
        className={cn(
          "h-6 rounded-md px-1.5 !text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40",
          destructive
            ? "!text-destructive/75 hover:bg-destructive/10 hover:!text-destructive"
            : "!text-foreground hover:bg-muted/70",
        )}
      >
        {confirmLabel}
      </button>
      <button
        type="button"
        onClick={onCancel}
        className="h-6 rounded-md px-1.5 !text-xs !text-muted-foreground transition-colors hover:bg-muted/70 hover:!text-foreground"
      >
        {cancelLabel}
      </button>
    </div>
  );
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
    <div className={cn(topbarPanelClass, "max-w-[min(38rem,64vw)] gap-2 px-2.5")}>
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
      className="flex h-5 min-w-0 items-center justify-center gap-1 rounded-md bg-background/75 px-1.5 !text-xs tabular-nums !text-muted-foreground"
      title={title}
    >
      <span className="shrink-0 !text-muted-foreground/70">{icon}</span>
      <span className="flex items-center justify-center truncate">{children}</span>
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
  selectedOutputFormat,
  pdfMarkdownComponentStatus = null,
  pdfMarkdownInstallProgress = null,
  pdfMarkdownExtractionStatus = null,
  pdfMarkdownError = null,
  isPdfMarkdownInstalling = false,
  isPdfMarkdownStartingExtraction = false,
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
  pdfSelectedPages = [],
  pdfPageCount = 0,
  pdfCurrentPage = 1,
  pdfForceRetranslate = false,
  onPdfForceRetranslateChange,
  onPdfNavigate,
  onPdfSelectRange,
  onSelectAllPages,
  onDeselectAllPages,
  onSourceLangChange,
  onTargetLangChange,
  onOutputFormatChange,
  onInstallPdfMarkdown,
  onRepairPdfMarkdown,
  onCancelPdfMarkdownInstall,
  onStartPdfMarkdownExtraction,
  onCancelPdfMarkdownExtraction,
  onTranslate,
  onCancelTranslation,
  onExport,
  onRetranslateSelected,
  onClearSelection,
  onRetranslateAll,
  onOpenRuntimeSettings,
}: WorkspaceTopbarProps) {
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const [confirmingRetranslateAll, setConfirmingRetranslateAll] = useState(false);
  const [pageInput, setPageInput] = useState(String(pdfCurrentPage));
  const [rangeInput, setRangeInput] = useState("");
  const pageInputRef = useRef<HTMLInputElement | null>(null);
  // Elapsed timer for the "翻译中 · 00:23" display. Starts the moment
  // `isTranslating` flips true (= user clicked translate) and stops when it
  // flips false. Independent of whether pdf2zh has emitted any progress
  // event yet — the whole point is to keep moving during the silent gap.
  const elapsedMs = useElapsedSince(isTranslating, runStartedAtMs);
  const elapsedLabel = formatElapsed(elapsedMs);

  useEffect(() => {
    if (document.activeElement !== pageInputRef.current) {
      setPageInput(String(pdfCurrentPage));
    }
  }, [pdfCurrentPage]);

  useEffect(() => {
    setRangeInput("");
  }, [job.id]);

  function commitPageNavigation() {
    const requestedPage = Number(pageInput);
    if (!Number.isInteger(requestedPage) || pdfPageCount <= 0) {
      setPageInput(String(pdfCurrentPage));
      return;
    }
    const pageNumber = Math.max(1, Math.min(requestedPage, pdfPageCount));
    setPageInput(String(pageNumber));
    onPdfNavigate?.(pageNumber);
  }

  function submitPageNavigation(event: FormEvent) {
    event.preventDefault();
    commitPageNavigation();
  }

  function submitRangeSelection(event: FormEvent) {
    event.preventDefault();
    const range = rangeInput.trim();
    if (!range) return;
    onPdfSelectRange?.(range);
  }

  const isPdfSource = job.format === "pdf";
  const isPdf = isPdfSource && selectedOutputFormat === "pdf";
  const isPdfMarkdown = isPdfSource && selectedOutputFormat === "markdown";
  const markdownComponentReady = pdfMarkdownComponentStatus?.state === "installed";
  const markdownExtractionReady = pdfMarkdownExtractionStatus?.state === "ready";
  const markdownExtractionActive = pdfMarkdownExtractionStatus?.state === "extracting";
  const markdownOperationBusy =
    isPdfMarkdownInstalling ||
    isPdfMarkdownStartingExtraction ||
    markdownExtractionActive;
  const hasTranslation =
    activeTranslationFile &&
    (isPdf ||
      activeTranslationFile.completedSegments > 0);
  const allTranslated =
    !!activeTranslationFile &&
    (isPdf
      ? activeTranslationFile.status === "translated"
      : activeTranslationFile.segmentCount > 0 &&
        activeTranslationFile.completedSegments >= activeTranslationFile.segmentCount);
  const sameLanguage = sourceLang === targetLang;
  const noPdfPagesSelected = isPdf && pdfSelectedPageCount === 0;
  const translateDisabled =
    sameLanguage ||
    noPdfPagesSelected ||
    isTranslationBusyElsewhere ||
    isRuntimeUnavailable ||
    (isPdfMarkdown && !markdownExtractionReady) ||
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
    : isPdfMarkdown && !markdownExtractionReady
      ? "请先完成 PDF Markdown 提取"
    : noPdfPagesSelected
      ? "请选择页面"
      : undefined;
  const pdfSelectionReady = isPdf && pdfSelectedPageCount > 0;
  const pdfActionTarget =
    pdfSelectedPageCount > 0 && pdfPageCount > 0
      ? pdfPageSelectionLabel(pdfSelectedPages, pdfPageCount)
      : "页面";
  const pageSelectionLabel =
    pdfPageCount > 0
      ? pdfSelectedPageCount === pdfPageCount
        ? pdfActionTarget
        : `${pdfActionTarget} · 共 ${pdfPageCount} 页`
      : "等待页数";
  const runPhaseLabel = isPdf
    ? isPausingTranslation
      ? "正在停止"
      : pdfProgress
      ? PDF_PHASE_LABELS[pdfProgress.phase] ?? pdfProgress.phase
      : PDF_PHASE_LABELS.warmup
    : "翻译中";
  const runPageLabel =
    isPdf && pdfProgress?.completedPages != null && pdfProgress?.totalPages != null
      ? `${pdfProgress.completedPages}/${pdfProgress.totalPages} 页`
      : null;
  const runCountValue = isPdf
    ? pdfProgress?.translatedChars != null
      ? <RollingTranslatedChars value={pdfProgress.translatedChars} />
      : null
    : `${translatedCount}/${totalCount}`;

  return (
    <div className="border-b border-border/50 bg-background/95 px-4 py-2.5" data-window-no-drag>
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          {isPdfSource ? (
            <ToggleGroup
              type="single"
              value={selectedOutputFormat}
              onValueChange={(value) => {
                if (value === "pdf" || value === "markdown") {
                  onOutputFormatChange?.(value);
                }
              }}
              variant="outline"
              size="sm"
              aria-label="PDF 输出格式"
              className="h-8 shrink-0 gap-0"
            >
              <ToggleGroupItem value="pdf" aria-label="PDF 输出" className="h-8 px-2.5 text-xs">
                PDF
              </ToggleGroupItem>
              <ToggleGroupItem value="markdown" aria-label="Markdown 输出" className="h-8 px-2.5 text-xs">
                Markdown
              </ToggleGroupItem>
            </ToggleGroup>
          ) : null}
          <AnimatedWidth className="min-w-0" contentClassName="min-w-0">
            {isPdf ? (
              <div className="flex min-w-0 max-w-full flex-wrap items-center gap-2">
                <form
                  className={cn(topbarPanelClass, "gap-0.5 px-1")}
                  onSubmit={submitPageNavigation}
                >
                  <Button
                    type="button"
                    size="icon-xs"
                    variant="ghost"
                    title="上一页"
                    aria-label="上一页"
                    onClick={() => onPdfNavigate?.(Math.max(1, pdfCurrentPage - 1))}
                    disabled={pdfPageCount <= 0 || pdfCurrentPage <= 1}
                  >
                    <ChevronUp />
                  </Button>
                  <span className="shrink-0 text-foreground">第</span>
                  <Input
                    ref={pageInputRef}
                    value={pageInput}
                    onChange={(event) => setPageInput(event.target.value.replace(/\D/g, ""))}
                    onFocus={(event) => event.currentTarget.select()}
                    onBlur={() => {
                      if (pageInput !== String(pdfCurrentPage)) {
                        commitPageNavigation();
                      }
                    }}
                    inputMode="numeric"
                    aria-label="当前 PDF 页码"
                    className="h-6 w-10 rounded-md border-0 bg-muted/60 px-1 text-center text-xs tabular-nums shadow-none focus-visible:ring-2"
                  />
                  <span className="shrink-0 tabular-nums">/ {pdfPageCount || "-"} 页</span>
                  <Button
                    type="button"
                    size="icon-xs"
                    variant="ghost"
                    title="下一页"
                    aria-label="下一页"
                    onClick={() =>
                      onPdfNavigate?.(Math.min(pdfPageCount, pdfCurrentPage + 1))
                    }
                    disabled={pdfPageCount <= 0 || pdfCurrentPage >= pdfPageCount}
                  >
                    <ChevronDown />
                  </Button>
                </form>

                <div className={cn(topbarPanelClass, "min-w-0")}>
                  <span className="shrink-0 font-medium text-foreground">翻译范围</span>
                  <form className="flex items-center" onSubmit={submitRangeSelection}>
                    <Input
                      value={rangeInput}
                      onChange={(event) => setRangeInput(event.target.value)}
                      placeholder="如 21-30"
                      aria-label="输入要翻译的 PDF 页面范围"
                      disabled={isTranslating}
                      className="h-6 w-20 rounded-r-none border-border/60 px-1.5 text-xs shadow-none"
                    />
                    <Button
                      type="submit"
                      size="icon-xs"
                      variant="outline"
                      title="应用页面范围"
                      aria-label="应用页面范围"
                      disabled={isTranslating || !rangeInput.trim()}
                      className="rounded-l-none border-l-0"
                    >
                      <Check />
                    </Button>
                  </form>
                  <TopbarBadge variant={pdfSelectionReady ? "secondary" : "outline"}>
                    {pageSelectionLabel}
                  </TopbarBadge>
                  <TopbarDivider />
                  <div className="flex items-center gap-0.5">
                    <Button
                      size="xs"
                      variant="ghost"
                      className={topbarGhostButtonClass}
                      onClick={onSelectAllPages}
                      disabled={isTranslating || pdfSelectedPageCount === pdfPageCount}
                    >
                      全选
                    </Button>
                    <Button
                      size="xs"
                      variant="ghost"
                      className={topbarGhostButtonClass}
                      onClick={onDeselectAllPages}
                      disabled={isTranslating || pdfSelectedPageCount === 0}
                    >
                      清空
                    </Button>
                  </div>
                  <label className="flex h-6 cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-xs leading-none text-muted-foreground transition-colors hover:bg-muted/70 hover:text-foreground has-disabled:cursor-not-allowed has-disabled:opacity-50">
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
              </div>
            ) : isPdfMarkdown && !markdownExtractionReady ? (
              <div className={topbarPanelClass} title={pdfMarkdownError ?? undefined}>
                {isPdfMarkdownInstalling ? (
                  <>
                    <Loader2 className="size-3 animate-spin" />
                    <span>下载组件</span>
                    {pdfMarkdownInstallProgress?.expectedBytes ? (
                      <TopbarBadge>
                        {Math.min(
                          100,
                          Math.round(
                            (pdfMarkdownInstallProgress.downloadedBytes /
                              pdfMarkdownInstallProgress.expectedBytes) *
                              100,
                          ),
                        )}%
                      </TopbarBadge>
                    ) : null}
                  </>
                ) : markdownExtractionActive ? (
                  <>
                    <Loader2 className="size-3 animate-spin" />
                    <span>提取 Markdown</span>
                    <TopbarBadge>
                      {pdfMarkdownExtractionStatus?.completedPages ?? 0}/
                      {pdfMarkdownExtractionStatus?.pageCount || "-"} 页
                    </TopbarBadge>
                  </>
                ) : (
                  <span className="font-medium text-foreground">
                    {pdfMarkdownComponentStatus?.state === "unsupported"
                      ? "当前平台不支持 Markdown"
                      : pdfMarkdownComponentStatus?.state === "needs-repair"
                        ? "Markdown 组件需要修复"
                        : !markdownComponentReady
                          ? "Markdown 组件未安装"
                          : pdfMarkdownExtractionStatus?.state === "stale"
                            ? "源文件已变化，需要重新提取"
                            : pdfMarkdownExtractionStatus?.state === "failed"
                              ? "Markdown 提取失败"
                              : pdfMarkdownExtractionStatus?.state === "cancelled"
                                ? "Markdown 提取已取消"
                                : "尚未提取 Markdown"}
                  </span>
                )}
              </div>
            ) : selectedBlockCount > 0 ? (
              <div className={topbarPanelClass}>
                <span className="!text-xs font-medium !text-foreground">已选段落</span>
                <TopbarBadge variant="secondary">{selectedBlockCount} 段</TopbarBadge>
                <Button
                  size="xs"
                  variant="ghost"
                  className={topbarGhostButtonClass}
                  onClick={onClearSelection}
                  disabled={isTranslating}
                >
                  清空
                </Button>
              </div>
            ) : (
              <div className={cn(topbarPanelClass, "border-transparent bg-transparent")}>
                <span className="!text-xs font-medium !text-foreground">整篇文档</span>
                <TopbarBadge>{totalCount} 段</TopbarBadge>
              </div>
            )}
          </AnimatedWidth>
        </div>

        <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
          {isTranslating ? (
            <>
              <AnimatedWidth>
                <TranslationRunIndicator
                  phaseLabel={runPhaseLabel}
                  pageLabel={runPageLabel}
                  countValue={runCountValue}
                  countTitle={isPdf ? "已翻译字数" : "段落进度"}
                  elapsedLabel={elapsedLabel}
                />
              </AnimatedWidth>
              <AnimatedWidth>
                {isPausingTranslation ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled
                    className={cn(topbarButtonClass, "border-border/60 bg-card/80")}
                  >
                    <Loader2 className="size-3 animate-spin" />
                    正在停止
                  </Button>
                ) : confirmingCancel ? (
                  <TopbarConfirm
                    label="确认暂停？"
                    confirmLabel="暂停"
                    cancelLabel="继续"
                    destructive
                    onConfirm={() => {
                      onCancelTranslation();
                      setConfirmingCancel(false);
                    }}
                    onCancel={() => setConfirmingCancel(false)}
                  />
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => setConfirmingCancel(true)}
                    className={cn(topbarButtonClass, "border-border/60 bg-card/80")}
                  >
                    <Square className="size-3" /> 暂停
                  </Button>
                )}
              </AnimatedWidth>
            </>
          ) : (
            <>
              {hasTranslation && (
                <AnimatedWidth>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => onExport("translation")}
                    className={cn(topbarButtonClass, "border-border/60 bg-card/80")}
                  >
                    <Download className="size-3" />
                    {isPdfMarkdown ? "导出 Markdown" : "导出译文"}
                  </Button>
                </AnimatedWidth>
              )}

              <AnimatedWidth>
                <div className={cn(topbarPanelClass, "gap-1 px-1")}>
                  <Select value={sourceLang} onValueChange={onSourceLangChange}>
                    <SelectTrigger
                      aria-label="原文语言"
                      className="h-7 w-28 border-0 bg-transparent px-2 !text-xs shadow-none transition-colors focus:ring-0 data-[state=open]:bg-muted/70"
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
                  <ArrowRight className="size-3.5 !text-muted-foreground/45" aria-hidden="true" />
                  <Select value={targetLang} onValueChange={onTargetLangChange}>
                    <SelectTrigger
                      aria-label="译文语言"
                      className="h-7 w-28 border-0 bg-transparent px-2 !text-xs shadow-none transition-colors focus:ring-0 data-[state=open]:bg-muted/70"
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
              </AnimatedWidth>

              <AnimatedWidth>
                {isPdfMarkdown && !markdownExtractionReady ? (
                  isPdfMarkdownInstalling ? (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={onCancelPdfMarkdownInstall}
                      className={topbarButtonClass}
                    >
                      <Square className="size-3" /> 取消下载
                    </Button>
                  ) : markdownExtractionActive ? (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={onCancelPdfMarkdownExtraction}
                      className={topbarButtonClass}
                    >
                      <Square className="size-3" /> 停止提取
                    </Button>
                  ) : pdfMarkdownComponentStatus?.state === "unsupported" ? (
                    <Button size="sm" disabled className={topbarButtonClass}>
                      当前平台不支持
                    </Button>
                  ) : pdfMarkdownComponentStatus?.state === "needs-repair" ? (
                    <Button
                      size="sm"
                      onClick={onRepairPdfMarkdown}
                      disabled={isTranslationBusyElsewhere || isTranslating || markdownOperationBusy}
                      className={topbarButtonClass}
                    >
                      <RefreshCw className="size-3" /> 修复组件
                    </Button>
                  ) : !markdownComponentReady ? (
                    <Button
                      size="sm"
                      onClick={onInstallPdfMarkdown}
                      disabled={isTranslationBusyElsewhere || isTranslating || markdownOperationBusy}
                      className={topbarButtonClass}
                    >
                      <Download className="size-3" /> 安装 Markdown 组件
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      onClick={onStartPdfMarkdownExtraction}
                      disabled={isTranslationBusyElsewhere || isTranslating || markdownOperationBusy}
                      className={topbarButtonClass}
                    >
                      {isPdfMarkdownStartingExtraction ? (
                        <Loader2 className="size-3 animate-spin" />
                      ) : (
                        <Play className="size-3" />
                      )}
                      {pdfMarkdownExtractionStatus?.state === "stale" ? "重新提取" : "提取 Markdown"}
                    </Button>
                  )
                ) : isPdfEngineInstalling ? (
                  <Button size="sm" disabled className={topbarButtonClass}>
                    <Loader2 className="size-3 animate-spin" />
                    {pdfEngineProgressMessage ?? "正在准备 PDF 引擎…"}
                  </Button>
                ) : isRuntimeStarting ? (
                  <Button size="sm" disabled className={topbarButtonClass}>
                    <Loader2 className="size-3 animate-spin" />
                    正在启动模型…
                  </Button>
                ) : selectedBlockCount > 0 ? (
                  <Button
                    size="sm"
                    disabled={translateDisabled}
                    onClick={onRetranslateSelected}
                    className={topbarButtonClass}
                    title={translateTitle}
                  >
                    <RefreshCw className="size-3" />
                    重翻选中 {selectedBlockCount} 段
                  </Button>
                ) : allTranslated ? (
                  confirmingRetranslateAll ? (
                    <TopbarConfirm
                      label={isPdf ? `确认重翻${pdfActionTarget}？` : "确认重翻全部？"}
                      confirmLabel="确定"
                      destructive
                      disabled={translateDisabled}
                      title={translateTitle}
                      onConfirm={() => {
                        if (translateDisabled) return;
                        if (isPdf) onRetranslateSelected();
                        else onRetranslateAll();
                        setConfirmingRetranslateAll(false);
                      }}
                      onCancel={() => setConfirmingRetranslateAll(false)}
                    />
                  ) : (
                    <Button
                      size="sm"
                      disabled={translateDisabled}
                      onClick={() => setConfirmingRetranslateAll(true)}
                      className={topbarButtonClass}
                      title={translateTitle}
                    >
                      <RefreshCw className="size-3" />
                      {isPdf ? `重翻${pdfActionTarget}` : "重翻全部"}
                    </Button>
                  )
                ) : (
                  <Button
                    size="sm"
                    disabled={translateDisabled}
                    onClick={() => onTranslate(targetLang, sourceLang)}
                    className={topbarButtonClass}
                    title={translateTitle}
                  >
                    <Play className="size-3" />
                    {isPdf ? `翻译${pdfActionTarget}` : "翻译"}
                  </Button>
                )}
              </AnimatedWidth>
            </>
          )}
        </div>
      </div>
      {isRuntimeUnavailable && !isRuntimeStarting ? (
        <div className="mt-3 flex flex-col gap-2 rounded-lg border border-amber-500/35 bg-amber-500/8 px-3 py-2 text-sm sm:flex-row sm:items-center sm:justify-between">
          <div className="flex min-w-0 items-start gap-2">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-700 dark:text-amber-300" />
            <div className="min-w-0">
              <p className="font-medium text-foreground">本地模型需要处理后才能翻译</p>
              <p className="mt-0.5 text-xs leading-5 text-muted-foreground">
                {runtimeUnavailableMessage ??
                  "Rosetta 无法连接本地翻译服务，请到设置页修复本地运行时。"}
              </p>
            </div>
          </div>
          {onOpenRuntimeSettings ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={onOpenRuntimeSettings}
              className="shrink-0"
            >
              打开设置修复
            </Button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
