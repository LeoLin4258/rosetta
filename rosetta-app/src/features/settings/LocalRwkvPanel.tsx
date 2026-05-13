import { useMemo, useState } from "react";
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
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import { useManagedRwkvRuntime } from "@/lib/useManagedRwkvRuntime";
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

type LocalRwkvPanelProps = {
  /** Optional ref-style hook for opening Settings from a banner / job page. */
  className?: string;
};

export function LocalRwkvPanel({ className }: LocalRwkvPanelProps) {
  const rt = useManagedRwkvRuntime();
  const status = rt.status;
  const [logs, setLogs] = useState<ManagedRuntimeLogsSummary | null>(null);
  const [logsOpen, setLogsOpen] = useState(false);

  const state: ManagedRuntimeState | null = status?.state ?? null;
  const isUnsupported = state === "unsupported";
  const installPhase = rt.progress?.phase ?? null;
  const isInstallActive =
    !!installPhase && INSTALL_ACTIVE_PHASES.has(installPhase);

  const headerBadge = useMemo(() => makeHeaderBadge(state, isInstallActive), [
    state,
    isInstallActive,
  ]);

  async function openLogs(next: boolean) {
    setLogsOpen(next);
    if (next && !logs) {
      const summary = await rt.readLogs();
      setLogs(summary);
    }
  }

  return (
    <section className={cn("flex flex-col gap-3", className)} id="local-rwkv">
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
            <Cpu className="size-4" />
          </div>
          <div className="min-w-0">
            <h2 className="text-lg font-semibold tracking-normal">
              本地 RWKV 翻译
            </h2>
            <p className="mt-1 text-sm text-muted-foreground">
              一键下载与启动本地翻译模型，全程离线、文档不离开本机。
            </p>
          </div>
        </div>
        {headerBadge}
      </div>

      <Card>
        <CardHeader>
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <CardTitle>{statePanelTitle(state, isInstallActive)}</CardTitle>
              <CardDescription>
                {statePanelDescription(status, isInstallActive)}
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {isInstallActive ? (
            <InstallProgressRow
              percent={installPercent(rt.progress)}
              message={rt.progress?.message ?? ""}
              speedBytesPerSec={rt.progress?.speedBytesPerSec ?? 0}
            />
          ) : null}

          <PrimaryActionRow
            state={state}
            installPhase={installPhase}
            isInstallActive={isInstallActive}
            isInstalling={rt.isInstalling}
            isStarting={rt.isStarting}
            isStopping={rt.isStopping}
            onInstall={() => void rt.install({ repair: false })}
            onRepair={() => void rt.install({ repair: true })}
            onStart={() => void rt.start()}
            onStop={() => void rt.stop()}
            onCancel={() => void rt.cancelInstall()}
            isUnsupported={isUnsupported}
          />

          {rt.lastError ? (
            <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
              <AlertCircle className="mt-0.5 size-4 shrink-0" />
              <span className="break-all">{rt.lastError}</span>
            </div>
          ) : null}

          {status && !isUnsupported ? (
            <>
              <Separator />
              <ModelInfoRows status={status} />
            </>
          ) : null}

          {status && !isUnsupported ? (
            <Collapsible open={logsOpen} onOpenChange={openLogs}>
              <CollapsibleTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="-ml-2 h-7 gap-1 px-2 text-xs text-muted-foreground hover:text-foreground"
                >
                  <TerminalSquare className="size-3.5" />
                  查看运行时日志摘要
                  <ChevronDown
                    className={cn(
                      "size-3.5 transition-transform",
                      logsOpen && "rotate-180"
                    )}
                  />
                </Button>
              </CollapsibleTrigger>
              <CollapsibleContent>
                <LogsSummaryBlock logs={logs} />
              </CollapsibleContent>
            </Collapsible>
          ) : null}
        </CardContent>
      </Card>
    </section>
  );
}

