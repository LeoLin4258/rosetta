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
  Monitor,
  Moon,
  Palette,
  RefreshCw,
  Send,
  ShieldCheck,
  Sun,
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
import { PdfMarkdownPanel } from "./PdfMarkdownPanel";
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

const themeOptions: Array<{
  icon: typeof Palette;
  label: string;
  value: AppThemeMode;
}> = [
  { icon: Sun, label: "浅色", value: "light" },
  { icon: Moon, label: "深色", value: "dark" },
  { icon: Monitor, label: "跟随系统", value: "system" },
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

type SettingsSectionId =
  | "translation-ai"
  | "document-handling"
  | "appearance"
  | "about-settings"
  | "danger-settings";

type SettingsNavItem = {
  id: SettingsSectionId;
  title: string;
  icon: typeof Palette;
  tone?: "default" | "danger";
};

const settingsNavItems: SettingsNavItem[] = [
  {
    id: "translation-ai",
    title: "翻译引擎",
    icon: Globe,
  },
  {
    id: "document-handling",
    title: "PDF 处理",
    icon: FileText,
  },
  {
    id: "appearance",
    title: "外观",
    icon: Palette,
  },
  {
    id: "about-settings",
    title: "关于",
    icon: Info,
  },
  {
    id: "danger-settings",
    title: "危险操作",
    icon: Trash2,
    tone: "danger",
  },
];

const SETTINGS_PANEL_CLASS =
  "overflow-hidden rounded-[10px] border border-border/70 bg-card";
const SETTINGS_TOGGLE_ITEM_CLASS = "rosetta-settings-toggle-item";

export function SettingsPage() {
  const [searchParams] = useSearchParams();
  const themeMode = useRosettaStore((state) => state.themeMode);
  const setThemeMode = useRosettaStore((state) => state.setThemeMode);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const downloadProxyUrl = useRosettaStore((state) => state.downloadProxy.url);
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
  const [activeSection, setActiveSection] =
    useState<SettingsSectionId>("translation-ai");

  useEffect(() => {
    void getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion("未知版本"));
  }, []);

  useEffect(() => {
    if (searchParams.get("panel") === "local-runtime") {
      setActiveSection("translation-ai");
    } else if (searchParams.get("panel") === "pdf-processing") {
      setActiveSection("document-handling");
    }
  }, [searchParams]);

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
      const update = await check({
        proxy: downloadProxyUrl.trim() || undefined,
      });

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
      <section className="rosetta-settings-page mx-auto flex w-full max-w-6xl flex-col gap-6 px-6 py-8 pb-20">
        <header className="border-b border-border/70 pb-5">
          <h1 className="text-[1.75rem] font-semibold leading-tight tracking-normal">
            设置
          </h1>
        </header>

        <div className="grid gap-6 lg:grid-cols-[13rem_minmax(0,1fr)] lg:items-start">
          <SettingsNavigation
            activeSection={activeSection}
            onSelect={setActiveSection}
          />

          <main className="min-w-0 overflow-hidden">
            <SettingsSectionTransition activeSection={activeSection}>
              {(displayedSection) => (
                <>
                  {displayedSection === "translation-ai" ? (
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
                  ) : null}

                  {displayedSection === "document-handling" ? (
                    <DocumentHandlingSection />
                  ) : null}

                  {displayedSection === "appearance" ? (
                    <AppearanceSettingsSection
                      setThemeMode={setThemeMode}
                      themeMode={themeMode}
                    />
                  ) : null}

                  {displayedSection === "about-settings" ? (
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
                  ) : null}

                  {displayedSection === "danger-settings" ? (
                    <DangerSettingsSection clearJobHistory={clearJobHistory} />
                  ) : null}
                </>
              )}
            </SettingsSectionTransition>
          </main>
        </div>
      </section>
    </ScrollArea>
  );
}

