import { useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  Cpu,
  Download,
  LoaderCircle,
  Play,
  RefreshCw,
  Square,
  TerminalSquare,
  X,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { useManagedRwkvRuntime } from "@/lib/useManagedRwkvRuntime";
import { useRosettaStore } from "@/store/useRosettaStore";
import type {
  ManagedRuntimeInstallPhase,
  ManagedRuntimeLogsSummary,
  ManagedRuntimeState,
  ManagedRuntimeStatus,
} from "@/types/rosetta";

const INSTALL_ACTIVE_PHASES: ReadonlySet<ManagedRuntimeInstallPhase> = new Set([
  "preflight",
  "downloading",
  "verifying",
  "extracting",
  "writing-manifest",
]);

export function LocalRwkvPanel({ className }: { className?: string }) {
  const rt = useManagedRwkvRuntime();
  const status = rt.status;
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [logs, setLogs] = useState<ManagedRuntimeLogsSummary | null>(null);

  const state: ManagedRuntimeState | null = status?.state ?? null;
  const isUnsupported = state === "unsupported";
  const installPhase = rt.progress?.phase ?? null;
  const isInstallActive = !!installPhase && INSTALL_ACTIVE_PHASES.has(installPhase);

  async function openDetails(next: boolean) {
    setDetailsOpen(next);
    if (next && !logs && status && !isUnsupported) {
      const summary = await rt.readLogs();
      setLogs(summary);
    }
  }

  return (
    <section
      className={cn(
        "flex flex-col gap-5 rounded-xl border border-black/8 bg-muted/28 p-5 dark:border-white/8 dark:bg-muted/12",
        className
      )}
      id="local-rwkv"
    >
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
            <Cpu className="size-4" />
          </div>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold tracking-normal">
              管理本地模型
            </h3>
            <p className="mt-1 text-sm text-muted-foreground">
              下载或启动 Rosetta 管理的本地模型。翻译请求在本机处理。
            </p>
          </div>
        </div>
        <RuntimeBadge state={state} isInstallActive={isInstallActive} />
      </div>

      <div className="flex flex-col gap-4 border-t pt-4">
        <StatusRow
          state={state}
          status={status}
          isInstallActive={isInstallActive}
        />

        {isInstallActive && (
          <InstallProgressRow
            percent={installPercent(rt.progress)}
            message={rt.progress?.message ?? ""}
            speedBytesPerSec={rt.progress?.speedBytesPerSec ?? 0}
          />
        )}

        <RuntimeControls
          state={state}
          isInstallActive={isInstallActive}
          isStarting={rt.isStarting}
          isStopping={rt.isStopping}
          onStart={() => void rt.start()}
          onStop={() => void rt.stop()}
          onCancel={() => void rt.cancelInstall()}
          isUnsupported={isUnsupported}
        />

        {showProxyInput(state, isInstallActive) && (
          <DownloadProxyField disabled={isInstallActive} />
        )}

        {rt.lastError && (
          <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
            <AlertCircle className="mt-0.5 size-4 shrink-0" />
            <span className="break-all">{rt.lastError}</span>
          </div>
        )}

        {!isUnsupported && (
          <Collapsible open={detailsOpen} onOpenChange={openDetails}>
            <CollapsibleTrigger asChild>
              <button
                type="button"
                className="flex h-8 w-fit items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <ChevronDown
                  className={cn(
                    "size-3.5 transition-transform",
                    detailsOpen && "rotate-180"
                  )}
                />
                技术信息
              </button>
            </CollapsibleTrigger>
            <CollapsibleContent>
              <div className="mt-2 grid gap-4 border-t pt-4 lg:grid-cols-[minmax(0,1fr)_minmax(18rem,0.8fr)]">
                <div className="flex min-w-0 flex-col gap-4">
                  <RepairActions
                    state={state}
                    installPhase={installPhase}
                    isInstallActive={isInstallActive}
                    isInstalling={rt.isInstalling}
                    modelSizeBytes={status?.profile?.modelSizeBytes ?? null}
                    onInstall={() => void rt.install({ repair: false })}
                    onRepair={() => void rt.install({ repair: true })}
                  />
                  {status && <ModelInfoRows status={status} />}
                </div>

                {status && (
                  <div className="flex min-w-0 flex-col gap-2">
                    <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                      <TerminalSquare className="size-3.5" />
                      运行日志
                    </div>
                    <LogsSummaryBlock logs={logs} />
                  </div>
                )}
              </div>
            </CollapsibleContent>
          </Collapsible>
        )}
      </div>
    </section>
  );
}

// ─── Status dot + one-line text ───────────────────────────────────────────────

function StatusRow({
  state,
  status,
  isInstallActive,
}: {
  state: ManagedRuntimeState | null;
  status: ManagedRuntimeStatus | null;
  isInstallActive: boolean;
}) {
  const { dot, label, sub, spinning } = resolveStatus(state, status, isInstallActive);
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2.5">
        {spinning ? (
          <LoaderCircle className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
        ) : (
          <div className={cn("size-2 shrink-0 rounded-full", dot)} />
        )}
        <span className="text-sm font-medium">{label}</span>
      </div>
      {sub && <p className="pl-4 text-xs text-muted-foreground">{sub}</p>}
    </div>
  );
}