function makeHeaderBadge(
  state: ManagedRuntimeState | null,
  isInstallActive: boolean
) {
  if (isInstallActive) {
    return (
      <Badge variant="secondary" className="gap-1">
        <LoaderCircle className="size-3 animate-spin" /> 正在安装
      </Badge>
    );
  }
  switch (state) {
    case "ready":
      return (
        <Badge variant="secondary" className="gap-1 text-emerald-600 dark:text-emerald-400">
          <CheckCircle2 className="size-3" /> 运行中
        </Badge>
      );
    case "starting":
      return (
        <Badge variant="secondary" className="gap-1">
          <LoaderCircle className="size-3 animate-spin" /> 启动中
        </Badge>
      );
    case "installed":
      return <Badge variant="outline">已安装 · 未启动</Badge>;
    case "stopped":
      return <Badge variant="outline">已停止</Badge>;
    case "failed":
      return <Badge variant="destructive">故障</Badge>;
    case "unsupported":
      return <Badge variant="outline">仅支持 Apple Silicon</Badge>;
    case "not-installed":
      return <Badge variant="outline">未安装</Badge>;
    default:
      return null;
  }
}

function statePanelTitle(
  state: ManagedRuntimeState | null,
  isInstallActive: boolean
): string {
  if (isInstallActive) return "正在准备本地翻译模型";
  switch (state) {
    case "ready":
      return "本地翻译已就绪";
    case "starting":
      return "正在加载模型…";
    case "installed":
      return "已安装，未启动";
    case "stopped":
      return "运行时已停止";
    case "failed":
      return "运行时遇到问题";
    case "unsupported":
      return "暂不支持当前设备";
    case "not-installed":
      return "尚未安装本地翻译";
    default:
      return "正在检查本地翻译状态…";
  }
}

function statePanelDescription(
  status: ManagedRuntimeStatus | null,
  isInstallActive: boolean
): string {
  if (isInstallActive) {
    return "首次下载约 1.3 GB；下载完成后无需再联网。";
  }
  if (status?.state === "unsupported") {
    return "本地 RWKV 仅在 macOS Apple Silicon（M1/M2/M3/M4）上运行；其他设备请使用下方的外部翻译 API。";
  }
  if (status?.state === "not-installed") {
    return "点击下载并安装翻译模型（约 1.3 GB，国内需要可用的代理）。";
  }
  if (status?.state === "ready" && status.process.baseUrl) {
    return `已绑定 ${status.process.baseUrl}，仅本机可访问。`;
  }
  return status?.message ?? "";
}

function installPercent(
  progress: ReturnType<typeof useManagedRwkvRuntime>["progress"]
): number {
  if (!progress || progress.bytesTotal === 0) return 0;
  return Math.min(100, Math.floor((progress.bytesDone * 100) / progress.bytesTotal));
}

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
          {percent}% · {formatSpeed(speedBytesPerSec)}
        </span>
      </div>
      <div className="relative h-2 w-full overflow-hidden rounded-full bg-muted">
        <div
          className="absolute inset-y-0 left-0 rounded-full bg-primary transition-[width] duration-200"
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

