import { useState, type ReactNode } from "react";
import {
  AlertTriangle,
  ChevronDown,
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
import {
  isLightningProfile,
  isLlamaCppProfile,
  selectManagedRuntimeProfileStatus,
} from "@/lib/managedRuntimeSelection";
import { ManagedRuntimeConnectivityPanel } from "@/features/runtime/ManagedRuntimeConnectivityPanel";
import { cn } from "@/lib/utils";
import { useManagedRwkvRuntime } from "@/lib/useManagedRwkvRuntime";
import { useRosettaStore } from "@/store/useRosettaStore";
import type {
  ManagedRuntimeConnectivityDiagnostics,
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
  const [logsOpenByProfileId, setLogsOpenByProfileId] = useState<
    Record<string, boolean>
  >({});
  const [logsByProfileId, setLogsByProfileId] = useState<
    Record<string, ManagedRuntimeLogsSummary | null>
  >({});
  const [logsLoadingProfileId, setLogsLoadingProfileId] = useState<string | null>(
    null
  );
  const [actionProfileId, setActionProfileId] = useState<string | null>(null);
  const [connectivityDiagnostics, setConnectivityDiagnostics] =
    useState<ManagedRuntimeConnectivityDiagnostics | null>(null);
  const [connectivityRepairMessage, setConnectivityRepairMessage] =
    useState<string | null>(null);

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
  const runningProfileStatus =
    profileStatuses.find((entry) => isRuntimeRunningState(entry.state)) ?? null;

  async function stopRunningProfileBeforeSwitch(profileId: string) {
    if (
      runningProfileStatus &&
      runningProfileStatus.profile.id !== profileId
    ) {
      return rt.stop(runningProfileStatus.profile.id);
    }
    return true;
  }

  async function activateProfile(profileId: string) {
    if (actionsDisabled || profileId === activeProfileId) {
      return;
    }
    setActionProfileId(profileId);
    try {
      const stopped = await stopRunningProfileBeforeSwitch(profileId);
      if (!stopped) return;
      updateRwkvConfig({ managedRuntimeProfileId: profileId });
      await rt.refreshStatus(profileId);
    } finally {
      setActionProfileId(null);
    }
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
    try {
      const stopped = await stopRunningProfileBeforeSwitch(profileId);
      if (!stopped) return;
      updateRwkvConfig({ managedRuntimeProfileId: profileId });
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

  async function diagnoseConnectivity() {
    const diagnostics = await rt.diagnoseConnectivity(activeProfileId);
    setConnectivityDiagnostics(diagnostics);
  }

  async function repairConnectivity() {
    const result = await rt.repairConnectivity(activeProfileId);
    if (!result) return;
    setConnectivityRepairMessage(result.message);
    setConnectivityDiagnostics(result.diagnostics);
    if (result.ok && activeProfileId && selectedStatus?.state === "failed") {
      await rt.start(activeProfileId);
    }
  }

  function setProfileDetailsOpen(profileId: string, nextOpen: boolean) {
    setDetailsOpenByProfileId(nextOpen ? { [profileId]: true } : {});
    if (!nextOpen) {
      setLogsOpenByProfileId({});
    }
  }

  async function setProfileLogsOpen(profileId: string, nextOpen: boolean) {
    setLogsOpenByProfileId((current) => ({
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
        "flex flex-col gap-4",
        className
      )}
      id="local-rwkv"
    >
      <div className="flex flex-col gap-3">
        {isTranslationRunning && (
          <div className="flex items-start gap-2 rounded-md border border-amber-500/25 bg-amber-500/8 px-3 py-2 text-sm text-amber-800 dark:text-amber-300">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" />
            <span>停止当前翻译后才能切换或修复本地模型。</span>
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
          <div className="overflow-hidden rounded-lg border border-border/70 bg-card">
            {profileStatuses.map((profileStatus) => (
              <RuntimeProfileRow
                key={profileStatus.profile.id}
                status={profileStatus}
                isSelected={profileStatus.profile.id === activeProfileId}
                isActionTarget={profileStatus.profile.id === actionProfileId}
                actionsDisabled={actionsDisabled}
                selectionDisabled={actionsDisabled}
                detailsOpen={
                  detailsOpenByProfileId[profileStatus.profile.id] ?? false
                }
                logs={logsByProfileId[profileStatus.profile.id] ?? null}
                logsLoading={
                  logsLoadingProfileId === profileStatus.profile.id &&
                  logsByProfileId[profileStatus.profile.id] === undefined
                }
                logsOpen={
                  logsOpenByProfileId[profileStatus.profile.id] ?? false
                }
                isRecommended={isRecommendedRuntimeProfile(
                  profileStatus,
                  profileStatuses
                )}
                onActivate={() => void activateProfile(profileStatus.profile.id)}
                onInstall={() => void installProfile(profileStatus.profile.id, false)}
                onRepair={() => void installProfile(profileStatus.profile.id, true)}
                onStart={() => void startProfile(profileStatus.profile.id)}
                onStop={() => void stopProfile(profileStatus.profile.id)}
                onDetailsOpenChange={(open) =>
                  setProfileDetailsOpen(profileStatus.profile.id, open)
                }
                onLogsOpenChange={(open) =>
                  void setProfileLogsOpen(profileStatus.profile.id, open)
                }
                connectivityPanel={
                  profileStatus.profile.id === activeProfileId &&
                  (rt.lastError ||
                    profileStatus.state === "failed" ||
                    connectivityDiagnostics) ? (
                    <ManagedRuntimeConnectivityPanel
                      diagnostics={connectivityDiagnostics}
                      isLoading={rt.isDiagnosingConnectivity}
                      isRepairing={rt.isRepairingConnectivity}
                      repairMessage={connectivityRepairMessage}
                      onDiagnose={() => void diagnoseConnectivity()}
                      onRepair={() => void repairConnectivity()}
                    />
                  ) : null
                }
              />
            ))}
          </div>
        ) : (
          <div className="flex items-center gap-2 rounded-md border border-border/70 bg-muted/20 p-3 text-sm text-muted-foreground">
            <LoaderCircle className="size-4 animate-spin" />
            正在读取本地模型状态
          </div>
        )}

        {showProxyInput(profileStatuses, isInstallActive) && (
          <DownloadProxyField disabled={isInstallActive} />
        )}
      </div>
    </section>
  );
}

function RuntimeProfileRow({
  status,
  isSelected,
  isActionTarget,
  actionsDisabled,
  selectionDisabled,
  detailsOpen,
  logs,
  logsLoading,
  logsOpen,
  isRecommended,
  onActivate,
  onInstall,
  onRepair,
  onStart,
  onStop,
  onDetailsOpenChange,
  onLogsOpenChange,
  connectivityPanel,
}: {
  status: ManagedRuntimeProfileStatus;
  isSelected: boolean;
  isActionTarget: boolean;
  actionsDisabled: boolean;
  selectionDisabled: boolean;
  detailsOpen: boolean;
  logs: ManagedRuntimeLogsSummary | null;
  logsLoading: boolean;
  logsOpen: boolean;
  isRecommended: boolean;
  onActivate: () => void;
  onInstall: () => void;
  onRepair: () => void;
  onStart: () => void;
  onStop: () => void;
  onDetailsOpenChange: (open: boolean) => void;
  onLogsOpenChange: (open: boolean) => void;
  connectivityPanel?: ReactNode;
}) {
  const [pathsOpen, setPathsOpen] = useState(false);
  const isUnsupported = status.state === "unsupported";
  const isBusy = isActionTarget && actionsDisabled;
  const summary = resolveStatus(status.state, status);
  const showSummary =
    summary.spinning ||
    !!summary.sub ||
    status.state === "failed" ||
    status.state === "unsupported";

  return (
    <article
      className={cn(
        "border-t border-border/70 px-4 py-3.5 transition-colors first:border-t-0",
        isSelected &&
          status.state !== "failed" &&
          !isUnsupported &&
          "bg-muted/20",
        isSelected &&
          status.state === "failed" &&
          "bg-destructive/5 ring-1 ring-inset ring-destructive/20",
        isSelected && isUnsupported && "bg-muted/25"
      )}
    >
      <div className="grid gap-3 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-start">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="text-[0.95rem] font-semibold leading-5 tracking-normal">
              {status.profile.runtimeLabel}
            </h4>
            {isRecommended && (
              <Badge
                variant="outline"
                className="h-5 border-transparent bg-muted px-1.5 text-xs font-medium text-muted-foreground ring-1 ring-inset ring-border/70"
              >
                推荐
              </Badge>
            )}
            {isSelected && (
              <Badge
                variant="outline"
                className="rosetta-settings-accent-badge h-5 border-transparent px-1.5 text-xs font-medium"
              >
                当前
              </Badge>
            )}
            <StateBadge state={status.state} />
          </div>

          {showSummary && (
            <div className="mt-1.5 flex min-w-0 items-start gap-2 text-[0.85rem] leading-5 text-muted-foreground">
              {summary.spinning ? (
                <LoaderCircle className="mt-0.5 size-3.5 shrink-0 animate-spin" />
              ) : (
                <span
                  className={cn("mt-1.5 size-2 shrink-0 rounded-full", summary.dot)}
                />
              )}
              <p className="min-w-0">
                <span className="font-medium text-foreground/85">
                  {summary.label}
                </span>
                {summary.sub ? (
                  <span className="text-muted-foreground"> · {summary.sub}</span>
                ) : null}
              </p>
            </div>
          )}
        </div>

        <div className="flex shrink-0 flex-wrap gap-2 xl:justify-end">
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

      <div className="mt-2 flex flex-col gap-2">
        {connectivityPanel ? <div className="mt-1">{connectivityPanel}</div> : null}

        {!isUnsupported && (
          <Collapsible open={detailsOpen} onOpenChange={onDetailsOpenChange}>
            <CollapsibleTrigger asChild>
              <button
                type="button"
                className="flex h-7 w-fit items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <ChevronDown
                  className={cn(
                    "size-3.5 transition-transform duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]",
                    detailsOpen && "rotate-180"
                  )}
                />
                详情
              </button>
            </CollapsibleTrigger>
            <CollapsibleContent className="rosetta-settings-collapsible-content">
              <div className="mt-2 flex flex-col gap-3 border-t border-border/70 pt-4">
                <ModelInfoRows status={status} />

                <Collapsible open={pathsOpen} onOpenChange={setPathsOpen}>
                  <CollapsibleTrigger asChild>
                    <button
                      type="button"
                      className="flex h-7 w-fit items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      <ChevronDown
                        className={cn(
                          "size-3.5 transition-transform duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]",
                          pathsOpen && "rotate-180"
                        )}
                      />
                      路径
                    </button>
                  </CollapsibleTrigger>
                  <CollapsibleContent className="rosetta-settings-collapsible-content">
                    <div className="pt-1">
                      <PathInfoRows status={status} />
                    </div>
                  </CollapsibleContent>
                </Collapsible>

                <Collapsible open={logsOpen} onOpenChange={onLogsOpenChange}>
                  <CollapsibleTrigger asChild>
                    <button
                      type="button"
                      className="flex h-7 w-fit items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      <ChevronDown
                        className={cn(
                          "size-3.5 transition-transform duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]",
                          logsOpen && "rotate-180"
                        )}
                      />
                      <TerminalSquare className="size-3.5" />
                      日志
                    </button>
                  </CollapsibleTrigger>
                  <CollapsibleContent className="rosetta-settings-collapsible-content">
                    <div className="pt-1">
                      <LogsSummaryBlock logs={logs} isLoading={logsLoading} />
                    </div>
                  </CollapsibleContent>
                </Collapsible>
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

function isRecommendedRuntimeProfile(
  status: ManagedRuntimeProfileStatus,
  profileStatuses: ManagedRuntimeProfileStatus[]
): boolean {
  const lightningAvailable = profileStatuses.some(
    (entry) => isLightningProfile(entry) && entry.hardware.supported
  );

  if (isLightningProfile(status)) {
    return status.hardware.supported;
  }

  if (isLlamaCppProfile(status) && lightningAvailable) {
    return false;
  }

  return status.profile.recommended;
}

function StateBadge({ state }: { state: ManagedRuntimeState }) {
  const label = stateLabel(state);
  if (!label) return null;
  return (
    <Badge
      variant="outline"
      className={cn(
        "h-5 border-transparent px-1.5 text-xs font-medium ring-1 ring-inset ring-border/70",
        state === "failed"
          ? "bg-destructive/10 text-destructive ring-destructive/20"
          : state === "unsupported"
            ? "bg-muted/60 text-muted-foreground"
            : state === "not-installed" || state === "stopped" || state === "installed"
              ? "bg-muted/70 text-muted-foreground"
              : "rosetta-settings-accent-badge"
      )}
    >
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
        dot: status.process.cpuFallback ? "bg-amber-500" : "bg-primary",
        label: status.process.cpuFallback ? "CPU 回退运行中" : "运行中",
      };
    case "starting":
      return {
        dot: "bg-primary",
        label: "启动中",
        spinning: true,
      };
    case "installed":
      return {
        dot: "bg-muted-foreground/50",
        label: "已安装",
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
        sub: summarizeRuntimeError(status.process.lastError ?? status.message),
      };
    case "unsupported":
      return {
        dot: "bg-muted-foreground/30",
        label: "不支持",
        sub: summarizeHardwareIssue(status.hardware.message),
      };
    case "not-installed":
      return {
        dot: "bg-muted-foreground/30",
        label: "未安装",
      };
    default:
      return {
        dot: "bg-muted-foreground/30",
        label: "正在读取状态",
        spinning: true,
      };
  }
}

function summarizeRuntimeError(message: string | null | undefined): string {
  if (!message) {
    return "本地模型启动失败。";
  }
  if (
    message.includes("Windows 无法连接") ||
    message.includes("loopback") ||
    message.includes("127.0.0.1")
  ) {
    return "Windows 拦住了本机连接，Rosetta 无法连接本地翻译服务。";
  }
  if (message.includes("在 45 秒内未就绪") || message.includes("timed out")) {
    return "本地翻译服务启动后没有及时响应。";
  }
  if (message.includes("Vulkan") || message.includes("vk::")) {
    return "显卡 Vulkan 初始化失败，可尝试更新显卡驱动或改用 CPU 回退。";
  }
  const firstLine =
    message
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find((line) => line && !line.startsWith("---")) ?? message;
  return firstLine.length > 120 ? `${firstLine.slice(0, 120).trimEnd()}...` : firstLine;
}

function summarizeHardwareIssue(message: string): string {
  if (message.includes("NVIDIA")) {
    return "需要 NVIDIA GPU";
  }
  if (message.includes("Vulkan") || message.includes("vk::")) {
    return "需要 Vulkan 可用显卡";
  }
  const firstLine = message
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find(Boolean);
  if (!firstLine) {
    return "当前设备不可用";
  }
  return firstLine.length > 64 ? `${firstLine.slice(0, 64).trimEnd()}...` : firstLine;
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

function isRuntimeRunningState(state: ManagedRuntimeState): boolean {
  return state === "ready" || state === "starting";
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
          下载代理
        </Label>
        <span className="text-[11px] text-muted-foreground">可选</span>
      </div>
      <Input
        id="managed-rwkv-download-proxy"
        type="text"
        placeholder="http://127.0.0.1:7897"
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
  ];

  if (status.process.baseUrl) {
    rows.push({ label: "监听", value: status.process.baseUrl });
  }

  rows.push(
    {
      label: "健康检查",
      value: status.profile.healthPath,
    },
    {
      label: "翻译接口",
      value: status.profile.batchChatPath,
    }
  );

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

function PathInfoRows({ status }: { status: ManagedRuntimeProfileStatus }) {
  const rows: Array<{ label: string; value: string }> = [
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
        <span className="truncate">{message || "正在安装本地模型"}</span>
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
