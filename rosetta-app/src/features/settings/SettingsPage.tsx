import { useEffect, useState, type ChangeEvent } from "react";
import { useSearchParams } from "react-router-dom";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import {
  CheckCircle2,
  ChevronDown,
  Cloud,
  Cpu,
  FileText,
  Globe,
  Info,
  Download,
  LoaderCircle,
  Palette,
  RefreshCw,
  Send,
  ShieldCheck,
  Timer,
  Trash2,
  XCircle,
} from "lucide-react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  isManagedRuntimeProfileReady,
  selectManagedRuntimeProfileStatus,
} from "@/lib/managedRuntimeSelection";
import { getReleaseNote, type ReleaseNote } from "../../data/releaseNotes";
import {
  clearRosettaLocalData,
  type LocalDataResetResult,
} from "../../lib/rosettaJobs";
import { probeRwkvTranslationApi } from "../../lib/rwkvApi";
import { cn } from "../../lib/utils";
import { useRosettaStore } from "../../store/useRosettaStore";
import { LocalRwkvPanel } from "./LocalRwkvPanel";
import { Pdf2zhPanel } from "./Pdf2zhPanel";
import type {
  AppThemeMode,
  ManagedRuntimeProfileStatus,
  ManagedRuntimeStatus,
  RwkvConnectionConfig,
  RwkvProviderPreference,
  RwkvTranslationApiProbeResult,
  TranslationMode,
} from "../../types/rosetta";
import { ScrollArea } from "@/components/ui/scroll-area";

const modeOptions: Array<{ label: string; value: TranslationMode }> = [
  { label: "极速", value: "fast" },
  { label: "平衡", value: "balanced" },
  { label: "连贯", value: "coherent" },
];

const themeOptions: Array<{ label: string; value: AppThemeMode }> = [
  { label: "浅色", value: "light" },
  { label: "深色", value: "dark" },
  { label: "跟随系统", value: "system" },
];

type AvailableAppUpdate = NonNullable<Awaited<ReturnType<typeof check>>>;

type UpdateStatus =
  | "idle"
  | "checking"
  | "latest"
  | "available"
  | "downloading"
  | "installing"
  | "ready-to-restart"
  | "failed";

const SETTINGS_SECTION_CLASS =
  "grid gap-5 border-t border-border/70 py-7 md:grid-cols-[12rem_minmax(0,1fr)] md:gap-8";
const SETTINGS_PANEL_CLASS =
  "rounded-lg border border-border/70 bg-muted/20";

