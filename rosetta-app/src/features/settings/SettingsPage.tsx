import { useEffect, useState, type ChangeEvent } from "react";
import {
  Activity,
  Archive,
  FolderPlus,
  PackageCheck,
  Play,
  RefreshCw,
  ScanLine,
  Send,
} from "lucide-react";
import { probeRwkvTranslationApi } from "../../lib/rwkvApi";
import {
  extractRwkvRuntimeArtifact,
  getRwkvRuntimeArtifactCatalog,
  getRwkvRuntimeInstallProgress,
  getRwkvRuntimeInstallPlan,
  getRwkvRuntimeProcessStatus,
  getRwkvRuntimeStatus,
  initializeRwkvRuntimeLayout,
  probeRwkvRuntimeTranslation,
  prepareRwkvRuntimeInstall,
  scanRwkvRuntimeArtifacts,
  startRwkvRuntime,
} from "../../lib/rwkvRuntime";
import { useRosettaStore } from "../../store/useRosettaStore";
import type {
  AppThemeMode,
  RwkvConnectionConfig,
  RwkvRuntimeArtifactCatalog,
  RwkvRuntimeArtifactCatalogItem,
  RwkvRuntimeArtifactScanResult,
  RwkvRuntimeExtractionResult,
  RwkvRuntimeInstallPlan,
  RwkvRuntimeInstallPlanItem,
  RwkvRuntimeInstallProgress,
  RwkvRuntimeInstallProgressItem,
  RwkvRuntimeProcessStatus,
  RwkvRuntimeStatus,
  RwkvRuntimeTranslationProbeResult,
  RwkvTranslationApiProbeResult,
  TranslationMode,
} from "../../types/rosetta";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";

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

const runtimeStateLabel: Record<RwkvRuntimeStatus["state"], string> = {
  "not-installed": "未安装",
  partial: "目录已准备",
  installed: "已安装",
  invalid: "需检查",
};

const installItemStateLabel: Record<RwkvRuntimeInstallPlanItem["state"], string> =
  {
    missing: "待准备",
    ready: "就绪",
    invalid: "需检查",
  };

const progressStateLabel: Record<RwkvRuntimeInstallProgress["state"], string> = {
  "not-started": "未开始",
  queued: "已排队",
  ready: "已就绪",
  blocked: "已阻塞",
};

const progressItemStateLabel: Record<
  RwkvRuntimeInstallProgressItem["state"],
  string
> = {
  pending: "待处理",
  ready: "就绪",
  blocked: "已阻塞",
};

const catalogItemStateLabel: Record<
  RwkvRuntimeArtifactCatalogItem["state"],
  string
> = {
  ready: "可下载",
};

const processStateLabel: Record<RwkvRuntimeProcessStatus["state"], string> = {
  stopped: "未启动",
  starting: "启动中",
  ready: "端口就绪",
};

