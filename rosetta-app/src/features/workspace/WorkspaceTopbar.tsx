import {
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ArrowRight,
  AlertTriangle,
  Download,
  FileText,
  Loader2,
  Pause,
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
import {
  PDF_AUTO_SELECT_ALL_PAGE_LIMIT,
  PDF_LONG_DOCUMENT_DEFAULT_SELECTION,
} from "@/lib/pdfPageSelectionPolicy";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type {
  PdfV3RunControlStatus,
  RosettaJobSummary,
  RosettaTranslationFile,
} from "@/types/rosetta";
import type { PdfV3RunOperation } from "./usePdfV3RunControl";

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
  isRuntimeUnavailable?: boolean;
  runtimeUnavailableMessage?: string | null;
  translatedCount: number;
  totalCount: number;
  /// Epoch ms when the active run started. Anchors the elapsed timer so it
  /// survives unmount/remount (file switches) during a long run.
  runStartedAtMs?: number | null;
  pdfV3RunStatus?: PdfV3RunControlStatus | null;
  pdfV3ControlOperation?: PdfV3RunOperation | null;
  pdfV3CanRecover?: boolean;
  pdfV3IsDiscovering?: boolean;
  pdfV3DiscoveryError?: string | null;
  isPdfV3Exporting?: boolean;
  sourceLang: string;
  targetLang: string;
  selectedBlockCount: number;
  pdfSelectedPageCount?: number;
  pdfPageCount?: number;
  onSelectAllPages?: () => void;
  onSelectPreviewPages?: () => void;
  onDeselectAllPages?: () => void;
  onSourceLangChange: (lang: string) => void;
  onTargetLangChange: (lang: string) => void;
  onTranslate: (targetLang: string, sourceLang: string) => void;
  onCancelTranslation: () => void;
  onPausePdfV3Run?: () => void;
  onResumePdfV3Run?: () => void;
  onCancelPdfV3Run?: () => void;
  onRecoverPdfV3Run?: () => void;
  onExport: (kind: "translation" | "bilingual") => void;
  onRetranslateSelected: () => void;
  onClearSelection: () => void;
  onRetranslateAll: () => void;
  onOpenRuntimeSettings?: () => void;
};

/// Format milliseconds as `mm:ss` for active translation runs.
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
  elapsedLabel: string | null;
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
        {elapsedLabel ? (
          <RunMetric title="已用时间" icon={<Timer className="size-3" />}>
            <span className="rosetta-run-time-value">{elapsedLabel}</span>
          </RunMetric>
        ) : null}
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