function resolveStatus(
  state: ManagedRuntimeState | null,
  status: ManagedRuntimeStatus | null,
  isInstallActive: boolean
): { dot: string; label: string; sub?: string; spinning?: boolean } {
  if (isInstallActive) {
    return {
      dot: "bg-blue-500",
      label: "正在安装本地翻译引擎",
      sub: "运行包和模型校验完成后可离线使用。",
      spinning: true,
    };
  }
  switch (state) {
    case "ready":
      return {
        dot: "bg-emerald-500",
        label: "本地模型正在运行",
        sub: status?.process.baseUrl
          ? "翻译请求会发送到本机服务，不会离开这台设备。"
          : undefined,
      };
    case "starting":
      return {
        dot: "bg-blue-500",
        label: "正在启动本地模型",
        spinning: true,
      };
    case "installed":
    case "stopped":
      return {
        dot: "bg-amber-400",
        label: "本地模型已安装，需要启动后才能翻译",
      };
    case "failed":
      return {
        dot: "bg-destructive",
        label: "本地模型启动失败。展开技术信息查看日志。",
      };
    case "unsupported":
      return {
        dot: "bg-muted-foreground/40",
        label: "当前设备不支持本地翻译",
        sub:
          status?.hardware?.message ??
          "你仍可显式配置自己的翻译 API。",
      };
    case "not-installed":
      return {
        dot: "bg-muted-foreground/30",
        label: "本地模型尚未下载",
        sub: "展开技术信息可手动下载，或重启 Rosetta 进入安装向导。",
      };
    default:
      return {
        dot: "bg-muted-foreground/30",
        label: "正在检查本地模型状态",
        spinning: true,
      };
  }
}

// ─── Runtime controls (start / stop / cancel install) — NOT install/repair ───

function RuntimeControls({
  state,
  isInstallActive,
  isStarting,
  isStopping,
  onStart,
  onStop,
  onCancel,
  isUnsupported,
}: {
  state: ManagedRuntimeState | null;
  isInstallActive: boolean;
  isStarting: boolean;
  isStopping: boolean;
  onStart: () => void;
  onStop: () => void;
  onCancel: () => void;
  isUnsupported: boolean;
}) {
  if (isUnsupported || state === "not-installed") return null;

  if (isInstallActive) {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" size="sm" onClick={onCancel}>
          <X className="size-4" /> 取消下载
        </Button>
        <span className="text-xs text-muted-foreground">
          取消后，下次下载会从中断处继续。
        </span>
      </div>
    );
  }

  if (state === "ready") {
    return (
      <Button variant="outline" size="sm" onClick={onStop} disabled={isStopping}>
        <Square className="size-4" /> 停止翻译服务
      </Button>
    );
  }

  if (state === "starting") {
    return (
      <Button variant="outline" size="sm" disabled>
        <LoaderCircle className="size-4 animate-spin" /> 正在启动
      </Button>
    );
  }

  if (state === "installed" || state === "stopped" || state === "failed") {
    return (
      <Button size="sm" onClick={onStart} disabled={isStarting}>
        {isStarting ? (
          <LoaderCircle className="size-4 animate-spin" />
        ) : (
          <Play className="size-4" />
        )}
        启动本地翻译
      </Button>
    );
  }

  return null;
}