export function SettingsPage() {
  const themeMode = useRosettaStore((state) => state.themeMode);
  const setThemeMode = useRosettaStore((state) => state.setThemeMode);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const updateRwkvConfig = useRosettaStore((state) => state.updateRwkvConfig);
  const setTranslationMode = useRosettaStore((state) => state.setTranslationMode);
  const [runtimeStatus, setRuntimeStatus] =
    useState<RwkvRuntimeStatus | null>(null);
  const [installPlan, setInstallPlan] =
    useState<RwkvRuntimeInstallPlan | null>(null);
  const [installProgress, setInstallProgress] =
    useState<RwkvRuntimeInstallProgress | null>(null);
  const [artifactCatalog, setArtifactCatalog] =
    useState<RwkvRuntimeArtifactCatalog | null>(null);
  const [scanResult, setScanResult] =
    useState<RwkvRuntimeArtifactScanResult | null>(null);
  const [extractionResult, setExtractionResult] =
    useState<RwkvRuntimeExtractionResult | null>(null);
  const [processStatus, setProcessStatus] =
    useState<RwkvRuntimeProcessStatus | null>(null);
  const [translationProbeResult, setTranslationProbeResult] =
    useState<RwkvRuntimeTranslationProbeResult | null>(null);
  const [apiProbeResult, setApiProbeResult] =
    useState<RwkvTranslationApiProbeResult | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [apiError, setApiError] = useState<string | null>(null);
  const [isCheckingRuntime, setIsCheckingRuntime] = useState(false);
  const [isPreparingRuntime, setIsPreparingRuntime] = useState(false);
  const [isPreparingInstall, setIsPreparingInstall] = useState(false);
  const [isScanningArtifacts, setIsScanningArtifacts] = useState(false);
  const [isExtractingRuntime, setIsExtractingRuntime] = useState(false);
  const [isStartingRuntime, setIsStartingRuntime] = useState(false);
  const [isProbingTranslation, setIsProbingTranslation] = useState(false);
  const [isProbingApi, setIsProbingApi] = useState(false);

  async function refreshRuntimeStatus() {
    setIsCheckingRuntime(true);
    setRuntimeError(null);

    try {
      const [nextRuntimeStatus, nextInstallPlan] = await Promise.all([
        getRwkvRuntimeStatus(),
        getRwkvRuntimeInstallPlan(),
      ]);
      const [nextInstallProgress, nextArtifactCatalog] = await Promise.all([
        getRwkvRuntimeInstallProgress(),
        getRwkvRuntimeArtifactCatalog(),
      ]);
      const nextProcessStatus = await getRwkvRuntimeProcessStatus();
      setRuntimeStatus(nextRuntimeStatus);
      setInstallPlan(nextInstallPlan);
      setInstallProgress(nextInstallProgress);
      setArtifactCatalog(nextArtifactCatalog);
      setProcessStatus(nextProcessStatus);
    } catch (error) {
      setRuntimeStatus(null);
      setInstallPlan(null);
      setInstallProgress(null);
      setArtifactCatalog(null);
      setScanResult(null);
      setExtractionResult(null);
      setProcessStatus(null);
      setTranslationProbeResult(null);
      setRuntimeError(
        error instanceof Error ? error.message : "无法读取本地 RWKV 状态。"
      );
    } finally {
      setIsCheckingRuntime(false);
    }
  }

  useEffect(() => {
    void refreshRuntimeStatus();
  }, []);

  async function prepareRuntimeLayout() {
    setIsPreparingRuntime(true);
    setRuntimeError(null);

    try {
      setRuntimeStatus(await initializeRwkvRuntimeLayout());
      setInstallPlan(await getRwkvRuntimeInstallPlan());
      setInstallProgress(await getRwkvRuntimeInstallProgress());
      setArtifactCatalog(await getRwkvRuntimeArtifactCatalog());
      setProcessStatus(await getRwkvRuntimeProcessStatus());
    } catch (error) {
      setRuntimeError(
        error instanceof Error ? error.message : "无法准备本地 RWKV 目录。"
      );
    } finally {
      setIsPreparingRuntime(false);
    }
  }

  async function prepareInstall() {
    setIsPreparingInstall(true);
    setRuntimeError(null);

    try {
      setInstallProgress(await prepareRwkvRuntimeInstall());
      setRuntimeStatus(await getRwkvRuntimeStatus());
      setInstallPlan(await getRwkvRuntimeInstallPlan());
      setArtifactCatalog(await getRwkvRuntimeArtifactCatalog());
      setProcessStatus(await getRwkvRuntimeProcessStatus());
    } catch (error) {
      setRuntimeError(
        error instanceof Error ? error.message : "无法准备本地 RWKV 安装任务。"
      );
    } finally {
      setIsPreparingInstall(false);
    }
  }

  async function scanArtifacts() {
    setIsScanningArtifacts(true);
    setRuntimeError(null);

    try {
      const nextScanResult = await scanRwkvRuntimeArtifacts();
      setScanResult(nextScanResult);
      setInstallPlan(nextScanResult.plan);
      setRuntimeStatus(await getRwkvRuntimeStatus());
      setInstallProgress(await getRwkvRuntimeInstallProgress());
      setArtifactCatalog(await getRwkvRuntimeArtifactCatalog());
      setProcessStatus(await getRwkvRuntimeProcessStatus());
    } catch (error) {
      setRuntimeError(
        error instanceof Error ? error.message : "无法扫描本地 RWKV artifact。"
      );
    } finally {
      setIsScanningArtifacts(false);
    }
  }

  async function extractRuntime() {
    setIsExtractingRuntime(true);
    setRuntimeError(null);

    try {
      const nextExtractionResult = await extractRwkvRuntimeArtifact();
      setExtractionResult(nextExtractionResult);
      setInstallPlan(nextExtractionResult.plan);
      setRuntimeStatus(await getRwkvRuntimeStatus());
      setInstallProgress(await getRwkvRuntimeInstallProgress());
      setArtifactCatalog(await getRwkvRuntimeArtifactCatalog());
      setProcessStatus(await getRwkvRuntimeProcessStatus());
    } catch (error) {
      setRuntimeError(
        error instanceof Error ? error.message : "无法解压本地 RWKV runtime。"
      );
    } finally {
      setIsExtractingRuntime(false);
    }
  }

  async function startRuntime() {
    setIsStartingRuntime(true);
    setRuntimeError(null);

    try {
      const startResult = await startRwkvRuntime();
      setProcessStatus(startResult.process);
      setRuntimeStatus(await getRwkvRuntimeStatus());
      setInstallPlan(await getRwkvRuntimeInstallPlan());
      setInstallProgress(await getRwkvRuntimeInstallProgress());
    } catch (error) {
      setRuntimeError(
        error instanceof Error ? error.message : "无法启动本地 RWKV runtime。"
      );
    } finally {
      setIsStartingRuntime(false);
    }
  }

  async function probeTranslation() {
    setIsProbingTranslation(true);
    setRuntimeError(null);

    try {
      const probeResult = await probeRwkvRuntimeTranslation();
      setTranslationProbeResult(probeResult);
      setProcessStatus(probeResult.process);
    } catch (error) {
      setRuntimeError(
        error instanceof Error ? error.message : "无法测试本地 RWKV 翻译接口。"
      );
    } finally {
      setIsProbingTranslation(false);
    }
  }

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

  const canProbeApi =
    rwkv.baseUrl.trim().length > 0 &&
    rwkv.endpoint.trim().length > 0 &&
    rwkv.internalToken.trim().length > 0 &&
    rwkv.bodyPassword.trim().length > 0 &&
    rwkv.timeoutMs > 0 &&
    !isProbingApi;
  const isManagedRuntimePaused = true;

  return (
    <section className="mx-auto flex max-w-3xl flex-col gap-4 px-6 py-6">
      <Card>
        <CardHeader>
          <CardTitle>外观</CardTitle>
          <CardDescription>设置 Rosetta 的界面主题</CardDescription>
        </CardHeader>

        <CardContent>
          <div className="flex flex-col gap-2">
            <Label>主题</Label>
            <ToggleGroup
              className="w-full"
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
                <ToggleGroupItem
                  className="flex-1"
                  key={option.value}
                  value={option.value}
                >
                  {option.label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-start justify-between gap-3">
            <div>
              <CardTitle>RWKV API</CardTitle>
              <CardDescription>
                当前主路径：连接已部署的 RWKV 批量翻译接口
              </CardDescription>
            </div>
            <Badge variant="outline">当前使用</Badge>
          </div>
        </CardHeader>

        <CardContent>
          <div className="flex flex-col gap-4">
            <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3 text-xs text-muted-foreground">
              <p>
                远程或云端 API 是显式配置的后端选项。测试时，样本文本会发送到该
                API。
              </p>
              <p>
                真实 token 只保存在本机设置中，不应写入代码、文档或测试 fixture。
              </p>
            </div>

            <div className="flex flex-col gap-2">
              <Label htmlFor="rwkv-base-url">API 地址</Label>
              <Input
                id="rwkv-base-url"
                onChange={updateTextField("baseUrl")}
                value={rwkv.baseUrl}
              />
            </div>

            <div className="flex flex-col gap-2">
              <Label htmlFor="rwkv-endpoint">端点</Label>
              <Input
                id="rwkv-endpoint"
                onChange={updateTextField("endpoint")}
                value={rwkv.endpoint}
              />
            </div>

            <div className="flex flex-col gap-2">
              <Label htmlFor="rwkv-internal-token">X-Internal-Token</Label>
              <Input
                id="rwkv-internal-token"
                onChange={updateTextField("internalToken")}
                type="password"
                value={rwkv.internalToken}
              />
            </div>

            <div className="flex flex-col gap-2">
              <Label htmlFor="rwkv-body-password">Body password</Label>
              <Input
                id="rwkv-body-password"
                onChange={updateTextField("bodyPassword")}
                type="password"
                value={rwkv.bodyPassword}
              />
            </div>

            <div className="flex flex-col gap-2">
              <Label htmlFor="rwkv-timeout">Timeout ms</Label>
              <Input
                id="rwkv-timeout"
                min={1}
                onChange={updateTimeout}
                type="number"
                value={rwkv.timeoutMs}
              />
            </div>

            <div className="flex flex-col gap-2">
              <Label>翻译模式</Label>
              <ToggleGroup
                className="w-full"
                onValueChange={(value) => {
                  if (value) {
                    setTranslationMode(value as TranslationMode);
                  }
                }}
                type="single"
                value={rwkv.mode}
                variant="outline"
              >
                {modeOptions.map((option) => (
                  <ToggleGroupItem
                    className="flex-1"
                    key={option.value}
                    value={option.value}
                  >
                    {option.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            <div className="flex items-center gap-2">
              <Button
                disabled={!canProbeApi}
                onClick={() => void probeApi()}
                title="测试 RWKV API"
                type="button"
                variant="outline"
              >
                <Send />
                {isProbingApi ? "测试中" : "测试 API"}
              </Button>
              {apiProbeResult ? (
                <Badge variant="outline">
                  {apiProbeResult.ok ? "成功" : "失败"}
                </Badge>
              ) : null}
            </div>

            {apiError ? (
              <p className="text-sm text-destructive">{apiError}</p>
            ) : null}

            {apiProbeResult ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">API 探测</span>
                  <Badge variant="outline">
                    {apiProbeResult.ok ? "成功" : "失败"}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {apiProbeResult.message}
                </p>
                <p className="font-mono text-xs text-muted-foreground">
                  status: {apiProbeResult.statusCode ?? "none"} / latency:{" "}
                  {apiProbeResult.latencyMs}ms
                </p>
                {apiProbeResult.translations.length > 0 ? (
                  <div className="grid gap-2">
                    {apiProbeResult.translations.map((translation, index) => (
                      <div
                        className="grid gap-1 rounded-md border border-border bg-background/60 p-2 text-xs"
                        key={`api-probe-${index}`}
                      >
                        <span className="font-medium text-foreground">
                          Translation {index + 1}
                        </span>
                        <p className="text-muted-foreground">{translation}</p>
                      </div>
                    ))}
                  </div>
                ) : null}
                {apiProbeResult.rawResponsePreview ? (
                  <pre className="max-h-40 overflow-auto rounded-sm bg-background/80 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
                    {apiProbeResult.rawResponsePreview}
                  </pre>
                ) : null}
              </div>
            ) : null}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-start justify-between gap-3">
            <div>
              <CardTitle>本地 RWKV</CardTitle>
              <CardDescription>
                已暂停：Rosetta 托管的本地翻译模型运行时
              </CardDescription>
            </div>
            <div className="flex items-center gap-2">
              <Badge variant="outline">已暂停</Badge>
              <Button
                disabled={
                  isManagedRuntimePaused ||
                  runtimeStatus?.runtimeExecutableExists ||
                  isCheckingRuntime ||
                  isPreparingRuntime ||
                  isPreparingInstall ||
                  isScanningArtifacts ||
                  isExtractingRuntime ||
                  isStartingRuntime ||
                  isProbingTranslation
                }
                onClick={() => void prepareRuntimeLayout()}
                title="准备本地 RWKV 目录"
                type="button"
                variant="outline"
              >
                <FolderPlus />
                准备目录
              </Button>
              <Button
                disabled={
                  isManagedRuntimePaused ||
                  isCheckingRuntime ||
                  isPreparingRuntime ||
                  isPreparingInstall ||
                  isScanningArtifacts ||
                  isExtractingRuntime ||
                  isStartingRuntime ||
                  isProbingTranslation
                }
                onClick={() => void prepareInstall()}
                title="准备本地 RWKV 安装任务"
                type="button"
                variant="outline"
              >
                <PackageCheck />
                准备安装
              </Button>
              <Button
                disabled={
                  isManagedRuntimePaused ||
                  isCheckingRuntime ||
                  isPreparingRuntime ||
                  isPreparingInstall ||
                  isScanningArtifacts ||
                  isExtractingRuntime ||
                  isStartingRuntime ||
                  isProbingTranslation
                }
                onClick={() => void scanArtifacts()}
                title="扫描已放入管理目录的 RWKV 文件"
                type="button"
                variant="outline"
              >
                <ScanLine />
                扫描文件
              </Button>
              <Button
                disabled={
                  isManagedRuntimePaused ||
                  isCheckingRuntime ||
                  isPreparingRuntime ||
                  isPreparingInstall ||
                  isScanningArtifacts ||
                  isExtractingRuntime ||
                  isStartingRuntime ||
                  isProbingTranslation
                }
                onClick={() => void extractRuntime()}
                title="解压已校验的 RWKV runtime"
                type="button"
                variant="outline"
              >
                <Archive />
                {runtimeStatus?.runtimeExecutableExists ? "已解压" : "解压运行时"}
              </Button>
              <Button
                disabled={
                  isManagedRuntimePaused ||
                  runtimeStatus?.compatibility.compatible === false ||
                  !installPlan?.ready ||
                  processStatus?.state === "ready" ||
                  isCheckingRuntime ||
                  isPreparingRuntime ||
                  isPreparingInstall ||
                  isScanningArtifacts ||
                  isExtractingRuntime ||
                  isStartingRuntime ||
                  isProbingTranslation
                }
                onClick={() => void startRuntime()}
                title="启动本地 RWKV runtime"
                type="button"
                variant="outline"
              >
                <Play />
                {processStatus?.state === "ready" ? "已启动" : "启动"}
              </Button>
              <Button
                disabled={
                  isManagedRuntimePaused ||
                  processStatus?.state !== "ready" ||
                  isCheckingRuntime ||
                  isPreparingRuntime ||
                  isPreparingInstall ||
                  isScanningArtifacts ||
                  isExtractingRuntime ||
                  isStartingRuntime ||
                  isProbingTranslation
                }
                onClick={() => void probeTranslation()}
                title="测试本地 RWKV 翻译接口"
                type="button"
                variant="outline"
              >
                <Activity />
                {isProbingTranslation ? "测试中" : "测试翻译"}
              </Button>
              <Button
                disabled={
                  isCheckingRuntime ||
                  isPreparingRuntime ||
                  isPreparingInstall ||
                  isScanningArtifacts ||
                  isExtractingRuntime ||
                  isStartingRuntime ||
                  isProbingTranslation
                }
                onClick={() => void refreshRuntimeStatus()}
                size="icon"
                title="刷新本地 RWKV 状态"
                type="button"
                variant="outline"
              >
                <RefreshCw
                  className={isCheckingRuntime ? "animate-spin" : undefined}
                />
              </Button>
            </div>
          </div>
        </CardHeader>

        <CardContent>
          <div className="flex flex-col gap-4 text-sm">
            <div className="grid gap-2 rounded-md border border-border bg-muted/30 p-3 text-xs text-muted-foreground">
              <p>
                本地一键运行 RWKV LLM 已暂停，当前翻译开发使用上方的外部 RWKV
                API。
              </p>
              <p>
                这里保留已有诊断信息，避免丢失后续恢复本地 runtime 工作所需的上下文。
              </p>
            </div>

            <div className="flex items-center gap-2">
              <span className="text-muted-foreground">状态</span>
              <Badge variant="outline">
                {runtimeStatus
                  ? runtimeStateLabel[runtimeStatus.state]
                  : runtimeError
                    ? "不可用"
                    : "检查中"}
              </Badge>
            </div>

            <p className="text-muted-foreground">
              {runtimeStatus?.message ??
                runtimeError ??
                "正在读取本机运行时状态。"}
            </p>

            {runtimeStatus ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3 text-xs">
                <RuntimePath label="API" value={runtimeStatus.apiUrl} />
                <RuntimePath label="Runtime" value={runtimeStatus.runtimeDir} />
                <RuntimePath
                  label="Runtime bundle"
                  value={runtimeStatus.runtimeBundleDir}
                />
                <RuntimePath
                  label="Executable"
                  value={runtimeStatus.runtimeExecutablePath}
                />
                <RuntimePath label="Model" value={runtimeStatus.modelDir} />
                <RuntimePath label="Logs" value={runtimeStatus.logsDir} />
                <RuntimePath label="Log" value={runtimeStatus.logFile} />
              </div>
            ) : null}

            {runtimeStatus?.manifestError ? (
              <p className="text-sm text-destructive">
                {runtimeStatus.manifestError}
              </p>
            ) : null}

            {runtimeStatus ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">硬件兼容性</span>
                  <Badge variant="outline">
                    {runtimeStatus.compatibility.compatible ? "可尝试" : "不兼容"}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {runtimeStatus.compatibility.message}
                </p>
                <div className="grid gap-1 font-mono text-xs text-muted-foreground">
                  <p>backend: {runtimeStatus.compatibility.runtimeBackend}</p>
                  <p>
                    requires: {runtimeStatus.compatibility.hardwareRequirement}
                  </p>
                  <p>
                    display:{" "}
                    {runtimeStatus.compatibility.detectedDisplayAdapters.length >
                    0
                      ? runtimeStatus.compatibility.detectedDisplayAdapters.join(
                          " / "
                        )
                      : "unknown"}
                  </p>
                </div>
              </div>
            ) : null}

            {installPlan ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">安装计划</span>
                  <Badge variant="outline">
                    {installPlan.ready ? "已满足" : "未完成"}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {installPlan.message}
                </p>
                <div className="grid gap-2">
                  {installPlan.items.map((item) => (
                    <InstallPlanItem key={`${item.kind}-${item.id}`} item={item} />
                  ))}
                </div>
              </div>
            ) : null}

            {installProgress ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">安装进度</span>
                  <Badge variant="outline">
                    {progressStateLabel[installProgress.state]}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {installProgress.message}
                </p>
                <div className="grid gap-2">
                  {installProgress.items.map((item) => (
                    <InstallProgressItem
                      item={item}
                      key={`progress-${item.kind}-${item.id}`}
                    />
                  ))}
                </div>
              </div>
            ) : null}

            {processStatus ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">运行进程</span>
                  <Badge variant="outline">
                    {processStateLabel[processStatus.state]}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {processStatus.message}
                </p>
                <div className="grid gap-2 text-xs">
                  <RuntimePath label="API" value={processStatus.apiUrl} />
                  <RuntimePath label="PID file" value={processStatus.pidFile} />
                  <RuntimePath label="Log" value={processStatus.logFile} />
                  <p className="font-mono text-muted-foreground">
                    pid: {processStatus.pid ?? "none"} / port:{" "}
                    {processStatus.port} / open:{" "}
                    {processStatus.portOpen ? "yes" : "no"}
                  </p>
                  <p className="font-mono text-muted-foreground">
                    process:{" "}
                    {processStatus.processRunning == null
                      ? "unknown"
                      : processStatus.processRunning
                        ? "running"
                        : "stopped"}{" "}
                    / http: {processStatus.httpReady ? "ready" : "not-ready"} /
                    status: {processStatus.httpStatusCode ?? "none"}
                  </p>
                  {processStatus.logTail.length > 0 ? (
                    <pre className="max-h-40 overflow-auto rounded-sm bg-background/80 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
                      {processStatus.logTail.join("\n")}
                    </pre>
                  ) : null}
                </div>
              </div>
            ) : null}

            {translationProbeResult ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">翻译探测</span>
                  <Badge variant="outline">
                    {translationProbeResult.ok ? "成功" : "失败"}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {translationProbeResult.message}
                </p>
                <p className="font-mono text-xs text-muted-foreground">
                  status: {translationProbeResult.statusCode ?? "none"}
                </p>
                {translationProbeResult.responseBodyPreview ? (
                  <pre className="max-h-40 overflow-auto rounded-sm bg-background/80 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
                    {translationProbeResult.responseBodyPreview}
                  </pre>
                ) : null}
              </div>
            ) : null}

            {artifactCatalog ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">Artifact Catalog</span>
                  <Badge variant="outline">
                    {artifactCatalog.readyForDownload ? "可下载" : "待确认"}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {artifactCatalog.message}
                </p>
                <div className="grid gap-2">
                  {artifactCatalog.items.map((item) => (
                    <ArtifactCatalogItem
                      item={item}
                      key={`catalog-${item.kind}-${item.id}`}
                    />
                  ))}
                </div>
              </div>
            ) : null}

            {scanResult ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">扫描结果</span>
                  <Badge variant="outline">
                    {scanResult.errors.length > 0 ? "需检查" : "已扫描"}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {scanResult.message}
                </p>
                {scanResult.installedManifests.length > 0 ? (
                  <div className="grid gap-2">
                    {scanResult.installedManifests.map((manifestPath) => (
                      <RuntimePath
                        key={manifestPath}
                        label="Manifest"
                        value={manifestPath}
                      />
                    ))}
                  </div>
                ) : null}
                {scanResult.errors.length > 0 ? (
                  <div className="grid gap-1 text-xs text-destructive">
                    {scanResult.errors.map((error) => (
                      <p key={error}>{error}</p>
                    ))}
                  </div>
                ) : null}
              </div>
            ) : null}

            {extractionResult ? (
              <div className="grid gap-3 rounded-md border border-border bg-muted/30 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="font-medium text-foreground">解压结果</span>
                  <Badge variant="outline">
                    {extractionResult.extracted ? "已解压" : "未解压"}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {extractionResult.message}
                </p>
                <div className="grid gap-2">
                  <RuntimePath label="Runtime bundle" value={extractionResult.targetDir} />
                  <RuntimePath
                    label="Executable"
                    value={extractionResult.executablePath}
                  />
                  <p className="font-mono text-xs text-muted-foreground">
                    {extractionResult.filesExtracted} files /{" "}
                    {extractionResult.bytesExtracted} bytes
                  </p>
                </div>
              </div>
            ) : null}
          </div>
        </CardContent>
      </Card>

    </section>
  );
}

