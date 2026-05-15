import { useEffect, useState, type ChangeEvent } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import {
  CheckCircle2,
  ChevronDown,
  Cloud,
  Download,
  Palette,
  RefreshCw,
  Rocket,
  Send,
  ShieldCheck,
  Timer,
  XCircle,
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { probeRwkvTranslationApi } from "../../lib/rwkvApi";
import { cn } from "../../lib/utils";
import { useRosettaStore } from "../../store/useRosettaStore";
import { LocalRwkvPanel } from "./LocalRwkvPanel";
import type {
  AppThemeMode,
  RwkvConnectionConfig,
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
        error instanceof Error ? error.message : "无法测试 RWKV API。"
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
        error instanceof Error ? error.message : "检查更新失败。"
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
        error instanceof Error ? error.message : "下载或安装更新失败。"
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
        error instanceof Error ? error.message : "重启应用失败。"
      );
    }
  }

  const missingConnectionFields = [
    !rwkv.baseUrl.trim() && "API 地址",
    !rwkv.endpoint.trim() && "接口路径",
    !rwkv.internalToken.trim() && "访问密钥",
    !rwkv.bodyPassword.trim() && "模型口令",
    rwkv.timeoutMs <= 0 && "超时时间",
  ].filter(Boolean) as string[];
  const canProbeApi = missingConnectionFields.length === 0 && !isProbingApi;
  const apiStatus = apiProbeResult?.ok
    ? "connected"
    : apiProbeResult || apiError
      ? "failed"
      : "not-tested";

  return (
    <ScrollArea className="h-full w-full">
      <section className="mx-auto flex w-full max-w-4xl flex-col gap-6 px-6 py-6 mb-20">
        <header className="flex flex-col gap-2">
          <h1 className="text-2xl font-semibold tracking-normal">设置</h1>
          <p className="max-w-xl text-sm text-muted-foreground">
            管理 Rosetta 的翻译服务、显示方式和本地模型选项。
          </p>
        </header>

        <main className="flex w-full flex-col gap-20">
          <section className="flex flex-col gap-3" id="appearance">
            <SettingsSectionHeader
              description="调整 Rosetta 的显示方式。"
              icon={<Palette />}
              title="界面外观"
            />

            <Card>
              <CardHeader>
                <CardTitle>主题</CardTitle>
                <CardDescription>选择浅色、深色，或跟随系统。</CardDescription>
              </CardHeader>
              <CardContent>
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
              </CardContent>
            </Card>
          </section>

          <section className="flex flex-col gap-3" id="app-update">
            <SettingsSectionHeader
              description="手动检查内部测试版更新。"
              icon={<Rocket />}
              title="应用更新"
            >
              <Badge variant="outline">Windows Beta</Badge>
            </SettingsSectionHeader>

            <Card>
              <CardHeader>
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <CardTitle>Rosetta {appVersion}</CardTitle>
                    <CardDescription>
                      更新包会通过 GitHub Release 分发，并使用 Tauri updater
                      签名校验。
                    </CardDescription>
                  </div>
                  <UpdateStatusBadge status={updateStatus} />
                </div>
              </CardHeader>
              <CardContent className="flex flex-col gap-4">
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    disabled={
                      updateStatus === "checking" ||
                      updateStatus === "downloading" ||
                      updateStatus === "installing"
                    }
                    onClick={() => void checkForUpdate()}
                    type="button"
                    variant="outline"
                  >
                    <RefreshCw
                      className={
                        updateStatus === "checking" ? "animate-spin" : undefined
                      }
                      data-icon="inline-start"
                    />
                    检查更新
                  </Button>

                  {updateStatus === "available" && availableUpdate ? (
                    <Button
                      onClick={() => void installAvailableUpdate()}
                      type="button"
                    >
                      <Download data-icon="inline-start" />
                      下载并安装
                    </Button>
                  ) : null}

                  {updateStatus === "ready-to-restart" ? (
                    <Button onClick={() => void restartApp()} type="button">
                      <RefreshCw data-icon="inline-start" />
                      重启完成更新
                    </Button>
                  ) : null}
                </div>

                <UpdateStatusMessage
                  error={updateError}
                  progress={downloadProgress}
                  status={updateStatus}
                  update={availableUpdate}
                />
              </CardContent>
            </Card>
          </section>

          <LocalRwkvPanel />

          <section className="flex flex-col gap-3" id="translation-service">
            <Collapsible open={externalApiOpen} onOpenChange={setExternalApiOpen}>
              <div className="flex items-start justify-between gap-4">
                <SettingsSectionHeader
                  description="本地翻译未就绪时可回落到远程或自部署接口。"
                  icon={<Cloud />}
                  title="外部翻译 API"
                >
                  <StatusBadge status={apiStatus} />
                </SettingsSectionHeader>
                <CollapsibleTrigger asChild>
                  <button
                    type="button"
                    className="mt-1 flex shrink-0 items-center gap-1 rounded px-2 py-1 text-xs text-muted-foreground/60 transition-colors hover:text-muted-foreground"
                  >
                    <ChevronDown
                      className={cn(
                        "size-3.5 transition-transform",
                        externalApiOpen && "rotate-180"
                      )}
                    />
                    {externalApiOpen ? "收起" : "展开"}
                  </button>
                </CollapsibleTrigger>
              </div>

              <CollapsibleContent>
                <Card className="mt-3 overflow-hidden">
                  <CardContent className="flex flex-col gap-5 py-5">
                    <div className="grid gap-4 md:grid-cols-2">
                      <SettingField htmlFor="rwkv-base-url" label="API 地址">
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
                          placeholder="/v1/chat/completions"
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

                      <SettingField htmlFor="rwkv-body-password" label="模型口令">
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
                        <Label>翻译偏好</Label>
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
                          onClick={() => void probeApi()}
                          type="button"
                          variant={apiStatus === "connected" ? "outline" : "default"}
                        >
                          <Send data-icon="inline-start" />
                          {isProbingApi ? "测试中…" : "测试连接"}
                        </Button>
                      </div>

                      {missingConnectionFields.length > 0 && (
                        <p className="text-xs text-muted-foreground">
                          还需要填写：{missingConnectionFields.join("、")}。
                        </p>
                      )}
                      {apiError && (
                        <p className="text-sm text-destructive">{apiError}</p>
                      )}
                      {apiProbeResult && (
                        <ApiProbeResult result={apiProbeResult} />
                      )}
                    </div>
                  </CardContent>
                </Card>
              </CollapsibleContent>
            </Collapsible>
          </section>

        </main>
      </section>
    </ScrollArea>
  );
}

