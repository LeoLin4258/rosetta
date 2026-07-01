import { useState } from "react";
import {
  AlertCircle,
  AlertTriangle,
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
import { selectManagedRuntimeProfileStatus } from "@/lib/managedRuntimeSelection";
import { cn } from "@/lib/utils";
import { useManagedRwkvRuntime } from "@/lib/useManagedRwkvRuntime";
import { useRosettaStore } from "@/store/useRosettaStore";
import type {
  ManagedRuntimeInstallPhase,
  ManagedRuntimeLogsSummary,
  ManagedRuntimeProfileStatus,
  ManagedRuntimeState,
} from "@/types/rosetta";

const INSTALL_ACTIVE_PHASES: ReadonlySet<ManagedRuntimeInstallPhase> = new Set([
  "preflight",
  "downloading",
  "verifying",
  "extracting",
  "writing-manifest",
]);

type LocalRwkvPanelProps = {
  className?: string;
  isTranslationRunning?: boolean;
};

export function LocalRwkvPanel({
  className,
  isTranslationRunning = false,
}: LocalRwkvPanelProps) {
  const rt = useManagedRwkvRuntime();
  const status = rt.status;
  const selectedProfileId = useRosettaStore(
    (state) => state.rwkv.managedRuntimeProfileId
  );
  const updateRwkvConfig = useRosettaStore((state) => state.updateRwkvConfig);
  const [detailsOpenByProfileId, setDetailsOpenByProfileId] = useState<
    Record<string, boolean>
  >({});
  const [logsByProfileId, setLogsByProfileId] = useState<
    Record<string, ManagedRuntimeLogsSummary | null>
  >({});
  const [logsLoadingProfileId, setLogsLoadingProfileId] = useState<string | null>(
    null
  );
  const [actionProfileId, setActionProfileId] = useState<string | null>(null);

  const profileStatuses = status?.profileStatuses ?? [];
  const selectedStatus = selectManagedRuntimeProfileStatus(
    status,
    selectedProfileId
  );
  const activeProfileId =
    selectedStatus?.profile.id ?? selectedProfileId ?? status?.profile?.id ?? null;
  const installPhase = rt.progress?.phase ?? null;
  const isInstallActive = !!installPhase && INSTALL_ACTIVE_PHASES.has(installPhase);
  const actionsDisabled =
    isTranslationRunning ||
    rt.isInstalling ||
    rt.isStarting ||
    rt.isStopping ||
    isInstallActive;

  async function activateProfile(profileId: string) {
    if (isTranslationRunning || profileId === activeProfileId) {
      return;
    }
    updateRwkvConfig({ managedRuntimeProfileId: profileId });
    await rt.refreshStatus(profileId);
  }

  async function installProfile(profileId: string, repair: boolean) {
    if (actionsDisabled) {
      return;
    }
    setActionProfileId(profileId);
    try {
      await rt.install({ profileId, repair });
    } finally {
      setActionProfileId(null);
    }
  }

  async function startProfile(profileId: string) {
    if (actionsDisabled) {
      return;
    }
    setActionProfileId(profileId);
    updateRwkvConfig({ managedRuntimeProfileId: profileId });
    try {
      await rt.start(profileId);
    } finally {
      setActionProfileId(null);
    }
  }

  async function stopProfile(profileId: string) {
    if (actionsDisabled) {
      return;
    }
    setActionProfileId(profileId);
    try {
      await rt.stop(profileId);
    } finally {
      setActionProfileId(null);
    }
  }

  async function cancelInstall() {
    await rt.cancelInstall();
  }

  async function setProfileDetailsOpen(profileId: string, nextOpen: boolean) {
    setDetailsOpenByProfileId((current) => ({
      ...current,
      [profileId]: nextOpen,
    }));

    if (!nextOpen || logsByProfileId[profileId] !== undefined) {
      return;
    }

    setLogsLoadingProfileId(profileId);
    try {
      const logs = await rt.readLogs(profileId);
      setLogsByProfileId((current) => ({
        ...current,
        [profileId]: logs,
      }));
    } finally {
      setLogsLoadingProfileId(null);
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
              管理本地翻译运行时
            </h3>
            <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
              安装、切换并检查 Rosetta 管理的本机翻译后端。翻译请求只发送到本机服务。
            </p>
          </div>
        </div>
        <RuntimeBadge status={selectedStatus} isInstallActive={isInstallActive} />
      </div>

      <div className="flex flex-col gap-4 border-t pt-4">
        {isTranslationRunning && (
          <div className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-sm text-amber-800 dark:text-amber-300">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" />
            <span>
              正在翻译。完成或暂停当前任务后，才能切换、启动、停止或修复本地运行时。
            </span>
          </div>
        )}

        {rt.lastError && (
          <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
            <AlertCircle className="mt-0.5 size-4 shrink-0" />
            <span className="break-all">{rt.lastError}</span>
          </div>
        )}

        {isInstallActive && (
          <InstallProgressRow
            percent={installPercent(rt.progress)}
            message={rt.progress?.message ?? ""}
            speedBytesPerSec={rt.progress?.speedBytesPerSec ?? 0}
            onCancel={cancelInstall}
          />
        )}

        {profileStatuses.length > 0 ? (
          <div className="grid gap-3">
            {profileStatuses.map((profileStatus) => (
              <RuntimeProfileCard
                key={profileStatus.profile.id}
                status={profileStatus}
                isSelected={profileStatus.profile.id === activeProfileId}
                isActionTarget={profileStatus.profile.id === actionProfileId}
                actionsDisabled={actionsDisabled}
                selectionDisabled={isTranslationRunning}
                detailsOpen={
                  detailsOpenByProfileId[profileStatus.profile.id] ?? false
                }
                logs={logsByProfileId[profileStatus.profile.id] ?? null}
                logsLoading={
                  logsLoadingProfileId === profileStatus.profile.id &&
                  logsByProfileId[profileStatus.profile.id] === undefined
                }
                onActivate={() => void activateProfile(profileStatus.profile.id)}
                onInstall={() => void installProfile(profileStatus.profile.id, false)}
                onRepair={() => void installProfile(profileStatus.profile.id, true)}
                onStart={() => void startProfile(profileStatus.profile.id)}
                onStop={() => void stopProfile(profileStatus.profile.id)}
                onDetailsOpenChange={(open) =>
                  void setProfileDetailsOpen(profileStatus.profile.id, open)
                }
              />
            ))}
          </div>
        ) : (
          <div className="flex items-center gap-2 rounded-md border bg-muted/20 p-3 text-sm text-muted-foreground">
            <LoaderCircle className="size-4 animate-spin" />
            正在读取本地运行时状态
          </div>
        )}

        {showProxyInput(profileStatuses, isInstallActive) && (
          <DownloadProxyField disabled={isInstallActive} />
        )}
      </div>
    </section>
  );
}

function RuntimeProfileCard({
  status,
  isSelected,
  isActionTarget,
  actionsDisabled,
  selectionDisabled,
  detailsOpen,
  logs,
  logsLoading,
  onActivate,
  onInstall,
  onRepair,
  onStart,
  onStop,
  onDetailsOpenChange,
}: {
  status: ManagedRuntimeProfileStatus;
  isSelected: boolean;
  isActionTarget: boolean;
  actionsDisabled: boolean;
  selectionDisabled: boolean;
  detailsOpen: boolean;
  logs: ManagedRuntimeLogsSummary | null;
  logsLoading: boolean;
  onActivate: () => void;
  onInstall: () => void;
  onRepair: () => void;
  onStart: () => void;
  onStop: () => void;
  onDetailsOpenChange: (open: boolean) => void;
}) {
  const isUnsupported = status.state === "unsupported";
  const isBusy = isActionTarget && actionsDisabled;
  const summary = resolveStatus(status.state, status);

  return (
    <article
      className={cn(
        "rounded-lg border bg-card/70 p-4 transition-colors",
        isSelected
          ? "border-foreground/20 ring-1 ring-foreground/10"
          : "border-border"
      )}
    >
      <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
        <div className="min-w-0 space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="text-sm font-semibold tracking-normal">
              {status.profile.runtimeLabel}
            </h4>
            {status.profile.recommended && (
              <Badge variant="secondary" className="h-5 px-1.5 text-[11px]">
                推荐
              </Badge>
            )}
            {isSelected && (
              <Badge variant="outline" className="h-5 px-1.5 text-[11px]">
                当前
              </Badge>
            )}
            <StateBadge state={status.state} />
          </div>
          <p className="max-w-2xl text-xs leading-5 text-muted-foreground">
            {runtimeDescription(status)}
          </p>
          {status.profile.runtimeWarning && (
            <p className="max-w-2xl text-xs leading-5 text-amber-700 dark:text-amber-300">
              {status.profile.runtimeWarning}
            </p>
          )}
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
            <span>{status.profile.backend}</span>
            <span>{status.profile.providerId}</span>
            <span>{formatBytes(status.profile.modelSizeBytes)}</span>
            {status.hardware.gpuName && <span>{status.hardware.gpuName}</span>}
            {status.hardware.computeCapability && (
              <span>SM {status.hardware.computeCapability}</span>
            )}
          </div>
        </div>

        <div className="flex shrink-0 flex-wrap gap-2 lg:justify-end">
          <Button
            type="button"
            size="sm"
            variant={isSelected ? "secondary" : "outline"}
            disabled={isSelected || isUnsupported || selectionDisabled}
            onClick={onActivate}
          >
            {isSelected ? "已设为当前" : "设为当前"}
          </Button>
          <RuntimeActionButtons
            state={status.state}
            isBusy={isBusy}
            disabled={actionsDisabled || isUnsupported}
            onInstall={onInstall}
            onRepair={onRepair}
            onStart={onStart}
            onStop={onStop}
          />
        </div>
      </div>

      <div className="mt-4 flex flex-col gap-2 border-t pt-3">
        <div className="flex items-start gap-2">
          {summary.spinning ? (
            <LoaderCircle className="mt-0.5 size-3.5 shrink-0 animate-spin text-muted-foreground" />
          ) : (
            <div className={cn("mt-1.5 size-2 shrink-0 rounded-full", summary.dot)} />
          )}
          <div className="min-w-0">
            <p className="text-xs font-medium">{summary.label}</p>
            {summary.sub && (
              <p className="mt-1 text-xs leading-5 text-muted-foreground">
                {summary.sub}
              </p>
            )}
          </div>
        </div>

        {!isUnsupported && (
          <Collapsible open={detailsOpen} onOpenChange={onDetailsOpenChange}>
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
              <div className="grid gap-4 border-t pt-4 lg:grid-cols-[minmax(0,1fr)_minmax(18rem,0.8fr)]">
                <ModelInfoRows status={status} />
                <div className="flex min-w-0 flex-col gap-2">
                  <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                    <TerminalSquare className="size-3.5" />
                    运行日志
                  </div>
                  <LogsSummaryBlock logs={logs} isLoading={logsLoading} />
                </div>
              </div>
            </CollapsibleContent>
          </Collapsible>
        )}
      </div>
    </article>
  );
}

function RuntimeActionButtons({
  state,
  isBusy,
  disabled,
  onInstall,
  onRepair,
  onStart,
  onStop,
}: {
  state: ManagedRuntimeState;
  isBusy: boolean;
  disabled: boolean;
  onInstall: () => void;
  onRepair: () => void;
  onStart: () => void;
  onStop: () => void;
}) {
  if (state === "not-installed") {
    return (
      <Button type="button" size="sm" onClick={onInstall} disabled={disabled}>
        {isBusy ? (
          <LoaderCircle className="size-4 animate-spin" />
        ) : (
          <Download className="size-4" />
        )}
        下载安装
      </Button>
    );
  }

  if (state === "ready") {
    return (
      <>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onRepair}
          disabled={disabled}
        >
          <RefreshCw className="size-4" />
          校验修复
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onStop}
          disabled={disabled}
        >
          {isBusy ? (
            <LoaderCircle className="size-4 animate-spin" />
          ) : (
            <Square className="size-4" />
          )}
          停止
        </Button>
      </>
    );
  }

  if (state === "installed" || state === "stopped" || state === "failed") {
    return (
      <>
        <Button type="button" size="sm" onClick={onStart} disabled={disabled}>
          {isBusy ? (
            <LoaderCircle className="size-4 animate-spin" />
          ) : (
            <Play className="size-4" />
          )}
          启动
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onRepair}
          disabled={disabled}
        >
          <RefreshCw className="size-4" />
          校验修复
        </Button>
      </>
    );
  }

  if (state === "starting") {
    return (
      <Button type="button" size="sm" disabled>
        <LoaderCircle className="size-4 animate-spin" />
        正在启动
      </Button>
    );
  }

  return null;
}