export function SettingsPage() {
  const themeMode = useRosettaStore((state) => state.themeMode);
  const setThemeMode = useRosettaStore((state) => state.setThemeMode);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const managedRuntimeStatus = useRosettaStore(
    (state) => state.managedRuntime.status
  );
  const activeTranslationRun = useRosettaStore(
    (state) => state.activeTranslationRun
  );
  const clearJobHistory = useRosettaStore((state) => state.clearJobHistory);
  const updateRwkvConfig = useRosettaStore((state) => state.updateRwkvConfig);
  const setTranslationMode = useRosettaStore((state) => state.setTranslationMode);
  const [externalApiOpen, setExternalApiOpen] = useState(false);
  const [apiProbeResult, setApiProbeResult] =
    useState<RwkvTranslationApiProbeResult | null>(null);
  const [apiError, setApiError] = useState<string | null>(null);
  const [isProbingApi, setIsProbingApi] = useState(false);
  const [appVersion, setAppVersion] = useState("读取中");
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const [availableUpdate, setAvailableUpdate] =
    useState<AvailableAppUpdate | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<{
    downloaded: number;
    total: number | null;
  }>({ downloaded: 0, total: null });

  useEffect(() => {
    void getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion("未知版本"));
  }, []);

  async function probeApi() {
    setIsProbingApi(true);
    setApiError(null);
    setApiProbeResult(null);

    try {
      const probeResult = await probeRwkvTranslationApi({
        baseUrl: rwkv.baseUrl,
        endpoint: rwkv.endpoint,
        internalToken: rwkv.internalToken,
        bodyPassword: rwkv.bodyPassword,
        timeoutMs: rwkv.timeoutMs,
      });
      setApiProbeResult(probeResult);
    } catch (error) {
      setApiError(
        error instanceof Error ? error.message : "无法连接到翻译服务。"
      );
    } finally {
      setIsProbingApi(false);
    }
  }

  function updateTextField(
    field: keyof Pick<
      RwkvConnectionConfig,
      "baseUrl" | "endpoint" | "internalToken" | "bodyPassword"
    >
  ) {
    return (event: ChangeEvent<HTMLInputElement>) => {
      updateRwkvConfig({ [field]: event.currentTarget.value });
    };
  }

  function updateTimeout(event: ChangeEvent<HTMLInputElement>) {
    const timeoutMs = Number.parseInt(event.currentTarget.value, 10);

    if (Number.isFinite(timeoutMs) && timeoutMs > 0) {
      updateRwkvConfig({ timeoutMs });
    }
  }

  async function checkForUpdate() {
    setUpdateStatus("checking");
    setAvailableUpdate(null);
    setUpdateError(null);
    setDownloadProgress({ downloaded: 0, total: null });

    try {
      const update = await check();

      if (update) {
        setAvailableUpdate(update);
        setUpdateStatus("available");
      } else {
        setUpdateStatus("latest");
      }
    } catch (error) {
      setUpdateStatus("failed");
      setUpdateError(
        error instanceof Error ? error.message : "无法检查更新。请稍后重试。"
      );
    }
  }

  async function installAvailableUpdate() {
    if (!availableUpdate) {
      return;
    }

    setUpdateStatus("downloading");
    setUpdateError(null);
    setDownloadProgress({ downloaded: 0, total: null });

    try {
      let downloaded = 0;

      await availableUpdate.downloadAndInstall((event) => {
        if (event.event === "Started") {
          downloaded = 0;
          setDownloadProgress({
            downloaded,
            total: event.data.contentLength ?? null,
          });
          setUpdateStatus("downloading");
        }

        if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setDownloadProgress((current) => ({
            downloaded,
            total: current.total,
          }));
        }

        if (event.event === "Finished") {
          setUpdateStatus("installing");
        }
      });

      setUpdateStatus("ready-to-restart");
    } catch (error) {
      setUpdateStatus("failed");
      setUpdateError(
        error instanceof Error ? error.message : "无法安装更新。请稍后重新下载。"
      );
    }
  }

  async function restartApp() {
    setUpdateStatus("installing");
    setUpdateError(null);

    try {
      await relaunch();
    } catch (error) {
      setUpdateStatus("failed");
      setUpdateError(
        error instanceof Error ? error.message : "无法重启 Rosetta。请手动退出后重新打开。"
      );
    }
  }

  const missingConnectionFields = [
    !rwkv.baseUrl.trim() && "服务地址",
    !rwkv.endpoint.trim() && "接口路径",
    !rwkv.internalToken.trim() && "访问密钥",
    !rwkv.bodyPassword.trim() && "请求口令",
    rwkv.timeoutMs <= 0 && "超时时间",
  ].filter(Boolean) as string[];
  const canProbeApi = missingConnectionFields.length === 0 && !isProbingApi;
  const apiStatus = apiProbeResult?.ok
    ? "connected"
    : apiProbeResult || apiError
      ? "failed"
      : "not-tested";
  const remoteApiConfigured = missingConnectionFields.length === 0;

  return (
    <ScrollArea className="h-full w-full">
      <section className="mx-auto mb-24 flex w-full max-w-5xl flex-col gap-7 px-6 py-8">
        <header className="flex flex-col gap-2">
          <h1 className="text-2xl font-semibold tracking-normal">设置</h1>
        </header>

        <main className="flex w-full flex-col">
          <AppearanceSettingsSection
            setThemeMode={setThemeMode}
            themeMode={themeMode}
          />

          <TranslationAiSection
            apiStatus={apiStatus}
            canProbeApi={canProbeApi}
            externalApiOpen={externalApiOpen}
            isProbingApi={isProbingApi}
            isTranslationRunning={activeTranslationRun != null}
            managedRuntimeStatus={managedRuntimeStatus}
            missingConnectionFields={missingConnectionFields}
            apiError={apiError}
            apiProbeResult={apiProbeResult}
            onExternalApiOpenChange={setExternalApiOpen}
            onProbeApi={() => void probeApi()}
            remoteApiConfigured={remoteApiConfigured}
            rwkv={rwkv}
            setProviderPreference={(providerPreference) =>
              updateRwkvConfig({ providerPreference })
            }
            setTranslationMode={setTranslationMode}
            updateTextField={updateTextField}
            updateTimeout={updateTimeout}
          />

          <DocumentHandlingSection />

          <AboutSettingsSection
            appVersion={appVersion}
            availableUpdate={availableUpdate}
            downloadProgress={downloadProgress}
            onCheckForUpdate={() => void checkForUpdate()}
            onInstallUpdate={() => void installAvailableUpdate()}
            onRestart={() => void restartApp()}
            updateError={updateError}
            updateStatus={updateStatus}
          />

          <DangerSettingsSection clearJobHistory={clearJobHistory} />
        </main>
      </section>
    </ScrollArea>
  );
}