function PrimaryActionRow({
  state,
  installPhase,
  isInstallActive,
  isInstalling,
  isStarting,
  isStopping,
  onInstall,
  onRepair,
  onStart,
  onStop,
  onCancel,
  isUnsupported,
}: {
  state: ManagedRuntimeState | null;
  installPhase: ManagedRuntimeInstallPhase | null;
  isInstallActive: boolean;
  isInstalling: boolean;
  isStarting: boolean;
  isStopping: boolean;
  onInstall: () => void;
  onRepair: () => void;
  onStart: () => void;
  onStop: () => void;
  onCancel: () => void;
  isUnsupported: boolean;
}) {
  if (isUnsupported) {
    return (
      <p className="text-sm text-muted-foreground">
        请滚动到下方"外部翻译 API"区配置远程接口。
      </p>
    );
  }

  if (isInstallActive) {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" size="sm" onClick={onCancel}>
          <X className="size-4" /> 取消下载
        </Button>
        <span className="text-xs text-muted-foreground">
          可随时取消，下次安装将自动从断点续传。
        </span>
      </div>
    );
  }

  switch (state) {
    case "ready":
      return (
        <Button variant="outline" size="sm" onClick={onStop} disabled={isStopping}>
          <Square className="size-4" /> 停止运行时
        </Button>
      );

    case "starting":
      return (
        <Button variant="outline" size="sm" disabled>
          <LoaderCircle className="size-4 animate-spin" /> 启动中
        </Button>
      );

    case "installed":
    case "stopped":
      return (
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={onStart} disabled={isStarting}>
            {isStarting ? (
              <LoaderCircle className="size-4 animate-spin" />
            ) : (
              <Play className="size-4" />
            )}
            启动本地翻译
          </Button>
          <Button variant="ghost" size="sm" onClick={onRepair} disabled={isInstalling}>
            <RefreshCw className="size-4" /> 重新校验模型
          </Button>
        </div>
      );

    case "failed":
      return (
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={onRepair} disabled={isInstalling}>
            <RefreshCw className="size-4" /> 修复并重试
          </Button>
          <Button variant="ghost" size="sm" onClick={onStart} disabled={isStarting}>
            <Play className="size-4" /> 直接重启
          </Button>
        </div>
      );

    case "not-installed":
    default:
      return (
        <Button size="sm" onClick={onInstall} disabled={isInstalling || installPhase === "preflight"}>
          {isInstalling ? (
            <LoaderCircle className="size-4 animate-spin" />
          ) : (
            <Download className="size-4" />
          )}
          安装本地翻译模型
        </Button>
      );
  }
}

function ModelInfoRows({ status }: { status: ManagedRuntimeStatus }) {
  if (!status.profile || !status.paths) return null;
  const rows: Array<{ label: string; value: string }> = [
    {
      label: "模型",
      value: `${status.profile.modelFilename} (${formatBytes(status.profile.modelSizeBytes)})`,
    },
    {
      label: "Runtime 后端",
      value: `${status.profile.backend} (${status.profile.providerId})`,
    },
    {
      label: "校验",
      value: `SHA-256 ${status.profile.modelSha256.slice(0, 16)}…`,
    },
    {
      label: "模型位置",
      value: status.paths.modelFile,
    },
    {
      label: "日志位置",
      value: status.paths.logsDir,
    },
  ];
  if (status.process.baseUrl) {
    rows.push({ label: "监听地址", value: status.process.baseUrl });
  }
  if (status.process.pid) {
    rows.push({ label: "进程 PID", value: String(status.process.pid) });
  }

  return (
    <dl className="grid gap-1.5 text-xs">
      {rows.map((row) => (
        <div key={row.label} className="grid grid-cols-[6rem_1fr] gap-3">
          <dt className="text-muted-foreground">{row.label}</dt>
          <dd className="truncate font-mono text-[11px] text-foreground/80">
            {row.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function LogsSummaryBlock({ logs }: { logs: ManagedRuntimeLogsSummary | null }) {
  if (!logs) {
    return (
      <p className="px-1 pt-2 text-xs text-muted-foreground">日志读取中…</p>
    );
  }
  if (logs.logTail.length === 0) {
    return (
      <p className="px-1 pt-2 text-xs text-muted-foreground">{logs.message}</p>
    );
  }
  return (
    <div className="mt-2 max-h-48 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-[11px] leading-relaxed text-muted-foreground">
      {logs.logTail.map((line, idx) => (
        <div key={idx} className="whitespace-pre-wrap break-all">
          {line}
        </div>
      ))}
    </div>
  );
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

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return "—";
  return `${formatBytes(bytesPerSec)}/s`;
}