// ─── Install / repair actions (inside details collapsible) ────────────────────

function RepairActions({
  state,
  installPhase,
  isInstallActive,
  isInstalling,
  modelSizeBytes,
  onInstall,
  onRepair,
}: {
  state: ManagedRuntimeState | null;
  installPhase: ManagedRuntimeInstallPhase | null;
  isInstallActive: boolean;
  isInstalling: boolean;
  modelSizeBytes: number | null;
  onInstall: () => void;
  onRepair: () => void;
}) {
  if (isInstallActive) return null;

  if (state === "not-installed") {
    const sizeLabel = modelSizeBytes
      ? `约 ${formatBytes(modelSizeBytes)}`
      : "大小未知";
    return (
      <div className="flex flex-col items-start gap-2">
        <p className="text-xs text-muted-foreground">
          如果跳过了安装向导，或模型文件被删除，可以在这里重新下载。
        </p>
        <Button
          size="sm"
          variant="outline"
          onClick={onInstall}
          disabled={isInstalling || installPhase === "preflight"}
        >
          {isInstalling ? (
            <LoaderCircle className="size-4 animate-spin" />
          ) : (
            <Download className="size-4" />
          )}
          下载本地模型（{sizeLabel}）
        </Button>
      </div>
    );
  }

  if (
    state === "installed" ||
    state === "stopped" ||
    state === "failed" ||
    state === "ready"
  ) {
    return (
      <Button
        variant="outline"
        size="sm"
        onClick={onRepair}
        disabled={isInstalling}
        className="w-fit"
      >
        <RefreshCw className="size-4" /> 校验并修复模型
      </Button>
    );
  }

  return null;
}

// ─── Header badge ─────────────────────────────────────────────────────────────

function RuntimeBadge({
  state,
  isInstallActive,
}: {
  state: ManagedRuntimeState | null;
  isInstallActive: boolean;
}) {
  if (isInstallActive) {
    return (
      <Badge
        variant="outline"
        className="gap-1 border-transparent bg-amber-500/12 text-amber-800 ring-1 ring-inset ring-black/5 dark:ring-white/6 dark:text-amber-300"
      >
        <LoaderCircle className="size-3 animate-spin" /> 安装中
      </Badge>
    );
  }
  if (state === "ready") {
    return (
      <Badge
        variant="outline"
        className="gap-1 border-transparent bg-emerald-500/12 text-emerald-700 ring-1 ring-inset ring-black/5 dark:ring-white/6 dark:text-emerald-300"
      >
        <CheckCircle2 className="size-3" /> 运行中
      </Badge>
    );
  }
  if (state === "starting") {
    return (
      <Badge
        variant="outline"
        className="gap-1 border-transparent bg-cyan-500/12 text-cyan-700 ring-1 ring-inset ring-black/5 dark:ring-white/6 dark:text-cyan-300"
      >
        <LoaderCircle className="size-3 animate-spin" /> 启动中
      </Badge>
    );
  }
  return null;
}

// ─── Proxy, model info, logs ──────────────────────────────────────────────────

function showProxyInput(
  state: ManagedRuntimeState | null,
  isInstallActive: boolean
): boolean {
  if (isInstallActive) return true;
  return state === "not-installed" || state === "failed";
}

