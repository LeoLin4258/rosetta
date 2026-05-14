import { useState } from "react";
import { AlertCircle, Download, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { ManagedRuntimeInstallProgress } from "@/types/rosetta";

type InstallStepProps = {
  progress: ManagedRuntimeInstallProgress | null;
  errorMessage: string | null;
  onCancel: () => void;
  onRetry: () => void;
  onSkipToExternal: () => void;
};

const ACTIVE_PHASES = new Set([
  "preflight",
  "downloading",
  "verifying",
  "writing-manifest",
]);

export function InstallStep({
  progress,
  errorMessage,
  onCancel,
  onRetry,
  onSkipToExternal,
}: InstallStepProps) {
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const [confirmingSkip, setConfirmingSkip] = useState(false);

  const percent = installPercent(progress);
  const isActive = !!progress && ACTIVE_PHASES.has(progress.phase);
  const speed = progress?.speedBytesPerSec ?? 0;

  if (errorMessage) {
    return (
      <div className="flex h-full flex-col items-center justify-between gap-4 px-14 py-10">
        <div className="flex w-full flex-1 flex-col items-center justify-center gap-5 text-center">
          <div className="flex size-14 items-center justify-center rounded-2xl bg-destructive/10 text-destructive">
            <AlertCircle className="size-7" strokeWidth={1.5} />
          </div>
          <div className="space-y-2">
            <h2 className="text-xl font-semibold">下载没有完成</h2>
            <p className="max-w-md text-sm leading-relaxed text-muted-foreground">
              {errorMessage}
            </p>
          </div>
          <div className="flex flex-col items-center gap-3">
            <Button size="lg" onClick={onRetry} className="min-w-44">
              <Download className="size-4" /> 重新下载
            </Button>
            {confirmingSkip ? (
              <div className="flex items-center gap-3">
                <span className="text-xs text-muted-foreground/50">跳过后可在设置中配置 API</span>
                <button
                  type="button"
                  onClick={onSkipToExternal}
                  className="text-xs text-primary/80 transition-colors hover:text-primary"
                >
                  确认
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmingSkip(false)}
                  className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground/70"
                >
                  取消
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setConfirmingSkip(true)}
                className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground/70"
              >
                使用自己的翻译 API →
              </button>
            )}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col items-center justify-between gap-4 px-14 py-10">
      <div className="flex w-full flex-1 flex-col items-center justify-center gap-5 text-center">
        <div className="flex size-14 items-center justify-center rounded-2xl bg-primary/10 text-primary">
          <Download className="size-7 animate-pulse" strokeWidth={1.5} />
        </div>
        <div className="space-y-2">
          <h2 className="text-xl font-semibold">正在下载翻译引擎</h2>
          <p className="text-sm text-muted-foreground">
            {phaseCaption(progress?.phase)}
          </p>
        </div>

        <div className="w-full space-y-2">
          <div className="relative h-2 w-full overflow-hidden rounded-full bg-muted">
            <div
              className="absolute inset-y-0 left-0 rounded-full bg-primary transition-[width] duration-200"
              style={{ width: `${percent}%` }}
            />
          </div>
          <div className="flex items-center justify-between text-xs tabular-nums text-muted-foreground/60">
            <span>{formatBytes(progress?.bytesDone ?? 0)} / {formatBytes(progress?.bytesTotal ?? 0)}</span>
            <span>{percent}% · {speed > 0 ? `${formatBytes(speed)}/s` : "—"}</span>
          </div>
        </div>
      </div>

      {confirmingCancel ? (
        <div className="flex items-center gap-3">
          <span className="text-xs text-muted-foreground/60">确认取消下载？</span>
          <button
            type="button"
            onClick={onCancel}
            className="text-xs text-destructive/70 transition-colors hover:text-destructive"
          >
            确定
          </button>
          <button
            type="button"
            onClick={() => setConfirmingCancel(false)}
            className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground/70"
          >
            继续下载
          </button>
        </div>
      ) : (
        <Button
          variant="outline"
          size="sm"
          onClick={() => setConfirmingCancel(true)}
          disabled={!isActive}
          className="gap-2"
        >
          <X className="size-4" /> 取消
        </Button>
      )}
    </div>
  );
}

function installPercent(progress: ManagedRuntimeInstallProgress | null): number {
  if (!progress || progress.bytesTotal === 0) return 0;
  return Math.min(100, Math.floor((progress.bytesDone * 100) / progress.bytesTotal));
}

function phaseCaption(phase: ManagedRuntimeInstallProgress["phase"] | undefined): string {
  switch (phase) {
    case "preflight":
      return "准备下载…";
    case "downloading":
      return "约 1.3 GB · 下载完成后无需再联网";
    case "verifying":
      return "校验文件完整性…";
    case "writing-manifest":
      return "写入安装清单…";
    case "done":
      return "已完成";
    case "failed":
      return "出错了";
    case "cancelled":
      return "已取消";
    default:
      return "约 1.3 GB · 下载完成后无需再联网";
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