function SettingsSectionHeader({
  children,
  description,
  icon,
  title,
}: {
  children?: React.ReactNode;
  description: string;
  icon: React.ReactNode;
  title: string;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="flex min-w-0 gap-3">
        <SettingsIconFrame>{icon}</SettingsIconFrame>
        <div className="min-w-0">
          <h2 className="text-lg font-semibold tracking-normal">{title}</h2>
          <p className="mt-1 text-sm text-muted-foreground">{description}</p>
        </div>
      </div>
      {children ? <div className="shrink-0">{children}</div> : null}
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

function StatusBadge({
  status,
}: {
  status: "connected" | "failed" | "not-tested";
}) {
  if (status === "connected") {
    return (
      <Badge variant="secondary">
        <CheckCircle2 data-icon="inline-start" />
        可用
      </Badge>
    );
  }
  if (status === "failed") {
    return (
      <Badge variant="destructive">
        <XCircle data-icon="inline-start" />
        需检查
      </Badge>
    );
  }
  return <Badge variant="outline">未测试</Badge>;
}

function UpdateStatusBadge({ status }: { status: UpdateStatus }) {
  if (status === "latest") {
    return (
      <Badge variant="secondary">
        <CheckCircle2 data-icon="inline-start" />
        已是最新
      </Badge>
    );
  }

  if (status === "available") {
    return <Badge>发现新版本</Badge>;
  }

  if (
    status === "checking" ||
    status === "downloading" ||
    status === "installing"
  ) {
    return <Badge variant="outline">处理中</Badge>;
  }

  if (status === "ready-to-restart") {
    return (
      <Badge variant="secondary">
        <CheckCircle2 data-icon="inline-start" />
        等待重启
      </Badge>
    );
  }

  if (status === "failed") {
    return (
      <Badge variant="destructive">
        <XCircle data-icon="inline-start" />
        失败
      </Badge>
    );
  }

  return <Badge variant="outline">未检查</Badge>;
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
        {error ?? "更新失败，请稍后重试。"}
      </p>
    );
  }

  if (status === "latest") {
    return (
      <p className="text-sm text-muted-foreground">
        当前已经是最新版本，可以继续使用。
      </p>
    );
  }

  if (status === "available" && update) {
    return (
      <div className="flex flex-col gap-3 rounded-md border bg-muted/30 p-3">
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <span className="font-medium">发现 Rosetta {update.version}</span>
          {update.date ? (
            <span className="text-muted-foreground">{update.date}</span>
          ) : null}
        </div>
        {update.body ? (
          <p className="whitespace-pre-wrap text-sm leading-6 text-muted-foreground">
            {update.body}
          </p>
        ) : (
          <p className="text-sm text-muted-foreground">
            这个版本没有填写更新说明。
          </p>
        )}
      </div>
    );
  }

  if (status === "downloading") {
    return (
      <p className="text-sm text-muted-foreground">
        正在下载更新
        {progress.total
          ? `：${formatBytes(progress.downloaded)} / ${formatBytes(
            progress.total
          )}`
          : progress.downloaded > 0
            ? `：${formatBytes(progress.downloaded)}`
            : "。"}
      </p>
    );
  }

  if (status === "installing") {
    return (
      <p className="text-sm text-muted-foreground">
        正在安装更新，请不要关闭 Rosetta。
      </p>
    );
  }

  if (status === "ready-to-restart") {
    return (
      <p className="text-sm text-muted-foreground">
        更新已安装，重启后会进入新版本。
      </p>
    );
  }

  if (status === "checking") {
    return <p className="text-sm text-muted-foreground">正在检查更新。</p>;
  }

  return (
    <p className="text-sm text-muted-foreground">
      内部测试阶段不会自动弹窗更新，需要在这里手动检查。
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
          {result.ok ? "连接正常" : "连接失败"}
        </span>
        <span className="text-muted-foreground">{result.message}</span>
      </div>

      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <span className="inline-flex items-center gap-1">
          <Timer />
          {result.latencyMs}ms
        </span>
        <span>HTTP {result.statusCode ?? "none"}</span>
      </div>

      {result.translations.length > 0 ? (
        <div className="grid gap-2">
          {result.translations.map((translation, index) => (
            <div className="rounded-md bg-muted/40 p-2 text-sm" key={index}>
              <p className="text-xs text-muted-foreground">
                样例译文 {index + 1}
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