function DownloadProxyField({ disabled }: { disabled: boolean }) {
  const proxyUrl = useRosettaStore((s) => s.downloadProxy.url);
  const setProxyUrl = useRosettaStore((s) => s.setDownloadProxyUrl);

  return (
    <div className="flex flex-col gap-1.5 rounded-md border bg-muted/30 p-3">
      <div className="flex items-baseline justify-between gap-3">
        <Label htmlFor="managed-rwkv-download-proxy" className="text-xs font-medium">
          下载代理（可选）
        </Label>
        <span className="text-[11px] text-muted-foreground">只影响模型下载</span>
      </div>
      <Input
        id="managed-rwkv-download-proxy"
        type="text"
        placeholder="例如 http://127.0.0.1:7897 或留空"
        value={proxyUrl}
        disabled={disabled}
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => setProxyUrl(e.target.value)}
        className="h-8 font-mono text-xs"
      />
    </div>
  );
}

function ModelInfoRows({ status }: { status: ManagedRuntimeStatus }) {
  if (!status.profile && !status.paths) return null;

  const rows: Array<{ label: string; value: string }> = [];
  if (status.profile) {
    rows.push(
      { label: "模型文件", value: `${status.profile.modelFilename} (${formatBytes(status.profile.modelSizeBytes)})` },
      { label: "校验", value: `SHA-256 ${status.profile.modelSha256.slice(0, 16)}…` },
      { label: "运行后端", value: `${status.profile.backend} (${status.profile.providerId})` }
    );
  }
  if (status.paths) {
    rows.push(
      { label: "模型路径", value: status.paths.modelFile },
      { label: "日志路径", value: status.paths.logsDir }
    );
  }
  if (status.process.baseUrl) {
    rows.push({ label: "监听地址", value: status.process.baseUrl });
  }
  if (status.process.pid) {
    rows.push({ label: "进程 PID", value: String(status.process.pid) });
  }

  if (rows.length === 0) return null;

  return (
    <dl className="grid min-w-0 gap-1.5 text-xs">
      {rows.map((row) => (
        <div
          key={row.label}
          className="grid min-w-0 grid-cols-[5rem_minmax(0,1fr)] gap-3"
        >
          <dt className="text-muted-foreground">{row.label}</dt>
          <dd className="truncate font-mono text-[11px] text-foreground/70">{row.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function LogsSummaryBlock({ logs }: { logs: ManagedRuntimeLogsSummary | null }) {
  if (!logs) {
    return <p className="text-xs text-muted-foreground">正在读取日志</p>;
  }
  if (logs.logTail.length === 0) {
    return <p className="text-xs text-muted-foreground">{logs.message}</p>;
  }
  return (
    <div className="max-h-40 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-[11px] leading-relaxed text-muted-foreground">
      {logs.logTail.map((line, idx) => (
        <div key={idx} className="whitespace-pre-wrap break-all">
          {line}
        </div>
      ))}
    </div>
  );
}

// ─── Install progress bar ─────────────────────────────────────────────────────

function InstallProgressRow({
  percent,
  message,
  speedBytesPerSec,
}: {
  percent: number;
  message: string;
  speedBytesPerSec: number;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span className="truncate">{message}</span>
        <span className="shrink-0 tabular-nums">
          {percent}%{speedBytesPerSec > 0 ? ` · ${formatSpeed(speedBytesPerSec)}` : ""}
        </span>
      </div>
      <div className="relative h-1.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          className="absolute inset-y-0 left-0 rounded-full bg-primary transition-[width] duration-200"
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

function installPercent(
  progress: ReturnType<typeof useManagedRwkvRuntime>["progress"]
): number {
  if (!progress || progress.bytesTotal === 0) return 0;
  return Math.min(100, Math.floor((progress.bytesDone * 100) / progress.bytesTotal));
}

// ─── Utilities ────────────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let i = 1; i < units.length && value >= 1024; i++) {
    value /= 1024;
    unit = units[i];
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return "—";
  return `${formatBytes(bytesPerSec)}/s`;
}
