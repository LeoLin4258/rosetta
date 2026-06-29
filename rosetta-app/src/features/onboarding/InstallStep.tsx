import { useMemo, useState } from "react";
import { AlertCircle, Check, Copy, Download, LoaderCircle, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import type {
  ManagedRuntimeInstallProgress,
  ManagedRuntimeLogsSummary,
} from "@/types/rosetta";

import { OnboardingStepShell } from "./OnboardingStepShell";

type InstallProgressLike = Pick<
  ManagedRuntimeInstallProgress,
  "bytesDone" | "bytesTotal" | "speedBytesPerSec" | "message" | "lastError"
> & {
  phase:
    | ManagedRuntimeInstallProgress["phase"]
    | "extracting"
    | "preparing";
};

type InstallStepProps = {
  progress: InstallProgressLike | null;
  errorMessage: string | null;
  diagnostics?: Record<string, string | number | boolean | null | undefined>;
  logs?: ManagedRuntimeLogsSummary | null;
  onCancel: () => void;
  onRetry: () => void;
  onSkip: () => void;
  progressValue: number;
  title?: string;
  errorTitle?: string;
  retryLabel?: string;
  cancelLabel?: string;
  defaultCaption?: string;
  downloadingCaption?: string;
  skipLabel?: string;
  stepLabel?: string;
};

const ACTIVE_PHASES = new Set([
  "preflight",
  "downloading",
  "verifying",
  "writing-manifest",
  "extracting",
]);

export function InstallStep({
  progress,
  errorMessage,
  diagnostics,
  logs,
  onCancel,
  onRetry,
  onSkip,
  progressValue,
  title = "正在下载翻译模型",
  errorTitle = "下载没有完成",
  retryLabel = "重新下载",
  cancelLabel = "取消",
  defaultCaption = "下载完成后无需再联网",
  downloadingCaption = "下载完成后无需再联网",
  skipLabel = "使用自己的翻译 API →",
  stepLabel,
}: InstallStepProps) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">(
    "idle"
  );
  const percent = installPercent(progress);
  const isActive = !!progress && ACTIVE_PHASES.has(progress.phase);
  const isPostDownloadPhase =
    progress?.phase === "verifying" ||
    progress?.phase === "extracting" ||
    progress?.phase === "writing-manifest" ||
    progress?.phase === "preparing";
  const speed = progress?.speedBytesPerSec ?? 0;
  const errorSummary = useMemo(
    () => summarizeErrorMessage(errorMessage),
    [errorMessage]
  );
  const diagnosticText = useMemo(
    () => buildDiagnosticText(errorMessage, progress, diagnostics, logs),
    [diagnostics, errorMessage, logs, progress]
  );

  const handleCopyDiagnostics = async () => {
    if (!diagnosticText) return;
    try {
      await navigator.clipboard.writeText(diagnosticText);
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 1800);
    } catch {
      setCopyState("failed");
      window.setTimeout(() => setCopyState("idle"), 2400);
    }
  };

  if (errorMessage) {
    return (
      <OnboardingStepShell
        stepLabel={stepLabel ?? "安装出错"}
        progressValue={progressValue}
        title={errorTitle}
        description={errorSummary}
        align="start"
      >
        <div className="flex items-center gap-2 text-destructive">
          <AlertCircle className="size-4" strokeWidth={1.75} />
          <span className="text-xs font-medium">需要重试或复制错误信息</span>
        </div>
        <div className="w-full space-y-3 rounded-lg border border-border bg-card p-3">
          <div className="flex items-center justify-between gap-3">
            <p className="text-xs font-medium text-foreground">错误信息</p>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleCopyDiagnostics}
              className="gap-1.5"
            >
              {copyState === "copied" ? (
                <Check className="size-3.5" />
              ) : (
                <Copy className="size-3.5" />
              )}
              {copyState === "copied"
                ? "已复制"
                : copyState === "failed"
                  ? "复制失败"
                  : "复制错误信息"}
            </Button>
          </div>
          <pre className="max-h-28 overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted p-2 font-mono text-[11px] leading-5 text-muted-foreground">
            {errorMessage}
          </pre>
        </div>
        <Button size="lg" onClick={onRetry} className="h-11 w-full gap-2">
          <Download className="size-4" /> {retryLabel}
        </Button>
        <button
          type="button"
          onClick={onSkip}
          className="text-left text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground/70"
        >
          {skipLabel}
        </button>
      </OnboardingStepShell>
    );
  }

  return (
    <OnboardingStepShell
      stepLabel={stepLabel ?? "安装中"}
      progressValue={progressValue}
      title={title}
      description={
        progress?.message ||
        phaseCaption(progress?.phase, defaultCaption, downloadingCaption)
      }
      align="start"
    >
      <div className="w-full space-y-3">
        {isPostDownloadPhase ? (
          <>
            <div className="relative h-2 w-full overflow-hidden rounded-full bg-muted">
              <div className="absolute inset-y-0 left-0 w-full animate-pulse rounded-full bg-foreground/70 motion-reduce:animate-none" />
            </div>
            <div className="flex items-center gap-2 text-xs text-muted-foreground/70">
              <LoaderCircle
                className="size-3.5 animate-spin motion-reduce:animate-none"
                aria-hidden="true"
              />
              <span>{postDownloadCaption(progress?.phase)}</span>
            </div>
          </>
        ) : (
          <>
            <div className="relative h-2 w-full overflow-hidden rounded-full bg-muted">
              <div
                className="absolute inset-y-0 left-0 rounded-full bg-foreground/80 transition-[width] duration-200"
                style={{ width: `${percent}%` }}
              />
            </div>
            <div className="text-xs tabular-nums text-muted-foreground/60">
              {percent}% · {formatBytes(progress?.bytesDone ?? 0)} /{" "}
              {formatBytes(progress?.bytesTotal ?? 0)}
              {speed > 0 ? ` · ${formatBytes(speed)}/s` : ""}
            </div>
          </>
        )}
      </div>

      <button
        type="button"
        onClick={onCancel}
        disabled={!isActive}
        className="inline-flex items-center gap-2 text-xs text-muted-foreground/45 transition-colors hover:text-muted-foreground/70 disabled:pointer-events-none disabled:opacity-30"
      >
        <X className="size-4" /> {cancelLabel}
      </button>
    </OnboardingStepShell>
  );
}