function RuntimePath({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-1">
      <span className="font-medium text-foreground">{label}</span>
      <span className="break-all font-mono text-muted-foreground">{value}</span>
    </div>
  );
}

function InstallPlanItem({ item }: { item: RwkvRuntimeInstallPlanItem }) {
  return (
    <div className="grid gap-1 rounded-md border border-border bg-background/60 p-2 text-xs">
      <div className="flex items-center justify-between gap-3">
        <span className="font-medium text-foreground">{item.label}</span>
        <Badge variant="outline">{installItemStateLabel[item.state]}</Badge>
      </div>
      <p className="text-muted-foreground">{item.message}</p>
      <RuntimePath label="Manifest" value={item.manifestPath} />
      {item.artifactPath ? (
        <RuntimePath label="Artifact" value={item.artifactPath} />
      ) : null}
    </div>
  );
}

function InstallProgressItem({ item }: { item: RwkvRuntimeInstallProgressItem }) {
  return (
    <div className="grid gap-1 rounded-md border border-border bg-background/60 p-2 text-xs">
      <div className="flex items-center justify-between gap-3">
        <span className="font-medium text-foreground">{item.label}</span>
        <Badge variant="outline">{progressItemStateLabel[item.state]}</Badge>
      </div>
      <p className="text-muted-foreground">{item.message}</p>
      {item.bytesTotal ? (
        <p className="font-mono text-muted-foreground">
          {item.bytesDone} / {item.bytesTotal} bytes
        </p>
      ) : null}
    </div>
  );
}

function ArtifactCatalogItem({ item }: { item: RwkvRuntimeArtifactCatalogItem }) {
  return (
    <div className="grid gap-1 rounded-md border border-border bg-background/60 p-2 text-xs">
      <div className="flex items-center justify-between gap-3">
        <span className="font-medium text-foreground">{item.label}</span>
        <Badge variant="outline">{catalogItemStateLabel[item.state]}</Badge>
      </div>
      <p className="text-muted-foreground">{item.message}</p>
      <RuntimePath label="Target" value={item.targetDir} />
      <RuntimePath label="Manifest" value={item.manifestPath} />
      {item.artifactFilename ? (
        <RuntimePath label="Artifact" value={item.artifactFilename} />
      ) : null}
      {item.sourcePage ? <RuntimePath label="Source" value={item.sourcePage} /> : null}
      {item.downloadUrl ? (
        <RuntimePath label="Download" value={item.downloadUrl} />
      ) : null}
    </div>
  );
}
