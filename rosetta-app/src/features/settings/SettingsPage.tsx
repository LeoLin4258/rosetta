import { useEffect, useState, type ChangeEvent } from "react";
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
import { Card, CardContent } from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { isManagedRuntimeReady } from "@/lib/useManagedRwkvRuntime";
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
      <section className="mx-auto mb-20 flex w-full max-w-5xl flex-col gap-8 px-6 py-6">
        <header className="flex flex-col gap-2">
          <h1 className="text-2xl font-semibold tracking-normal">设置</h1>
          <p className="max-w-3xl text-sm text-muted-foreground">
            选择翻译方式，检查本地组件，并管理外观与更新。
          </p>
        </header>

        <main className="flex w-full flex-col gap-8">
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

          <AppearanceSettingsSection
            setThemeMode={setThemeMode}
            themeMode={themeMode}
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
  const [localSettingsOpen, setLocalSettingsOpen] = useState(false);
  const [switchingTo, setSwitchingTo] =
    useState<RwkvProviderPreference | null>(null);
  const localReady = isManagedRuntimeReady(managedRuntimeStatus);
  const selectedLocal = rwkv.providerPreference === "local";
  const selectedProviderReady = selectedLocal ? localReady : remoteApiConfigured;
  const isSwitchingProvider = switchingTo != null;
  const state = managedRuntimeStatus?.state ?? null;
  const switchDisabled = isSwitchingProvider || isTranslationRunning;
  const currentEngineLabel = selectedLocal ? "本地模型" : "远程服务";
  const currentEngineTone = selectedProviderReady ? "selected" : "warning";

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

  return (
    <section className="flex flex-col gap-3" id="translation-ai">
      <Card className="">
        <CardContent className="flex flex-col gap-5 py-5">
          <SettingsRowHeader
            description={
              <>
                选择 Rosetta 翻译文档时使用的服务。当前使用：
                <SemanticBadge tone={currentEngineTone}>
                  {currentEngineLabel}
                  <span
                    className={cn(
                      "size-1.5 rounded-full",
                      selectedProviderReady ? "bg-emerald-500" : "bg-amber-500"
                    )}
                  />
                </SemanticBadge>
              </>
            }
            icon={<Globe />}
            title="翻译引擎"
          />

          <div className="grid gap-4 md:grid-cols-2">
            <BackendChoiceCard
              description={localServiceDescription(state, managedRuntimeStatus)}
              icon={<Cpu className="size-4" />}
              label="本地模型"
              onSelect={() => selectProviderPreference("local")}
              selected={selectedLocal}
              status={
                selectedLocal ? (localReady ? "active" : "blocked") : "idle"
              }
              statusLabel={
                switchingTo === "local"
                  ? "正在切换"
                  : selectedLocal
                  ? localReady
                    ? "已选择"
                    : localServiceSelectedProblemLabel(state)
                  : localServiceStatusLabel(state)
              }
              switchDisabled={switchDisabled}
            />
            <BackendChoiceCard
              description={
                remoteApiConfigured
                  ? displayRemoteApiUrl(rwkv)
                  : "填写服务地址、接口路径和口令后才能使用。"
              }
              icon={<Cloud className="size-4" />}
              label="远程服务"
              onSelect={() => selectProviderPreference("remote-api")}
              selected={!selectedLocal}
              status={
                !selectedLocal
                  ? remoteApiConfigured
                    ? "active"
                    : "blocked"
                  : "idle"
              }
              statusLabel={
                switchingTo === "remote-api"
                  ? "正在切换"
                  : !selectedLocal
                  ? remoteApiConfigured
                    ? "已选择"
                    : "缺少配置"
                  : remoteApiConfigured
                    ? remoteApiFallbackLabel(apiStatus)
                    : "未配置"
              }
              switchDisabled={switchDisabled}
            />
          </div>

          {isTranslationRunning ? (
            <p className="text-xs text-amber-700 dark:text-amber-300">
              正在翻译。停止当前任务后再切换翻译引擎。
            </p>
          ) : isSwitchingProvider ? (
            <p className="flex items-center gap-1.5 text-xs text-sky-700 dark:text-sky-300">
              <LoaderCircle className="size-3.5 animate-spin" />
              正在切换翻译引擎。
            </p>
          ) : null}

          <div className="flex flex-wrap gap-2">
            <Button
              aria-expanded={localSettingsOpen}
              onClick={() => setLocalSettingsOpen((open) => !open)}
              size="sm"
              type="button"
              variant="outline"
            >
              <Cpu data-icon="inline-start" />
              管理本地模型
              <ChevronDown
                className={cn(
                  "ml-1 size-3.5 transition-transform",
                  localSettingsOpen && "rotate-180"
                )}
              />
            </Button>
            <CollapsibleTriggerButton
              icon={<Cloud data-icon="inline-start" />}
              label="配置远程服务"
              open={externalApiOpen}
              onOpenChange={onExternalApiOpenChange}
            />
          </div>

          <Collapsible
            open={localSettingsOpen}
            onOpenChange={setLocalSettingsOpen}
          >
            <CollapsibleContent>
              <div className="border-t pt-5">
                <LocalRwkvPanel />
              </div>
            </CollapsibleContent>
          </Collapsible>

          <Collapsible
            open={externalApiOpen}
            onOpenChange={onExternalApiOpenChange}
          >
            <CollapsibleContent>
              <div className="border-t pt-5">
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
        </CardContent>
      </Card>
    </section>
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
      variant="outline"
    >
      {icon}
      {label}
      <ChevronDown
        className={cn(
          "ml-1 size-3.5 transition-transform",
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
    <section className="flex flex-col gap-5 rounded-md bg-muted/20 p-4">
      <div>
        <div className="flex items-start justify-between gap-3">
          <div>
            <h3 className="text-sm font-semibold tracking-normal">
              远程翻译服务
            </h3>
            <p className="mt-1 text-sm text-muted-foreground">
              填写兼容 OpenAI Chat Completions 的服务地址。选择远程服务后，文本会发送到该地址。
            </p>
          </div>
          <StatusBadge status={apiStatus} />
        </div>
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
            <Label>译文生成模式</Label>
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

        <div className="flex flex-col gap-3 rounded-md border bg-muted/30 p-3">
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

function BackendChoiceCard({
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
      ? "success"
      : status === "blocked"
        ? "warning"
        : "neutral";

  return (
    <button
      aria-pressed={selected}
      className={cn(
        "group flex min-h-24 w-full items-start gap-3 rounded-md border p-4 text-left transition-colors",
        "hover:border-foreground/30 hover:bg-muted/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        selected && "border-blue-500/70 bg-blue-500/5",
        status === "active" &&
          selected &&
          "border-blue-500/70 bg-blue-500/5",
        status === "blocked" &&
          selected &&
          "border-amber-500/60 bg-amber-500/10",
        switchDisabled && !selected && "cursor-not-allowed opacity-60"
      )}
      disabled={switchDisabled && !selected}
      onClick={onSelect}
      type="button"
    >
      <div
        className={cn(
          "mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md",
          status === "active" &&
            "bg-blue-500/10 text-blue-600 dark:text-blue-300",
          status === "blocked" &&
            "bg-amber-500/15 text-amber-700 dark:text-amber-300",
          status === "idle" && "bg-muted text-muted-foreground"
        )}
      >
        {icon}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-start justify-between gap-3">
          <p className="text-sm font-medium">{label}</p>
          <div className="flex shrink-0 items-center gap-2">
            <SemanticBadge tone={selected ? "selected" : badgeTone}>
              {selected ? "当前使用" : statusLabel}
            </SemanticBadge>
            {selected ? (
              <CheckCircle2 className="size-5 text-blue-600 dark:text-blue-300" />
            ) : null}
          </div>
        </div>
        <p className="mt-1 break-words text-xs leading-5 text-muted-foreground">
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
  status: ManagedRuntimeStatus | null
) {
  if (isManagedRuntimeReady(status)) {
    return status?.process.baseUrl
      ? `正在本机运行：${status.process.baseUrl}`
      : "本地模型正在运行。";
  }
  if (state === "installed" || state === "stopped") {
    return "模型已安装但未启动。";
  }
  if (state === "starting") {
    return "模型正在加载。";
  }
  if (state === "not-installed") {
    return "尚未下载本地模型。";
  }
  if (state === "failed") {
    return "本地模型启动失败。打开管理面板查看日志。";
  }
  if (state === "unsupported") {
    return "当前设备不能运行 Rosetta 管理的本地模型。";
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
    <Card id="appearance">
      <CardContent className="grid gap-5 py-5 md:grid-cols-[minmax(16rem,0.42fr)_minmax(0,1fr)] md:items-center">
        <SettingsRowHeader
          description="选择窗口主题。"
          icon={<Palette />}
          title="外观"
        />
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
      </CardContent>
    </Card>
  );
}

function DocumentHandlingSection() {
  return (
    <section className="flex flex-col gap-3" id="document-handling">
      <Card>
        <CardContent className="flex flex-col gap-5 py-5">
          <div className="grid gap-5 md:grid-cols-[minmax(16rem,0.42fr)_minmax(0,1fr)] md:items-center">
            <SettingsRowHeader
              description="安装本地 PDF 组件。"
              icon={<FileText />}
              title="PDF 组件"
            />

            <div className="min-w-0">
              <p className="text-sm font-medium">翻译 PDF 前需要先安装组件</p>
              <p className="mt-1 text-sm text-muted-foreground">
                组件只在本机运行，用于读取 PDF 版面并生成译文 PDF。
              </p>
            </div>
          </div>

          <div className="border-t pt-5">
            <Pdf2zhPanel />
          </div>
        </CardContent>
      </Card>
    </section>
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
    <section className="flex flex-col gap-3" id="about-settings">
      <Card>
        <CardContent className="grid gap-5 py-5 md:grid-cols-[minmax(16rem,0.42fr)_minmax(0,1fr)_auto] md:items-start">
          <SettingsRowHeader
            description="查看当前版本并检查更新。"
            icon={<Info />}
            title="关于"
          />

          <div className="flex min-w-0 flex-col gap-3">
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="flex items-center gap-2">
                  <Label>Rosetta {appVersion}</Label>
                  <UpdateStatusBadge status={updateStatus} />
                </div>
              </div>
            </div>

            <CurrentVersionHighlights
              note={getReleaseNote(appVersion)}
            />

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
                className={
                  updateStatus === "checking" ? "animate-spin" : undefined
                }
                data-icon="inline-start"
              />
              检查应用更新
            </Button>

            {updateStatus === "available" && availableUpdate ? (
              <Button onClick={onInstallUpdate} type="button">
                <Download data-icon="inline-start" />
                下载并安装更新
              </Button>
            ) : null}

            {updateStatus === "ready-to-restart" ? (
              <Button onClick={onRestart} type="button">
                <RefreshCw data-icon="inline-start" />
                重启完成更新
              </Button>
            ) : null}
          </div>
        </CardContent>
      </Card>
    </section>
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
      setResetError(
        error instanceof Error
          ? error.message
          : "无法清除 Rosetta 本机数据。"
      );
    } finally {
      setIsClearing(false);
    }
  }

  const deletedItems =
    resetResult?.items.filter((item) => item.deleted).map((item) => item.label) ??
    [];

  return (
    <section className="flex flex-col gap-3" id="danger-settings">
      <Card className="border-destructive/30">
        <CardContent className="grid gap-5 py-5 md:grid-cols-[minmax(16rem,0.42fr)_minmax(0,1fr)_auto] md:items-start">
          <SettingsRowHeader
            description="清除这台电脑上的 Rosetta 数据。"
            icon={<Trash2 />}
            title="危险操作"
          />

          <div className="flex min-w-0 flex-col gap-3">
            <div>
              <p className="text-sm font-medium">清除本机数据</p>
              <p className="mt-1 text-sm leading-6 text-muted-foreground">
                删除任务历史、本地模型、PDF 组件和本机设置。不会删除原始文件、手动导出的文件或 Rosetta 应用本身。
              </p>
            </div>

            {resetResult ? (
              <div className="rounded-md border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm text-emerald-800 dark:text-emerald-200">
                <p className="font-medium">
                  已清除 Rosetta 本机数据。请重启应用以恢复初始设置。
                </p>
                <p className="mt-1 text-xs text-emerald-800/80 dark:text-emerald-200/80">
                  {deletedItems.length > 0
                    ? `已删除：${deletedItems.join("、")}。`
                    : "未找到需要删除的本机数据目录。"}
                </p>
                {resetResult.runtimeStopError ? (
                  <p className="mt-1 text-xs">
                    本地模型停止时返回：{resetResult.runtimeStopError}
                  </p>
                ) : null}
              </div>
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
                  <LoaderCircle
                    className="animate-spin"
                    data-icon="inline-start"
                  />
                ) : (
                  <Trash2 data-icon="inline-start" />
                )}
                清除 Rosetta 数据
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>清除 Rosetta 本机数据？</AlertDialogTitle>
                <AlertDialogDescription className="text-left leading-6">
                  这会停止正在运行的本地模型，并删除任务历史、本地模型文件、PDF 组件和本机设置。原始文件、手动导出的文件和 Rosetta 应用不会被删除。
                </AlertDialogDescription>
              </AlertDialogHeader>
              <div className="rounded-md bg-muted/50 p-3 text-xs leading-6 text-muted-foreground">
                删除后，Rosetta 下次启动会回到初始状态；本地模型和 PDF 组件需要重新安装。
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
                    <LoaderCircle
                      className="animate-spin"
                      data-icon="inline-start"
                    />
                  ) : (
                    <Trash2 data-icon="inline-start" />
                  )}
                  清除本机数据
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </CardContent>
      </Card>
    </section>
  );
}

function SettingsRowHeader({
  description,
  icon,
  title,
}: {
  description: React.ReactNode;
  icon: React.ReactNode;
  title: string;
}) {
  return (
    <div className="flex min-w-0 gap-3">
      <SettingsIconFrame>{icon}</SettingsIconFrame>
      <div className="min-w-0">
        <h2 className="text-base font-semibold tracking-normal">{title}</h2>
        <div className="mt-1 flex flex-wrap items-center gap-1.5 text-sm text-muted-foreground">
          {description}
        </div>
      </div>
    </div>
  );
}

function SettingsIconFrame({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
      {children}
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
        "border-transparent",
        tone === "selected" &&
          "bg-blue-500/15 text-blue-700 dark:text-blue-300",
        tone === "success" &&
          "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
        tone === "warning" &&
          "bg-amber-500/15 text-amber-800 dark:text-amber-300",
        tone === "danger" &&
          "bg-destructive/15 text-destructive dark:text-red-300",
        tone === "info" && "bg-sky-500/15 text-sky-700 dark:text-sky-300",
        tone === "neutral" &&
          "bg-muted text-muted-foreground dark:bg-muted/70"
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
              "size-3.5 transition-transform",
              open && "rotate-180"
            )}
          />
          当前版本特性
        </button>
      </CollapsibleTrigger>
      <CollapsibleContent>
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
        {result.statusCode != null && <span>HTTP {result.statusCode}</span>}
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