function installPercent(progress: InstallProgressLike | null): number {
  if (!progress || progress.bytesTotal === 0) return 0;
  return Math.min(100, Math.floor((progress.bytesDone * 100) / progress.bytesTotal));
}

function phaseCaption(
  phase: InstallProgressLike["phase"] | undefined,
  defaultCaption: string,
  downloadingCaption: string
): string {
  switch (phase) {
    case "preflight":
      return "准备中…";
    case "downloading":
      return downloadingCaption;
    case "verifying":
      return "校验文件完整性…";
    case "extracting":
      return "正在安装到本机…";
    case "writing-manifest":
      return "写入安装清单…";
    case "preparing":
      return "正在启动组件…";
    case "done":
      return "已完成";
    case "failed":
      return "出错了";
    case "cancelled":
      return "已取消";
    default:
      return defaultCaption;
  }
}

function postDownloadCaption(
  phase: InstallProgressLike["phase"] | undefined
): string {
  switch (phase) {
    case "verifying":
      return "下载完成，正在校验文件完整性";
    case "extracting":
      return "校验完成，正在解压并安装组件";
    case "writing-manifest":
      return "组件已解压，正在保存安装信息";
    case "preparing":
      return "组件已安装，正在启动 PDF 引擎";
    default:
      return "正在完成安装";
  }
}

function summarizeErrorMessage(message: string | null): string {
  if (!message) {
    return "安装没有完成。请重试，或复制错误信息发给开发者排查。";
  }

  const firstMeaningfulLine =
    message
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find((line) => line && !line.startsWith("---")) ?? "";

  const logLike =
    message.includes("slot load_model") ||
    message.includes("srv load_model") ||
    message.includes("llama_server") ||
    message.includes("--- sidecar log ---");

  if (logLike) {
    return "本地翻译引擎启动没有完成。下面保留了可复制的错误信息，方便发给开发者排查。";
  }

  if (!firstMeaningfulLine) {
    return "安装没有完成。请重试，或复制错误信息发给开发者排查。";
  }

  return truncateForSummary(firstMeaningfulLine);
}

function truncateForSummary(text: string): string {
  const maxLength = 120;
  if (text.length <= maxLength) return text;
  return `${text.slice(0, maxLength).trimEnd()}…`;
}

function buildDiagnosticText(
  errorMessage: string | null,
  progress: InstallProgressLike | null,
  diagnostics: InstallStepProps["diagnostics"],
  logs: ManagedRuntimeLogsSummary | null | undefined
): string {
  const lines = [
    "Rosetta onboarding error",
    `time: ${new Date().toISOString()}`,
  ];

  if (progress) {
    lines.push(`phase: ${progress.phase}`);
    lines.push(`bytes: ${progress.bytesDone}/${progress.bytesTotal}`);
    if (progress.speedBytesPerSec > 0) {
      lines.push(`speedBytesPerSec: ${progress.speedBytesPerSec}`);
    }
    if (progress.message) {
      lines.push(`progressMessage: ${progress.message}`);
    }
    if (progress.lastError) {
      lines.push(`progressLastError: ${progress.lastError}`);
    }
  }

  if (diagnostics) {
    for (const [key, value] of Object.entries(diagnostics)) {
      if (value == null || value === "") continue;
      lines.push(`${key}: ${String(value)}`);
    }
  }

  if (logs) {
    lines.push("", "runtimeLog:");
    lines.push(`logFile: ${logs.logFile}`);
    lines.push(`logMessage: ${logs.message}`);
    if (logs.logTail.length > 0) {
      lines.push("--- tail ---");
      lines.push(...logs.logTail.slice(-80));
    }
  }

  lines.push("", "error:", errorMessage ?? "");
  return lines.join("\n");
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let i = 1; i < units.length && value >= 1024; i += 1) {
    value /= 1024;
    unit = units[i];
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
}