export function WorkspaceTopbar({
  job,
  activeTranslationFile,
  isTranslating,
  isTranslationBusyElsewhere = false,
  isRuntimeStarting,
  isRuntimeUnavailable = false,
  runtimeUnavailableMessage = null,
  translatedCount,
  totalCount,
  runStartedAtMs = null,
  pdfV3RunStatus = null,
  pdfV3ControlOperation = null,
  pdfV3CanRecover = false,
  pdfV3IsDiscovering = false,
  pdfV3DiscoveryError = null,
  isPdfV3Exporting = false,
  sourceLang,
  targetLang,
  selectedBlockCount,
  pdfSelectedPageCount = 0,
  pdfPageCount = 0,
  onSelectAllPages,
  onSelectPreviewPages,
  onDeselectAllPages,
  onSourceLangChange,
  onTargetLangChange,
  onTranslate,
  onCancelTranslation,
  onPausePdfV3Run,
  onResumePdfV3Run,
  onCancelPdfV3Run,
  onRecoverPdfV3Run,
  onExport,
  onRetranslateSelected,
  onClearSelection,
  onRetranslateAll,
  onOpenRuntimeSettings,
}: WorkspaceTopbarProps) {
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const [confirmingRetranslateAll, setConfirmingRetranslateAll] = useState(false);
  const elapsedMs = useElapsedSince(isTranslating, runStartedAtMs);
  const elapsedLabel = formatElapsed(elapsedMs);

  useEffect(() => {
    setConfirmingCancel(false);
  }, [isTranslating, job.id, pdfV3RunStatus?.runId, pdfV3RunStatus?.state]);

  const isPdf = job.format === "pdf";
  const allTranslated =
    isPdf
      ? pdfV3RunStatus?.state === "completed"
      : !!activeTranslationFile &&
        activeTranslationFile.segmentCount > 0 &&
        activeTranslationFile.completedSegments >= activeTranslationFile.segmentCount;
  const hasTranslation = isPdf
    ? allTranslated
    : activeTranslationFile && activeTranslationFile.completedSegments > 0;
  const pdfV3HasOpenRun =
    !!pdfV3RunStatus &&
    pdfV3RunStatus.state !== "cancelled" &&
    pdfV3RunStatus.state !== "failed" &&
    pdfV3RunStatus.state !== "completed";
  const pdfV3FailedRunNeedsRecovery =
    pdfV3RunStatus?.state === "failed" &&
    !pdfV3RunStatus.ownedByCurrentSession;
  const pdfSelectionLocked =
    isTranslating || pdfV3HasOpenRun || pdfV3ControlOperation === "creating";
  const sameLanguage = sourceLang === targetLang;
  const noPdfPagesSelected = isPdf && pdfSelectedPageCount === 0;
  const translateDisabled =
    sameLanguage ||
    noPdfPagesSelected ||
    isTranslationBusyElsewhere ||
    isRuntimeUnavailable ||
    (isPdf && (pdfV3IsDiscovering || pdfV3DiscoveryError != null)) ||
    (isPdf && (pdfV3HasOpenRun || pdfV3ControlOperation != null));
  const translateTitle = sameLanguage
    ? "原文与译文语言不能相同"
    : isTranslationBusyElsewhere
      ? "另一个文件正在翻译"
    : isRuntimeUnavailable
      ? (runtimeUnavailableMessage ?? "本地翻译模型尚未就绪")
    : isPdf && pdfV3IsDiscovering
      ? "正在读取 PDF v3 运行状态"
    : isPdf && pdfV3DiscoveryError
      ? pdfV3DiscoveryError
    : isPdf && pdfV3HasOpenRun
      ? "请先恢复或停止当前 PDF 运行"
    : isPdf && pdfV3ControlOperation != null
      ? "PDF 操作正在进行"
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
  const longPdfPreviewPageCount = Math.min(
    PDF_LONG_DOCUMENT_DEFAULT_SELECTION,
    Math.max(pdfPageCount, 0),
  );
  const showLongPdfControls =
    isPdf && pdfPageCount > PDF_AUTO_SELECT_ALL_PAGE_LIMIT;
  const showLongPdfHint =
    showLongPdfControls &&
    !pdfSelectionLocked &&
    pdfSelectedPageCount <= longPdfPreviewPageCount;
  const runPhaseLabel = isPdf
    ? pdfV3OperationLabel(pdfV3ControlOperation) ??
      pdfV3WorkerLabel(pdfV3RunStatus)
    : "翻译中";
  const runPageLabel = isPdf && pdfV3RunStatus
    ? `${translatedCount}/${totalCount} 页`
    : null;
  const runCountValue = isPdf ? null : `${translatedCount}/${totalCount}`;

  return (
    <div className="border-b border-border/50 bg-background/95 px-4 py-2.5" data-window-no-drag>
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <AnimatedWidth className="min-w-0" contentClassName="min-w-0">
            {isPdf ? (
              <div className={cn(topbarPanelClass, "min-w-0")}>
                <span className="shrink-0 !text-xs font-medium !text-foreground">
                  页面范围
                </span>
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
                    disabled={pdfSelectionLocked || pdfSelectedPageCount === pdfPageCount}
                  >
                    全选
                  </Button>
                  <Button
                    size="xs"
                    variant="ghost"
                    className={topbarGhostButtonClass}
                    onClick={onDeselectAllPages}
                    disabled={pdfSelectionLocked || pdfSelectedPageCount === 0}
                  >
                    清空
                  </Button>
                  {showLongPdfControls ? (
                    <Button
                      size="xs"
                      variant="ghost"
                      className={topbarGhostButtonClass}
                      onClick={onSelectPreviewPages}
                      disabled={pdfSelectionLocked}
                    >
                      前 {longPdfPreviewPageCount} 页
                    </Button>
                  ) : null}
                </div>
                {showLongPdfHint ? (
                  <span className="max-w-[15rem] truncate !text-xs !text-muted-foreground">
                    长 PDF，默认前 {longPdfPreviewPageCount} 页
                  </span>
                ) : null}
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
          {isPdf &&
          (pdfV3HasOpenRun ||
            pdfV3FailedRunNeedsRecovery ||
            pdfV3ControlOperation != null) ? (
            <>
              {pdfV3HasOpenRun ? (
                <AnimatedWidth>
                  <TranslationRunIndicator
                    phaseLabel={runPhaseLabel}
                    pageLabel={runPageLabel}
                    countValue={runCountValue}
                    countTitle="页面进度"
                    elapsedLabel={pdfV3RunStatus?.state === "paused" ? null : elapsedLabel}
                  />
                </AnimatedWidth>
              ) : null}
              <AnimatedWidth>
                {pdfV3ControlOperation ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled
                    className={cn(topbarButtonClass, "border-border/60 bg-card/80")}
                  >
                    <Loader2 className="size-3 animate-spin" />
                    {pdfV3OperationLabel(pdfV3ControlOperation)}
                  </Button>
                ) : pdfV3RunStatus && !pdfV3RunStatus.ownedByCurrentSession ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!pdfV3CanRecover}
                    onClick={onRecoverPdfV3Run}
                    className={cn(topbarButtonClass, "border-border/60 bg-card/80")}
                    title={
                      pdfV3CanRecover
                        ? "接管失去心跳的 PDF 运行"
                        : "此运行仍由另一个窗口持有"
                    }
                  >
                    <RefreshCw className="size-3" />
                    {pdfV3CanRecover ? "接管运行" : "其他窗口运行中"}
                  </Button>
                ) : confirmingCancel ? (
                  <TopbarConfirm
                    label="确认停止？"
                    confirmLabel="停止"
                    destructive
                    onConfirm={() => {
                      onCancelPdfV3Run?.();
                      setConfirmingCancel(false);
                    }}
                    onCancel={() => setConfirmingCancel(false)}
                  />
                ) : pdfV3RunStatus?.state === "cancelling" ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled
                    className={cn(topbarButtonClass, "border-border/60 bg-card/80")}
                  >
                    <Loader2 className="size-3 animate-spin" />
                    正在停止
                  </Button>
                ) : (
                  <div className="flex items-center gap-1.5">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={
                        pdfV3RunStatus?.state === "paused"
                          ? onResumePdfV3Run
                          : onPausePdfV3Run
                      }
                      className={cn(topbarButtonClass, "border-border/60 bg-card/80")}
                    >
                      {pdfV3RunStatus?.state === "paused" ? (
                        <Play className="size-3" />
                      ) : (
                        <Pause className="size-3" />
                      )}
                      {pdfV3RunStatus?.state === "paused" ? "恢复" : "暂停"}
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="outline"
                      onClick={() => setConfirmingCancel(true)}
                      className="size-8 border-border/60 bg-card/80"
                      title="停止 PDF 翻译"
                      aria-label="停止 PDF 翻译"
                    >
                      <Square className="size-3" />
                    </Button>
                  </div>
                )}
              </AnimatedWidth>
            </>
          ) : isTranslating ? (
            <>
              <AnimatedWidth>
                <TranslationRunIndicator
                  phaseLabel={runPhaseLabel}
                  pageLabel={runPageLabel}
                  countValue={runCountValue}
                  countTitle="段落进度"
                  elapsedLabel={elapsedLabel}
                />
              </AnimatedWidth>
              <AnimatedWidth>
                {confirmingCancel ? (
                  <TopbarConfirm
                    label="确认停止？"
                    confirmLabel="停止"
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
                    <Square className="size-3" /> 停止
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
                    disabled={isPdfV3Exporting}
                    className={cn(topbarButtonClass, "border-border/60 bg-card/80")}
                  >
                    {isPdfV3Exporting ? (
                      <Loader2 className="size-3 animate-spin" />
                    ) : (
                      <Download className="size-3" />
                    )} {isPdfV3Exporting ? "正在导出" : "导出译文"}
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
                {isRuntimeStarting ? (
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
                      label={isPdf ? `确认重翻${selectedPdfLabel}？` : "确认重翻全部？"}
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
                      {isPdf ? `重翻${selectedPdfLabel}` : "重翻全部"}
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
                    {isPdf ? `翻译${selectedPdfLabel}` : "翻译"}
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

function pdfV3OperationLabel(operation: PdfV3RunOperation | null) {
  switch (operation) {
    case "creating":
      return "正在创建运行";
    case "pausing":
      return "正在暂停";
    case "resuming":
      return "正在恢复";
    case "cancelling":
      return "正在停止";
    case "recovering":
      return "正在接管";
    case "retrying":
      return "正在重试页面";
    default:
      return null;
  }
}

function pdfV3WorkerLabel(status: PdfV3RunControlStatus | null) {
  if (!status) return "准备 PDF 翻译";
  if (status.state === "paused") return "已暂停";
  if (status.state === "cancelling") return "正在停止";
  if (status.state === "cancelled") return "已停止";
  if (status.state === "failed") return "处理失败";
  if (status.state === "completed") return "已完成";

  switch (status.worker.stage) {
    case "starting":
      return "正在准备";
    case "extracting":
      return "正在解析页面";
    case "translating":
      return "正在翻译页面";
    case "stopping":
      return "正在停止";
    case "waiting":
      return "等待下一页";
    default:
      return "PDF 翻译中";
  }
}