function TranslationAiSection({
  apiError,
  apiProbeResult,
  apiStatus,
  canProbeApi,
  externalApiOpen,
  isProbingApi,
  isTranslationRunning,
  managedRuntimeStatus,
  missingConnectionFields,
  onExternalApiOpenChange,
  onProbeApi,
  remoteApiConfigured,
  rwkv,
  setProviderPreference,
  setTranslationMode,
  updateTextField,
  updateTimeout,
}: {
  apiError: string | null;
  apiProbeResult: RwkvTranslationApiProbeResult | null;
  apiStatus: "connected" | "failed" | "not-tested";
  canProbeApi: boolean;
  externalApiOpen: boolean;
  isProbingApi: boolean;
  isTranslationRunning: boolean;
  managedRuntimeStatus: ManagedRuntimeStatus | null;
  missingConnectionFields: string[];
  onExternalApiOpenChange: (open: boolean) => void;
  onProbeApi: () => void;
  remoteApiConfigured: boolean;
  rwkv: RwkvConnectionConfig;
  setProviderPreference: (preference: RwkvProviderPreference) => void;
  setTranslationMode: (mode: TranslationMode) => void;
  updateTextField: (
    field: keyof Pick<
      RwkvConnectionConfig,
      "baseUrl" | "endpoint" | "internalToken" | "bodyPassword"
    >
  ) => (event: ChangeEvent<HTMLInputElement>) => void;
  updateTimeout: (event: ChangeEvent<HTMLInputElement>) => void;
}) {
  const [searchParams] = useSearchParams();
  const [localSettingsOpen, setLocalSettingsOpen] = useState(false);
  const [switchingTo, setSwitchingTo] =
    useState<RwkvProviderPreference | null>(null);
  const selectedRuntimeStatus = selectManagedRuntimeProfileStatus(
    managedRuntimeStatus,
    rwkv.managedRuntimeProfileId
  );
  const localReady = isManagedRuntimeProfileReady(selectedRuntimeStatus);
  const selectedLocal = rwkv.providerPreference === "local";
  const selectedProviderReady = selectedLocal ? localReady : remoteApiConfigured;
  const isSwitchingProvider = switchingTo != null;
  const state = selectedRuntimeStatus?.state ?? managedRuntimeStatus?.state ?? null;
  const switchDisabled = isSwitchingProvider || isTranslationRunning;
  const currentEngineLabel = selectedLocal ? "本地模型" : "远程服务";
  const currentEngineTone = selectedProviderReady ? "selected" : "warning";

  useEffect(() => {
    if (searchParams.get("panel") !== "local-runtime") {
      return;
    }
    setLocalSettingsOpen(true);
    onExternalApiOpenChange(false);
    window.setTimeout(() => {
      document.getElementById("local-rwkv")?.scrollIntoView({
        block: "start",
        behavior: "smooth",
      });
    }, 0);
  }, [onExternalApiOpenChange, searchParams]);

  useEffect(() => {
    if (!switchingTo) return undefined;

    const timer = window.setTimeout(() => {
      setProviderPreference(switchingTo);
      setSwitchingTo(null);
    }, 650);

    return () => window.clearTimeout(timer);
  }, [setProviderPreference, switchingTo]);

  function selectProviderPreference(preference: RwkvProviderPreference) {
    if (
      switchDisabled ||
      preference === rwkv.providerPreference ||
      switchingTo === preference
    ) {
      return;
    }
    setSwitchingTo(preference);
  }

  function setLocalPanelOpen(nextOpen: boolean) {
    setLocalSettingsOpen(nextOpen);
    if (nextOpen) onExternalApiOpenChange(false);
  }

  function setRemotePanelOpen(nextOpen: boolean) {
    onExternalApiOpenChange(nextOpen);
    if (nextOpen) setLocalSettingsOpen(false);
  }

  return (
    <SettingsSection
      description={
        <>
          当前使用
          <SemanticBadge tone={currentEngineTone}>{currentEngineLabel}</SemanticBadge>
        </>
      }
      icon={<Globe />}
      id="translation-ai"
      title="翻译引擎"
    >
      <div className="flex flex-col gap-4">
        <div className={SETTINGS_PANEL_CLASS}>
          <BackendChoiceRow
            description={localServiceDescription(state, selectedRuntimeStatus)}
            icon={<Cpu className="size-4" />}
            label="本地模型"
            onSelect={() => selectProviderPreference("local")}
            selected={selectedLocal}
            status={selectedLocal ? (localReady ? "active" : "blocked") : "idle"}
            statusLabel={
              switchingTo === "local"
                ? "正在切换"
                : selectedLocal
                ? localReady
                  ? "当前"
                  : localServiceSelectedProblemLabel(state)
                : localServiceStatusLabel(state)
            }
            switchDisabled={switchDisabled}
          />
          <BackendChoiceRow
            description={
              remoteApiConfigured
                ? displayRemoteApiUrl(rwkv)
                : "填写服务地址后可选择。"
            }
            icon={<Cloud className="size-4" />}
            label="远程服务"
            onSelect={() => selectProviderPreference("remote-api")}
            selected={!selectedLocal}
            status={!selectedLocal ? (remoteApiConfigured ? "active" : "blocked") : "idle"}
            statusLabel={
              switchingTo === "remote-api"
                ? "正在切换"
                : !selectedLocal
                ? remoteApiConfigured
                  ? "当前"
                  : "缺少配置"
                : remoteApiConfigured
                  ? remoteApiFallbackLabel(apiStatus)
                  : "未配置"
            }
            switchDisabled={switchDisabled}
          />
        </div>

        {isTranslationRunning ? (
          <InlineNotice tone="warning">停止当前翻译后才能切换引擎。</InlineNotice>
        ) : isSwitchingProvider ? (
          <InlineNotice icon={<LoaderCircle className="size-3.5 animate-spin" />}>
            正在切换翻译引擎
          </InlineNotice>
        ) : null}

        <div className="flex flex-wrap gap-2">
          <Button
            aria-expanded={localSettingsOpen}
            onClick={() => setLocalPanelOpen(!localSettingsOpen)}
            size="sm"
            type="button"
            variant={localSettingsOpen ? "secondary" : "outline"}
          >
            <Cpu data-icon="inline-start" />
            本地模型
              <ChevronDown
                className={cn(
                  "ml-1 size-3.5 transition-transform duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]",
                  localSettingsOpen && "rotate-180"
                )}
              />
          </Button>
          <CollapsibleTriggerButton
            icon={<Cloud data-icon="inline-start" />}
            label="远程服务"
            open={externalApiOpen}
            onOpenChange={setRemotePanelOpen}
          />
        </div>

        <Collapsible
          open={localSettingsOpen}
          onOpenChange={setLocalPanelOpen}
        >
          <CollapsibleContent className="rosetta-settings-collapsible-content">
            <div className="border-t border-border/70 pt-4">
              <LocalRwkvPanel isTranslationRunning={isTranslationRunning} />
            </div>
          </CollapsibleContent>
        </Collapsible>

        <Collapsible
          open={externalApiOpen}
          onOpenChange={setRemotePanelOpen}
        >
          <CollapsibleContent className="rosetta-settings-collapsible-content">
            <div className="border-t border-border/70 pt-4">
              <RemoteApiSettingsPanel
                apiError={apiError}
                apiProbeResult={apiProbeResult}
                apiStatus={apiStatus}
                canProbeApi={canProbeApi}
                isProbingApi={isProbingApi}
                missingConnectionFields={missingConnectionFields}
                onProbeApi={onProbeApi}
                rwkv={rwkv}
                setTranslationMode={setTranslationMode}
                updateTextField={updateTextField}
                updateTimeout={updateTimeout}
              />
            </div>
          </CollapsibleContent>
        </Collapsible>
      </div>
    </SettingsSection>
  );
}

