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
  Card,
  CardContent,
} from "@/components/ui/card";
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
    <section className={cn("flex flex-col gap-3", className)} id="local-rwkv">
      {/* Section header */}
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
            <Cpu className="size-4" />
          </div>
          <div className="min-w-0">
            <h2 className="text-lg font-semibold tracking-normal">本地翻译引擎</h2>
            <p className="mt-1 text-sm text-muted-foreground">
              一键下载与启动本地翻译模型，全程离线、文档不离开本机。
            </p>
          </div>
        </div>
        <RuntimeBadge state={state} isInstallActive={isInstallActive} />
      </div>

      <Card>
        <CardContent className="flex flex-col gap-4 py-5">
          {/* Status indicator — one dot + one line */}
          <StatusRow
            state={state}
            status={status}
            isInstallActive={isInstallActive}
          />

          {/* Download progress (only when actively installing) */}
          {isInstallActive && (
            <InstallProgressRow
              percent={installPercent(rt.progress)}
              message={rt.progress?.message ?? ""}
              speedBytesPerSec={rt.progress?.speedBytesPerSec ?? 0}
            />
          )}

          {/* Primary runtime controls (start/stop/cancel) — NOT install */}
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

          {/* Proxy field — only needed when a download might happen */}
          {showProxyInput(state, isInstallActive) && (
            <DownloadProxyField disabled={isInstallActive} />
          )}

          {/* Error display */}
          {rt.lastError && (
            <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
              <AlertCircle className="mt-0.5 size-4 shrink-0" />
              <span className="break-all">{rt.lastError}</span>
            </div>
          )}

          {/* Details collapsible — technical info + install/repair */}
          {!isUnsupported && (
            <Collapsible open={detailsOpen} onOpenChange={openDetails}>
              <CollapsibleTrigger asChild>
                <button
                  type="button"
                  className="-ml-1 flex items-center gap-1 rounded px-1 py-0.5 text-xs text-muted-foreground/60 transition-colors hover:text-muted-foreground"
                >
                  <ChevronDown
                    className={cn(
                      "size-3.5 transition-transform",
                      detailsOpen && "rotate-180"
                    )}
                  />
                  详细信息
                </button>
              </CollapsibleTrigger>
              <CollapsibleContent>
                <div className="mt-3 flex flex-col gap-4 rounded-md border bg-muted/20 p-4">
                  {/* Install / repair actions (hidden from main path) */}
                  <RepairActions
                    state={state}
                    installPhase={installPhase}
                    isInstallActive={isInstallActive}
                    isInstalling={rt.isInstalling}
                    onInstall={() => void rt.install({ repair: false })}
                    onRepair={() => void rt.install({ repair: true })}
                  />

                  {/* Model technical info */}
                  {status && <ModelInfoRows status={status} />}

                  {/* Logs */}
                  {status && (
                    <div className="flex flex-col gap-2">
                      <div className="flex items-center gap-1.5 text-xs text-muted-foreground/60">
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
        </CardContent>
      </Card>
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
  const { dot, label, sub } = resolveStatus(state, status, isInstallActive);
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2.5">
        <div className={cn("size-2 shrink-0 rounded-full", dot)} />
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
): { dot: string; label: string; sub?: string } {
  if (isInstallActive) {
    return {
      dot: "bg-blue-500 animate-pulse",
      label: "正在下载安装翻译模型…",
      sub: "首次下载约 1.3 GB，完成后无需再联网。",
    };
  }
  switch (state) {
    case "ready":
      return {
        dot: "bg-emerald-500",
        label: "本地翻译运行中",
        sub: status?.process.baseUrl
          ? `本地翻译服务正在运行，数据不会离开你的设备。`
          : undefined,
      };
    case "starting":
      return {
        dot: "bg-blue-500 animate-pulse",
        label: "正在加载模型，请稍候…",
      };
    case "installed":
    case "stopped":
      return {
        dot: "bg-amber-400",
        label: "模型已就绪，等待启动",
      };
    case "failed":
      return {
        dot: "bg-destructive",
        label: "启动失败，可展开「详细信息」查看原因",
      };
    case "unsupported":
      return {
        dot: "bg-muted-foreground/40",
        label: "当前设备不支持本地翻译",
        sub: "本地翻译功能仅支持 Mac（M1/M2/M3/M4 芯片）。如需翻译，可在下方连接远程翻译服务。",
      };
    case "not-installed":
      return {
        dot: "bg-muted-foreground/30",
        label: "本地翻译引擎尚未下载",
        sub: '展开下方"详细信息"可手动安装，或重启 Rosetta 进入安装向导。',
      };
    default:
      return {
        dot: "bg-muted-foreground/30",
        label: "正在检测本地翻译状态…",
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
          可随时取消，下次会从中断处继续。
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
        <LoaderCircle className="size-4 animate-spin" /> 启动中
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
  onInstall,
  onRepair,
}: {
  state: ManagedRuntimeState | null;
  installPhase: ManagedRuntimeInstallPhase | null;
  isInstallActive: boolean;
  isInstalling: boolean;
  onInstall: () => void;
  onRepair: () => void;
}) {
  if (isInstallActive) return null;

  if (state === "not-installed") {
    return (
      <div className="flex flex-col gap-2">
        <p className="text-xs text-muted-foreground">
          首次安装建议通过启动向导进行。如果模型被意外删除，也可在此手动重新下载。
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
          下载翻译模型（约 1.3 GB）
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
        <RefreshCw className="size-4" /> 重新校验 / 修复模型
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
      <Badge variant="secondary" className="gap-1">
        <LoaderCircle className="size-3 animate-spin" /> 安装中
      </Badge>
    );
  }
  if (state === "ready") {
    return (
      <Badge variant="secondary" className="gap-1 text-emerald-600 dark:text-emerald-400">
        <CheckCircle2 className="size-3" /> 运行中
      </Badge>
    );
  }
  if (state === "starting") {
    return (
      <Badge variant="secondary" className="gap-1">
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
        <span className="text-[11px] text-muted-foreground">仅用于下载模型</span>
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
      { label: "后端", value: `${status.profile.backend} (${status.profile.providerId})` }
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
    <dl className="grid gap-1.5 text-xs">
      {rows.map((row) => (
        <div key={row.label} className="grid grid-cols-[6rem_1fr] gap-3">
          <dt className="text-muted-foreground">{row.label}</dt>
          <dd className="truncate font-mono text-[11px] text-foreground/70">{row.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function LogsSummaryBlock({ logs }: { logs: ManagedRuntimeLogsSummary | null }) {
  if (!logs) {
    return <p className="text-xs text-muted-foreground">日志读取中…</p>;
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