function RuntimeBadge({
  status,
  isInstallActive,
}: {
  status: ManagedRuntimeProfileStatus | null;
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
  if (!status) return null;
  if (status.state === "ready") {
    return (
      <Badge
        variant="outline"
        className="gap-1 border-transparent bg-emerald-500/12 text-emerald-700 ring-1 ring-inset ring-black/5 dark:ring-white/6 dark:text-emerald-300"
      >
        <CheckCircle2 className="size-3" /> 当前运行中
      </Badge>
    );
  }
  if (status.state === "starting") {
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

function StateBadge({ state }: { state: ManagedRuntimeState }) {
  const label = stateLabel(state);
  if (!label) return null;
  return (
    <Badge variant="outline" className="h-5 px-1.5 text-[11px] font-normal">
      {label}
    </Badge>
  );
}

function resolveStatus(
  state: ManagedRuntimeState,
  status: ManagedRuntimeProfileStatus
): { dot: string; label: string; sub?: string; spinning?: boolean } {
  switch (state) {
    case "ready":
      return {
        dot: status.process.cpuFallback ? "bg-amber-400" : "bg-emerald-500",
        label: status.process.cpuFallback
          ? "运行中，当前为 CPU 回退模式"
          : "运行中，可用于本地翻译",
        sub: status.process.baseUrl
          ? `监听 ${status.process.baseUrl}`
          : undefined,
      };
    case "starting":
      return {
        dot: "bg-blue-500",
        label: "正在启动本地服务",
        spinning: true,
      };
    case "installed":
      return {
        dot: "bg-amber-400",
        label: "已安装，启动后可以用于翻译",
      };
    case "stopped":
      return {
        dot: "bg-muted-foreground/50",
        label: "已停止",
      };
    case "failed":
      return {
        dot: "bg-destructive",
        label: "启动失败",
        sub: status.process.lastError ?? status.message,
      };
    case "unsupported":
      return {
        dot: "bg-muted-foreground/30",
        label: "当前设备不支持",
        sub: status.hardware.message,
      };
    case "not-installed":
      return {
        dot: "bg-muted-foreground/30",
        label: "尚未安装",
        sub: status.installPlan.message,
      };
    default:
      return {
        dot: "bg-muted-foreground/30",
        label: "正在读取状态",
        spinning: true,
      };
  }
}

function runtimeDescription(status: ManagedRuntimeProfileStatus): string {
  if (!status.hardware.supported) {
    return status.hardware.message;
  }
  if (status.profile.hardwareRequirement) {
    return status.profile.hardwareRequirement;
  }
  return "本机运行的 Rosetta 托管翻译后端。";
}

function showProxyInput(
  profileStatuses: ManagedRuntimeProfileStatus[],
  isInstallActive: boolean
): boolean {
  if (isInstallActive) return true;
  return profileStatuses.some(
    (status) => status.state === "not-installed" || status.state === "failed"
  );
}

function DownloadProxyField({ disabled }: { disabled: boolean }) {
  const proxyUrl = useRosettaStore((state) => state.downloadProxy.url);
  const setProxyUrl = useRosettaStore((state) => state.setDownloadProxyUrl);

  return (
    <div className="flex flex-col gap-1.5 rounded-md border bg-muted/30 p-3">
      <div className="flex items-baseline justify-between gap-3">
        <Label
          htmlFor="managed-rwkv-download-proxy"
          className="text-xs font-medium"
        >
          下载代理（可选）
        </Label>
        <span className="text-[11px] text-muted-foreground">只影响模型下载</span>
      </div>
      <Input
        id="managed-rwkv-download-proxy"
        type="text"
        placeholder="例如 http://127.0.0.1:7897，留空自动检测"
        value={proxyUrl}
        disabled={disabled}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => setProxyUrl(event.target.value)}
        className="h-8 font-mono text-xs"
      />
    </div>
  );
}

function ModelInfoRows({ status }: { status: ManagedRuntimeProfileStatus }) {
  const rows: Array<{ label: string; value: string }> = [
    {
      label: "Profile",
      value: status.profile.id,
    },
    {
      label: "模型文件",
      value: `${status.profile.modelFilename} (${formatBytes(
        status.profile.modelSizeBytes
      )})`,
    },
    {
      label: "校验",
      value: `SHA-256 ${status.profile.modelSha256.slice(0, 16)}...`,
    },
    {
      label: "接口",
      value: status.profile.batchChatPath,
    },
    {
      label: "模型路径",
      value: status.paths.modelFile,
    },
    {
      label: "日志路径",
      value: status.paths.logsDir,
    },
  ];

  if (status.paths.runtimeDir) {
    rows.push({ label: "运行包", value: status.paths.runtimeDir });
  }
  if (status.process.pid) {
    rows.push({ label: "进程 PID", value: String(status.process.pid) });
  }

  return (
    <dl className="grid min-w-0 gap-1.5 text-xs">
      {rows.map((row) => (
        <div
          key={row.label}
          className="grid min-w-0 grid-cols-[5rem_minmax(0,1fr)] gap-3"
        >
          <dt className="text-muted-foreground">{row.label}</dt>
          <dd className="truncate font-mono text-[11px] text-foreground/70">
            {row.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function LogsSummaryBlock({
  logs,
  isLoading,
}: {
  logs: ManagedRuntimeLogsSummary | null;
  isLoading: boolean;
}) {
  if (isLoading) {
    return <p className="text-xs text-muted-foreground">正在读取日志</p>;
  }
  if (!logs) {
    return <p className="text-xs text-muted-foreground">展开后读取日志。</p>;
  }
  if (logs.logTail.length === 0) {
    return <p className="text-xs text-muted-foreground">{logs.message}</p>;
  }
  return (
    <div className="max-h-40 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-[11px] leading-relaxed text-muted-foreground">
      {logs.logTail.map((line, index) => (
        <div key={`${index}-${line}`} className="whitespace-pre-wrap break-all">
          {line}
        </div>
      ))}
    </div>
  );
}

function InstallProgressRow({
  percent,
  message,
  speedBytesPerSec,
  onCancel,
}: {
  percent: number;
  message: string;
  speedBytesPerSec: number;
  onCancel: () => void;
}) {
  return (
    <div className="flex flex-col gap-3 rounded-md border bg-muted/20 p-3">
      <div className="flex items-center justify-between gap-3 text-xs text-muted-foreground">
        <span className="truncate">{message || "正在安装本地运行时"}</span>
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
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onCancel}
        className="w-fit"
      >
        <X className="size-4" />
        取消下载
      </Button>
    </div>
  );
}

function installPercent(
  progress: ReturnType<typeof useManagedRwkvRuntime>["progress"]
): number {
  if (!progress || progress.bytesTotal === 0) return 0;
  return Math.min(100, Math.floor((progress.bytesDone * 100) / progress.bytesTotal));
}

function stateLabel(state: ManagedRuntimeState): string | null {
  switch (state) {
    case "ready":
      return "运行中";
    case "starting":
      return "启动中";
    case "installed":
      return "已安装";
    case "stopped":
      return "已停止";
    case "failed":
      return "失败";
    case "unsupported":
      return "不支持";
    case "not-installed":
      return "未安装";
    default:
      return null;
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return "-";
  return `${formatBytes(bytesPerSec)}/s`;
}
