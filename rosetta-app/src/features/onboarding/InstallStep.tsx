import { AlertCircle, Download, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { ManagedRuntimeInstallProgress } from "@/types/rosetta";

import { OnboardingStepShell } from "./OnboardingStepShell";

type InstallProgressLike = Pick<
  ManagedRuntimeInstallProgress,
  "bytesDone" | "bytesTotal" | "speedBytesPerSec" | "message" | "lastError"
> & {
  phase:
    | ManagedRuntimeInstallProgress["phase"]
    | "extracting";
};

type InstallStepProps = {
  progress: InstallProgressLike | null;
  errorMessage: string | null;
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
  const percent = installPercent(progress);
  const isActive = !!progress && ACTIVE_PHASES.has(progress.phase);
  const speed = progress?.speedBytesPerSec ?? 0;

  if (errorMessage) {
    return (
      <OnboardingStepShell
        stepLabel={stepLabel ?? "安装出错"}
        progressValue={progressValue}
        title={errorTitle}
        description={errorMessage}
        align="start"
      >
        <div className="flex items-center gap-2 text-destructive">
          <AlertCircle className="size-4" strokeWidth={1.75} />
          <span className="text-xs font-medium">需要重试或跳过</span>
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