function SettingsNavigation({
  activeSection,
  onSelect,
}: {
  activeSection: SettingsSectionId;
  onSelect: (section: SettingsSectionId) => void;
}) {
  return (
    <nav
      aria-label="设置分区"
      className="rounded-[10px] border border-border/70 bg-muted/15 p-1 lg:sticky lg:top-6"
    >
      <div className="grid grid-cols-2 gap-1 md:grid-cols-5 lg:flex lg:flex-col">
        {settingsNavItems.map((item) => {
          const Icon = item.icon;
          const active = item.id === activeSection;
          return (
            <button
              key={item.id}
              aria-current={active ? "page" : undefined}
              className={cn(
                "flex h-10 w-full items-center gap-2.5 rounded-[8px] border border-transparent px-2.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                active
                  ? "border-border/80 bg-muted/65 text-foreground"
                  : "text-foreground/85 hover:bg-muted/60 hover:text-foreground",
                item.tone === "danger" &&
                  active &&
                  "border-destructive/20 bg-destructive/8 text-destructive"
              )}
              onClick={() => onSelect(item.id)}
              type="button"
            >
              <Icon
                className={cn(
                  "size-4 shrink-0 text-muted-foreground",
                  active && item.tone !== "danger" && "text-foreground/70",
                  active && item.tone === "danger" && "text-destructive"
                )}
              />
              <span className="min-w-0">
                <span className="block truncate text-[0.9rem] font-semibold leading-5">
                  {item.title}
                </span>
              </span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}

function SettingsSectionTransition({
  activeSection,
  children,
}: {
  activeSection: SettingsSectionId;
  children: (section: SettingsSectionId) => React.ReactNode;
}) {
  const [displayedSection, setDisplayedSection] =
    useState<SettingsSectionId>(activeSection);
  const [phase, setPhase] = useState<"idle" | "leaving" | "entering">(
    "idle",
  );

  useEffect(() => {
    if (activeSection === displayedSection) return;

    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setDisplayedSection(activeSection);
      setPhase("idle");
      return;
    }

    setPhase("leaving");
    const timeout = window.setTimeout(() => {
      setDisplayedSection(activeSection);
      setPhase("entering");
    }, 110);

    return () => window.clearTimeout(timeout);
  }, [activeSection, displayedSection]);

  useEffect(() => {
    if (phase !== "entering") return;
    const timeout = window.setTimeout(() => setPhase("idle"), 170);
    return () => window.clearTimeout(timeout);
  }, [phase]);

  return (
    <div
      className="rosetta-settings-section-transition min-w-0"
      data-phase={phase}
    >
      {children(displayedSection)}
    </div>
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
  const isSwitchingProvider = switchingTo != null;
  const state = selectedRuntimeStatus?.state ?? managedRuntimeStatus?.state ?? null;
  const switchDisabled = isSwitchingProvider || isTranslationRunning;

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
      icon={<Globe />}
      id="translation-ai"
      title="翻译引擎"
    >
      <div className="flex flex-col gap-4">
        <div className={SETTINGS_PANEL_CLASS}>
          <BackendChoiceRow
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
            aria-label="配置本地模型"
            className={cn(localSettingsOpen && "rosetta-settings-accent-control")}
            onClick={() => setLocalPanelOpen(!localSettingsOpen)}
            size="sm"
            type="button"
            variant="outline"
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
            ariaLabel="配置远程服务"
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
  ariaLabel,
  icon,
  label,
  onOpenChange,
  open,
}: {
  ariaLabel?: string;
  icon: React.ReactNode;
  label: string;
  onOpenChange: (open: boolean) => void;
  open: boolean;
}) {
  return (
    <Button
      aria-expanded={open}
      aria-label={ariaLabel}
      className={cn(open && "rosetta-settings-accent-control")}
      onClick={() => onOpenChange(!open)}
      size="sm"
      type="button"
      variant="outline"
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
          <h3 className="text-[0.95rem] font-semibold leading-5 tracking-normal">
            远程服务
          </h3>
          <p className="mt-1 text-sm leading-6 text-muted-foreground">
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
                <ToggleGroupItem
                  key={option.value}
                  className={SETTINGS_TOGGLE_ITEM_CLASS}
                  value={option.value}
                >
                  {option.label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
        </div>

        <div className="flex flex-col gap-3 rounded-lg border border-border/70 bg-muted/20 p-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2 text-sm">
              <ShieldCheck
                className={cn(
                  apiStatus === "connected"
                    ? "text-emerald-600 dark:text-emerald-400"
                    : "text-muted-foreground"
                )}
              />
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
  icon,
  label,
  meta,
  onSelect,
  selected,
  status,
  statusLabel,
  switchDisabled,
}: {
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
        selected && status === "active" && "bg-muted/25",
        selected && status === "blocked" && "bg-amber-500/8 ring-1 ring-inset ring-amber-500/20",
        switchDisabled && !selected && "cursor-not-allowed opacity-60"
      )}
      disabled={switchDisabled && !selected}
      onClick={onSelect}
      type="button"
    >
      <div
        className={cn(
          "mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md bg-background text-muted-foreground ring-1 ring-border/80",
          selected &&
            status === "active" &&
            "bg-muted/45 text-foreground/80 ring-border",
          status === "blocked" && "text-amber-700 dark:text-amber-300"
        )}
      >
        {icon}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-3">
          <p className="text-[0.95rem] font-semibold leading-5">{label}</p>
          <div className="flex shrink-0 items-center gap-2">
            <SemanticBadge tone={badgeTone}>
              {statusLabel}
            </SemanticBadge>
            {selected ? (
              <CheckCircle2
                className={cn(
                  "size-4",
                  status === "active"
                    ? "text-emerald-600 dark:text-emerald-400"
                    : "text-amber-700 dark:text-amber-300"
                )}
              />
            ) : null}
          </div>
        </div>
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

function remoteApiFallbackLabel(status: "connected" | "failed" | "not-tested") {
  if (status === "connected") return "测试通过";
  if (status === "failed") return "测试失败";
  return "未测试";
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
      icon={<Palette />}
      id="appearance"
      title="外观"
    >
      <div className="flex max-w-xl flex-col gap-3">
        <Label>主题</Label>
        <div
          aria-label="主题"
          className="grid grid-cols-3 gap-2"
          role="radiogroup"
        >
          {themeOptions.map((option) => {
            const Icon = option.icon;
            const selected = themeMode === option.value;

            return (
              <button
                key={option.value}
                aria-checked={selected}
                className={cn(
                  "flex h-20 min-w-0 flex-col items-center justify-center gap-2 rounded-lg border border-border/70 bg-background text-sm text-muted-foreground outline-none transition-colors hover:border-border hover:bg-muted/40 hover:text-foreground focus-visible:ring-2 focus-visible:ring-blue-500/50",
                  selected &&
                    "border-blue-500/45 bg-blue-500/[0.07] text-foreground ring-1 ring-inset ring-blue-500/20 dark:border-blue-400/45 dark:bg-blue-400/[0.08]",
                )}
                onClick={() => setThemeMode(option.value)}
                role="radio"
                type="button"
              >
                <Icon
                  className={cn(
                    "size-5",
                    selected && "text-blue-600 dark:text-blue-400",
                  )}
                />
                <span className="font-medium">{option.label}</span>
              </button>
            );
          })}
        </div>
      </div>
    </SettingsSection>
  );
}

function DocumentHandlingSection() {
  return (
    <SettingsSection
      icon={<FileText />}
      id="document-handling"
      title="PDF 处理"
    >
      <div className="flex flex-col gap-6">
        <div className="flex flex-col gap-4">
          <div>
            <h3 className="text-[0.95rem] font-semibold leading-5">PDF 版面翻译组件</h3>
            <p className="mt-1 max-w-[72ch] text-sm leading-6 text-muted-foreground">
              生成尽量保留原始页面布局的译文 PDF。
            </p>
          </div>
          <Pdf2zhPanel />
        </div>

        <Separator />

        <div className="flex flex-col gap-4">
          <div>
            <h3 className="text-[0.95rem] font-semibold leading-5">PDF 转 Markdown 组件</h3>
            <p className="mt-1 max-w-[72ch] text-sm leading-6 text-muted-foreground">
              提取结构化 Markdown，用于分段翻译、双栏预览和 Markdown 导出。
            </p>
          </div>
          <PdfMarkdownPanel />
        </div>
      </div>
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
      icon={<Trash2 />}
      id="danger-settings"
      tone="danger"
      title="危险操作"
    >
      <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-start">
        <div className="flex min-w-0 flex-col gap-3">
          <div>
            <p className="text-[0.95rem] font-semibold leading-5">
              清除本机数据
            </p>
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
        "min-w-0",
        tone === "danger" && "text-destructive"
      )}
      id={id}
    >
      <div
        className={cn(
          "mb-5 flex min-w-0 items-start gap-3 border-b border-border/70 pb-5",
          tone === "danger" && "border-destructive/25"
        )}
      >
        <SettingsIconFrame tone={tone}>{icon}</SettingsIconFrame>
        <div className="min-w-0 space-y-1">
          <h2 className="text-xl font-semibold leading-7 tracking-normal">
            {title}
          </h2>
          {description ? (
            <div
              className={cn(
                "flex flex-wrap items-center gap-1.5 text-[0.95rem] leading-6 text-muted-foreground",
                tone === "danger" && "text-destructive/80"
              )}
            >
              {description}
            </div>
          ) : null}
        </div>
      </div>
      <div className="min-w-0 text-foreground">{children}</div>
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
        "flex size-8 shrink-0 items-center justify-center rounded-[8px] border border-border/70 bg-muted/35 text-muted-foreground [&_svg]:size-4",
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
        tone === "success" &&
          "border-emerald-500/25 bg-emerald-500/[0.07] text-emerald-800 dark:text-emerald-300",
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
        "h-5 gap-1.5 border-transparent px-1.5 text-xs font-medium ring-1 ring-inset ring-border/70",
        tone === "selected" && "rosetta-settings-accent-badge",
        tone === "success" &&
          "border-emerald-500/20 bg-emerald-500/10 text-emerald-800 ring-emerald-500/20 dark:text-emerald-300",
        tone === "warning" &&
          "border-amber-500/20 bg-amber-500/10 text-amber-800 ring-amber-500/20 dark:text-amber-300",
        tone === "danger" &&
          "border-destructive/20 bg-destructive/10 text-destructive ring-destructive/20",
        tone === "info" && "rosetta-settings-accent-badge",
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
    return null;
  }

  if (status === "available" && update) {
    return (
      <div className="flex flex-col gap-2 rounded-md border border-primary/30 bg-primary/5 p-3">
        <div className="flex flex-wrap items-center gap-2 text-xs font-medium text-muted-foreground">
          <span>新版本包含</span>
          <span className="rounded-sm bg-primary/10 px-1.5 py-0.5 text-[var(--rosetta-settings-accent-ink)]">
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

  return null;
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
        result.ok
          ? "border-emerald-500/25 bg-emerald-500/[0.07]"
          : "border-destructive/40 bg-destructive/5"
      )}
    >
      <div className="flex flex-wrap items-center gap-2 text-sm">
        {result.ok ? (
          <CheckCircle2 className="text-emerald-600 dark:text-emerald-400" />
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