function CollapsibleTriggerButton({
  icon,
  label,
  onOpenChange,
  open,
}: {
  icon: React.ReactNode;
  label: string;
  onOpenChange: (open: boolean) => void;
  open: boolean;
}) {
  return (
    <Button
      aria-expanded={open}
      onClick={() => onOpenChange(!open)}
      size="sm"
      type="button"
      variant={open ? "secondary" : "outline"}
    >
      {icon}
      {label}
      <ChevronDown
        className={cn(
          "ml-1 size-3.5 transition-transform duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]",
          open && "rotate-180"
        )}
      />
    </Button>
  );
}

function RemoteApiSettingsPanel({
  apiError,
  apiProbeResult,
  apiStatus,
  canProbeApi,
  isProbingApi,
  missingConnectionFields,
  onProbeApi,
  rwkv,
  setTranslationMode,
  updateTextField,
  updateTimeout,
}: {
  apiError: string | null;
  apiProbeResult: RwkvTranslationApiProbeResult | null;
  apiStatus: "connected" | "failed" | "not-tested";
  canProbeApi: boolean;
  isProbingApi: boolean;
  missingConnectionFields: string[];
  onProbeApi: () => void;
  rwkv: RwkvConnectionConfig;
  setTranslationMode: (mode: TranslationMode) => void;
  updateTextField: (
    field: keyof Pick<
      RwkvConnectionConfig,
      "baseUrl" | "endpoint" | "internalToken" | "bodyPassword"
    >
  ) => (event: ChangeEvent<HTMLInputElement>) => void;
  updateTimeout: (event: ChangeEvent<HTMLInputElement>) => void;
}) {
  return (
    <section className="flex flex-col gap-5">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold tracking-normal">远程服务</h3>
          <p className="mt-1 text-sm text-muted-foreground">
            选择远程服务时，待翻译文本会发送到这里。
          </p>
        </div>
        <StatusBadge status={apiStatus} />
      </div>

      <div className="flex flex-col gap-5">
        <div className="grid gap-4 md:grid-cols-2">
          <SettingField htmlFor="rwkv-base-url" label="服务地址">
            <Input
              id="rwkv-base-url"
              onChange={updateTextField("baseUrl")}
              placeholder="https://..."
              value={rwkv.baseUrl}
            />
          </SettingField>

          <SettingField htmlFor="rwkv-endpoint" label="接口路径">
            <Input
              id="rwkv-endpoint"
              onChange={updateTextField("endpoint")}
              placeholder="/v1/batch/completions"
              value={rwkv.endpoint}
            />
          </SettingField>

          <SettingField htmlFor="rwkv-internal-token" label="访问密钥">
            <Input
              autoComplete="off"
              id="rwkv-internal-token"
              onChange={updateTextField("internalToken")}
              type="password"
              value={rwkv.internalToken}
            />
          </SettingField>

          <SettingField htmlFor="rwkv-body-password" label="请求口令">
            <Input
              autoComplete="off"
              id="rwkv-body-password"
              onChange={updateTextField("bodyPassword")}
              type="password"
              value={rwkv.bodyPassword}
            />
          </SettingField>
        </div>

        <Separator />

        <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_12rem]">
          <SettingField
            description="长文档建议保留较长等待时间"
            htmlFor="rwkv-timeout"
            label="超时时间（毫秒）"
          >
            <Input
              id="rwkv-timeout"
              min={1}
              onChange={updateTimeout}
              type="number"
              value={rwkv.timeoutMs}
            />
          </SettingField>

          <div className="flex flex-col gap-2">
            <Label>生成模式</Label>
            <ToggleGroup
              className="grid grid-cols-3"
              onValueChange={(value) => {
                if (value) setTranslationMode(value as TranslationMode);
              }}
              type="single"
              value={rwkv.mode}
              variant="outline"
            >
              {modeOptions.map((option) => (
                <ToggleGroupItem key={option.value} value={option.value}>
                  {option.label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
        </div>

        <div className="flex flex-col gap-3 rounded-lg border border-border/70 bg-muted/20 p-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2 text-sm">
              <ShieldCheck className="text-muted-foreground" />
              <span className="font-medium">连接测试</span>
            </div>
            <Button
              disabled={!canProbeApi}
              onClick={onProbeApi}
              type="button"
              variant={apiStatus === "connected" ? "outline" : "default"}
            >
              <Send data-icon="inline-start" />
              {isProbingApi ? "正在测试" : canProbeApi ? "测试连接" : "填写后测试"}
            </Button>
          </div>

          {missingConnectionFields.length > 0 && (
            <p className="text-xs text-muted-foreground">
              请先填写：{missingConnectionFields.join("、")}。
            </p>
          )}
          {apiError && <p className="text-sm text-destructive">{apiError}</p>}
          {apiProbeResult && <ApiProbeResult result={apiProbeResult} />}
        </div>
      </div>
    </section>
  );
}

function BackendChoiceRow({
  description,
  icon,
  label,
  meta,
  onSelect,
  selected,
  status,
  statusLabel,
  switchDisabled,
}: {
  description: string;
  icon: React.ReactNode;
  label: string;
  meta?: React.ReactNode;
  onSelect: () => void;
  selected: boolean;
  status: "active" | "idle" | "blocked";
  statusLabel: string;
  switchDisabled: boolean;
}) {
  const badgeTone =
    status === "active"
      ? "selected"
      : status === "blocked"
        ? "warning"
        : "neutral";

  return (
    <button
      aria-pressed={selected}
      className={cn(
        "group flex w-full items-start gap-3 border-t border-border/70 p-3 text-left transition-colors first:border-t-0",
        "hover:bg-muted/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        selected && "bg-muted/35",
        status === "blocked" && selected && "bg-amber-500/8",
        switchDisabled && !selected && "cursor-not-allowed opacity-60"
      )}
      disabled={switchDisabled && !selected}
      onClick={onSelect}
      type="button"
    >
      <div
        className={cn(
          "mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md bg-background text-muted-foreground ring-1 ring-border/80",
          selected && "text-foreground",
          status === "blocked" && "text-amber-700 dark:text-amber-300"
        )}
      >
        {icon}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-3">
          <p className="text-sm font-medium">{label}</p>
          <div className="flex shrink-0 items-center gap-2">
            <SemanticBadge tone={selected ? "selected" : badgeTone}>
              {statusLabel}
            </SemanticBadge>
            {selected ? (
              <CheckCircle2 className="size-4 text-foreground/70" />
            ) : null}
          </div>
        </div>
        <p className="mt-1.5 break-words text-xs leading-5 text-muted-foreground">
          {description}
        </p>
        {meta ? <p className="mt-2 text-xs text-muted-foreground">{meta}</p> : null}
      </div>
    </button>
  );
}

function localServiceStatusLabel(state: ManagedRuntimeStatus["state"] | null) {
  if (state === "starting") return "启动中";
  if (state === "installed" || state === "stopped") return "已停止";
  if (state === "not-installed") return "未下载";
  if (state === "failed") return "启动失败";
  if (state === "unsupported") return "不支持";
  return "检测中";
}

function localServiceSelectedProblemLabel(
  state: ManagedRuntimeStatus["state"] | null
) {
  if (state === "starting") return "正在启动";
  if (state === "installed" || state === "stopped") return "需启动";
  if (state === "not-installed") return "需下载";
  if (state === "failed") return "启动失败";
  if (state === "unsupported") return "不支持";
  return "检测中";
}

function localServiceDescription(
  state: ManagedRuntimeStatus["state"] | null,
  status: ManagedRuntimeProfileStatus | null
) {
  if (isManagedRuntimeProfileReady(status)) {
    return "已在本机运行。";
  }
  if (state === "installed" || state === "stopped") {
    return "已安装，启动后可用。";
  }
  if (state === "starting") {
    return "模型正在加载。";
  }
  if (state === "not-installed") {
    return "尚未下载本地模型。";
  }
  if (state === "failed") {
    return "启动失败，可在下方修复。";
  }
  if (state === "unsupported") {
    return "当前设备不支持。";
  }
  return "正在读取本地模型状态。";
}

function remoteApiFallbackLabel(status: "connected" | "failed" | "not-tested") {
  if (status === "connected") return "测试通过";
  if (status === "failed") return "测试失败";
  return "未测试";
}

function displayRemoteApiUrl(rwkv: RwkvConnectionConfig) {
  const baseUrl = rwkv.baseUrl.trim();
  const endpoint = rwkv.endpoint.trim();
  if (!baseUrl || !endpoint) {
    return "远程服务尚未填写完整。";
  }
  return `${baseUrl.replace(/\/+$/, "")}/${endpoint.replace(/^\/+/, "")}`;
}

function AppearanceSettingsSection({
  setThemeMode,
  themeMode,
}: {
  setThemeMode: (mode: AppThemeMode) => void;
  themeMode: AppThemeMode;
}) {
  return (
    <SettingsSection
      description="窗口主题"
      icon={<Palette />}
      id="appearance"
      title="外观"
    >
      <div className="grid gap-3 md:grid-cols-[8rem_minmax(18rem,1fr)] md:items-center">
        <Label>主题</Label>
        <ToggleGroup
          className="grid grid-cols-3"
          onValueChange={(value) => {
            if (value) {
              setThemeMode(value as AppThemeMode);
            }
          }}
          type="single"
          value={themeMode}
          variant="outline"
        >
          {themeOptions.map((option) => (
            <ToggleGroupItem key={option.value} value={option.value}>
              {option.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>
    </SettingsSection>
  );
}

function DocumentHandlingSection() {
  return (
    <SettingsSection
      description="PDF 翻译所需"
      icon={<FileText />}
      id="document-handling"
      title="PDF 组件"
    >
      <Pdf2zhPanel />
    </SettingsSection>
  );
}

function AboutSettingsSection({
  appVersion,
  availableUpdate,
  downloadProgress,
  onCheckForUpdate,
  onInstallUpdate,
  onRestart,
  updateError,
  updateStatus,
}: {
  appVersion: string;
  availableUpdate: AvailableAppUpdate | null;
  downloadProgress: { downloaded: number; total: number | null };
  onCheckForUpdate: () => void;
  onInstallUpdate: () => void;
  onRestart: () => void;
  updateError: string | null;
  updateStatus: UpdateStatus;
}) {
  return (
    <SettingsSection
      description="版本与更新"
      icon={<Info />}
      id="about-settings"
      title="关于"
    >
      <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-start">
        <div className="flex min-w-0 flex-col gap-3">
          <div className="flex items-center gap-2">
            <Label>Rosetta {appVersion}</Label>
            <UpdateStatusBadge status={updateStatus} />
          </div>

          <CurrentVersionHighlights note={getReleaseNote(appVersion)} />

          <UpdateStatusMessage
            error={updateError}
            progress={downloadProgress}
            status={updateStatus}
            update={availableUpdate}
          />
        </div>

        <div className="flex flex-wrap gap-2 md:justify-end">
          <Button
            disabled={
              updateStatus === "checking" ||
              updateStatus === "downloading" ||
              updateStatus === "installing"
            }
            onClick={onCheckForUpdate}
            type="button"
            variant="outline"
          >
            <RefreshCw
              className={updateStatus === "checking" ? "animate-spin" : undefined}
              data-icon="inline-start"
            />
            检查更新
          </Button>

          {updateStatus === "available" && availableUpdate ? (
            <Button onClick={onInstallUpdate} type="button">
              <Download data-icon="inline-start" />
              安装更新
            </Button>
          ) : null}

          {updateStatus === "ready-to-restart" ? (
            <Button onClick={onRestart} type="button">
              <RefreshCw data-icon="inline-start" />
              重启应用
            </Button>
          ) : null}
        </div>
      </div>
    </SettingsSection>
  );
}

function DangerSettingsSection({
  clearJobHistory,
}: {
  clearJobHistory: () => void;
}) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [isClearing, setIsClearing] = useState(false);
  const [resetError, setResetError] = useState<string | null>(null);
  const [resetResult, setResetResult] = useState<LocalDataResetResult | null>(
    null
  );

  async function clearLocalData() {
    setIsClearing(true);
    setResetError(null);
    setResetResult(null);

    try {
      const result = await clearRosettaLocalData();
      clearJobHistory();
      useRosettaStore.persist.clearStorage();
      window.localStorage.removeItem("rosetta-app-settings");
      setResetResult(result);
      setDialogOpen(false);
    } catch (error) {
      setResetError(errorMessage(error, "无法清除 Rosetta 本机数据。"));
    } finally {
      setIsClearing(false);
    }
  }

  const deletedItems =
    resetResult?.items.filter((item) => item.deleted).map((item) => item.label) ??
    [];

  return (
    <SettingsSection
      description="本机数据"
      icon={<Trash2 />}
      id="danger-settings"
      tone="danger"
      title="危险操作"
    >
      <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-start">
        <div className="flex min-w-0 flex-col gap-3">
          <div>
            <p className="text-sm font-medium">清除本机数据</p>
            <p className="mt-1 text-sm leading-6 text-muted-foreground">
              删除任务历史、本地模型、PDF 组件和本机设置。不会删除原始文件和已导出的文件。
            </p>
          </div>

          {resetResult ? (
            <InlineNotice tone="success">
              {deletedItems.length > 0
                ? `已清除：${deletedItems.join("、")}。重启后生效。`
                : "未找到需要清除的数据。"}
            </InlineNotice>
          ) : null}

          {resetResult?.runtimeStopError ? (
            <p className="text-xs text-muted-foreground">
              本地模型停止时返回：{resetResult.runtimeStopError}
            </p>
          ) : null}

          {resetError ? (
            <p className="text-sm text-destructive">{resetError}</p>
          ) : null}
        </div>

        <AlertDialog open={dialogOpen} onOpenChange={setDialogOpen}>
          <AlertDialogTrigger asChild>
            <Button
              className="justify-self-start md:justify-self-end"
              disabled={isClearing}
              type="button"
              variant="destructive"
            >
              {isClearing ? (
                <LoaderCircle className="animate-spin" data-icon="inline-start" />
              ) : (
                <Trash2 data-icon="inline-start" />
              )}
              清除数据
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>清除本机数据？</AlertDialogTitle>
              <AlertDialogDescription className="text-left leading-6">
                将删除任务历史、本地模型、PDF 组件和本机设置。原始文件和已导出的文件会保留。
              </AlertDialogDescription>
            </AlertDialogHeader>
            <div className="rounded-lg bg-muted/50 p-3 text-xs leading-6 text-muted-foreground">
              下次启动 Rosetta 时，需要重新安装本地模型和 PDF 组件。
            </div>
            <AlertDialogFooter>
              <AlertDialogCancel disabled={isClearing}>
                取消
              </AlertDialogCancel>
              <AlertDialogAction
                disabled={isClearing}
                onClick={(event) => {
                  event.preventDefault();
                  void clearLocalData();
                }}
                variant="destructive"
              >
                {isClearing ? (
                  <LoaderCircle className="animate-spin" data-icon="inline-start" />
                ) : (
                  <Trash2 data-icon="inline-start" />
                )}
                清除本机数据
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>
    </SettingsSection>
  );
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return fallback;
}

function SettingsSection({
  children,
  description,
  icon,
  id,
  title,
  tone = "default",
}: {
  children: React.ReactNode;
  description?: React.ReactNode;
  icon: React.ReactNode;
  id: string;
  title: string;
  tone?: "default" | "danger";
}) {
  return (
    <section
      className={cn(
        SETTINGS_SECTION_CLASS,
        tone === "danger" && "border-destructive/25"
      )}
      id={id}
    >
      <div className="flex min-w-0 gap-3 md:sticky md:top-4 md:self-start">
        <SettingsIconFrame tone={tone}>{icon}</SettingsIconFrame>
        <div className="min-w-0 space-y-1">
          <h2 className="text-sm font-semibold tracking-normal">{title}</h2>
          {description ? (
            <div className="flex flex-wrap items-center gap-1.5 text-xs leading-5 text-muted-foreground">
              {description}
            </div>
          ) : null}
        </div>
      </div>
      <div className="min-w-0">{children}</div>
    </section>
  );
}

function SettingsIconFrame({
  children,
  tone = "default",
}: {
  children: React.ReactNode;
  tone?: "default" | "danger";
}) {
  return (
    <div
      className={cn(
        "flex size-8 shrink-0 items-center justify-center rounded-lg border border-border/70 bg-muted/35 text-muted-foreground",
        tone === "danger" &&
          "border-destructive/25 bg-destructive/8 text-destructive"
      )}
    >
      {children}
    </div>
  );
}

function InlineNotice({
  children,
  icon,
  tone = "neutral",
}: {
  children: React.ReactNode;
  icon?: React.ReactNode;
  tone?: "neutral" | "success" | "warning" | "danger";
}) {
  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-md border border-border/70 bg-muted/25 px-3 py-2 text-sm text-muted-foreground",
        tone === "success" && "border-primary/15 bg-primary/5 text-foreground",
        tone === "warning" &&
          "border-amber-500/25 bg-amber-500/8 text-amber-800 dark:text-amber-300",
        tone === "danger" &&
          "border-destructive/30 bg-destructive/8 text-destructive"
      )}
    >
      {icon ? <span className="mt-0.5 shrink-0">{icon}</span> : null}
      <span className="min-w-0">{children}</span>
    </div>
  );
}

function SettingField({
  children,
  description,
  htmlFor,
  label,
}: {
  children: React.ReactNode;
  description?: string;
  htmlFor: string;
  label: string;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between gap-3">
        <Label htmlFor={htmlFor}>{label}</Label>
        {description ? (
          <span className="truncate text-xs text-muted-foreground">
            {description}
          </span>
        ) : null}
      </div>
      {children}
    </div>
  );
}

function SemanticBadge({
  children,
  tone,
}: {
  children: React.ReactNode;
  tone: "selected" | "success" | "warning" | "danger" | "info" | "neutral";
}) {
  return (
    <Badge
      variant="outline"
      className={cn(
        "h-5 gap-1.5 border-transparent px-1.5 text-[11px] font-normal ring-1 ring-inset ring-border/70",
        tone === "selected" &&
          "bg-foreground/8 text-foreground dark:bg-white/10 dark:text-white",
        tone === "success" &&
          "bg-foreground/8 text-foreground dark:bg-white/10 dark:text-white",
        tone === "warning" &&
          "border-amber-500/20 bg-amber-500/10 text-amber-800 ring-amber-500/20 dark:text-amber-300",
        tone === "danger" &&
          "border-destructive/20 bg-destructive/10 text-destructive ring-destructive/20",
        tone === "info" && "bg-muted/70 text-foreground",
        tone === "neutral" &&
          "bg-muted/70 text-muted-foreground"
      )}
    >
      {children}
    </Badge>
  );
}

function StatusBadge({
  status,
}: {
  status: "connected" | "failed" | "not-tested";
}) {
  if (status === "connected") {
    return (
      <SemanticBadge tone="success">
        <CheckCircle2 data-icon="inline-start" />
        测试通过
      </SemanticBadge>
    );
  }
  if (status === "failed") {
    return (
      <SemanticBadge tone="danger">
        <XCircle data-icon="inline-start" />
        测试失败
      </SemanticBadge>
    );
  }
  return <SemanticBadge tone="neutral">尚未测试</SemanticBadge>;
}

function UpdateStatusBadge({ status }: { status: UpdateStatus }) {
  if (status === "latest") {
    return (
      <SemanticBadge tone="success">
        <CheckCircle2 data-icon="inline-start" />
        已是最新
      </SemanticBadge>
    );
  }

  if (status === "available") {
    return <SemanticBadge tone="info">发现新版本</SemanticBadge>;
  }

  if (
    status === "checking" ||
    status === "downloading" ||
    status === "installing"
  ) {
    return (
      <SemanticBadge tone="warning">
        <LoaderCircle className="animate-spin" data-icon="inline-start" />
        正在处理
      </SemanticBadge>
    );
  }

  if (status === "ready-to-restart") {
    return (
      <SemanticBadge tone="success">
        <CheckCircle2 data-icon="inline-start" />
        需要重启
      </SemanticBadge>
    );
  }

  if (status === "failed") {
    return (
      <SemanticBadge tone="danger">
        <XCircle data-icon="inline-start" />
        更新失败
      </SemanticBadge>
    );
  }

  return <SemanticBadge tone="neutral">未检查</SemanticBadge>;
}

/**
 * Always-on display of the currently-installed version's release highlights.
 * Sits between the version badge line and the dynamic UpdateStatusMessage,
 * so the user can see "what I'm running" even when no update is available
 * (and offline). Returns a minimal placeholder when we don't have a note
 * for the current version (typically a dev build before the version was
 * added to `RELEASE_NOTES`).
 */
function CurrentVersionHighlights({ note }: { note: ReleaseNote | null }) {
  const [open, setOpen] = useState(false);

  if (!note || note.highlights.length === 0) {
    return null;
  }

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className="flex h-8 w-fit items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ChevronDown
            className={cn(
              "size-3.5 transition-transform duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]",
              open && "rotate-180"
            )}
          />
          当前版本特性
        </button>
      </CollapsibleTrigger>
      <CollapsibleContent className="rosetta-settings-collapsible-content">
        <ul className="ml-4 mt-2 list-disc space-y-1 text-sm leading-6 text-muted-foreground marker:text-muted-foreground/50">
          {note.highlights.map((line, index) => (
            <li key={index}>{line}</li>
          ))}
        </ul>
      </CollapsibleContent>
    </Collapsible>
  );
}

function UpdateStatusMessage({
  error,
  progress,
  status,
  update,
}: {
  error: string | null;
  progress: { downloaded: number; total: number | null };
  status: UpdateStatus;
  update: AvailableAppUpdate | null;
}) {
  if (status === "failed") {
    return (
      <p className="text-sm text-destructive">
        {error ?? "无法完成更新。请稍后再次检查。"}
      </p>
    );
  }

  if (status === "latest") {
    return (
      <p className="text-sm text-muted-foreground">
        {/* 当前已经是最新版本。 */}
      </p>
    );
  }

  if (status === "available" && update) {
    return (
      <div className="flex flex-col gap-2 rounded-md border border-primary/30 bg-primary/5 p-3">
        <div className="flex flex-wrap items-center gap-2 text-xs font-medium text-muted-foreground">
        <span>新版本包含</span>
          <span className="rounded-sm bg-primary/10 px-1.5 py-0.5 text-primary">
            {update.version}
          </span>
          {update.date ? (
            <span className="text-muted-foreground/70">{update.date}</span>
          ) : null}
        </div>
        {update.body ? (
          // `update.body` 来自 Tauri updater 后端，通常是 Supabase function
          // 拼接的 release notes（plain text 或 markdown）。这里按 whitespace
          // 保留呈现；如果将来你想渲染 markdown，可以换成 react-markdown，
          // 但 release notes 这种短文本 plain text 已经够了。
          <p className="whitespace-pre-wrap text-sm leading-6 text-foreground/90">
            {update.body}
          </p>
        ) : (
          <p className="text-sm text-muted-foreground">
            这个版本没有更新说明。
          </p>
        )}
      </div>
    );
  }

  if (status === "downloading") {
    return (
      <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
        <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
        正在下载更新
        {progress.total
          ? `：${formatBytes(progress.downloaded)} / ${formatBytes(
            progress.total
          )}`
          : progress.downloaded > 0
            ? `：${formatBytes(progress.downloaded)}`
            : ""}
      </p>
    );
  }

  if (status === "installing") {
    return (
      <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
        <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
        正在安装更新。请保持 Rosetta 打开。
      </p>
    );
  }

  if (status === "ready-to-restart") {
    return (
      <p className="text-sm text-muted-foreground">
        更新已安装。重启 Rosetta 后会进入新版本。
      </p>
    );
  }

  if (status === "checking") {
    return (
      <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
        <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
        正在检查更新…
      </p>
    );
  }

  return (
    <p className="text-sm text-muted-foreground">
      点击“检查应用更新”查看是否有新版本。
    </p>
  );
}

function ApiProbeResult({
  result,
}: {
  result: RwkvTranslationApiProbeResult;
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-3 rounded-md border bg-background p-3",
        !result.ok && "border-destructive/40"
      )}
    >
      <div className="flex flex-wrap items-center gap-2 text-sm">
        {result.ok ? (
          <CheckCircle2 className="text-primary" />
        ) : (
          <XCircle className="text-destructive" />
        )}
        <span className="font-medium">
          {result.ok ? "远程服务可用" : "远程服务不可用"}
        </span>
        <span className="text-muted-foreground">{result.message}</span>
      </div>

      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <span className="inline-flex items-center gap-1">
          <Timer />
          {result.latencyMs}ms
        </span>
        {result.statusCode != null && <span>状态码 {result.statusCode}</span>}
      </div>

      {result.translations.length > 0 ? (
        <div className="grid gap-2">
          {result.translations.map((translation, index) => (
            <div className="rounded-md bg-muted/40 p-2 text-sm" key={index}>
              <p className="text-xs text-muted-foreground">
                测试译文 {index + 1}
              </p>
              <p className="mt-1 leading-6">{translation}</p>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function formatBytes(value: number) {
  if (value < 1024) {
    return `${value} B`;
  }

  const units = ["KB", "MB", "GB"];
  let size = value / 1024;
  let unitIndex = 0;

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }

  return `${size.toFixed(size >= 10 ? 0 : 1)} ${units[unitIndex]}`;
}
