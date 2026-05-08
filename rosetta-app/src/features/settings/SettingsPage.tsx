import { useEffect, useState, type ChangeEvent } from "react";
import { FolderPlus, PackageCheck, RefreshCw } from "lucide-react";
import {
  getRwkvRuntimeArtifactCatalog,
  getRwkvRuntimeInstallProgress,
  getRwkvRuntimeInstallPlan,
  getRwkvRuntimeStatus,
  initializeRwkvRuntimeLayout,
  prepareRwkvRuntimeInstall,
} from "../../lib/rwkvRuntime";
import { useRosettaStore } from "../../store/useRosettaStore";
import type {
  AppThemeMode,
  RwkvConnectionConfig,
  RwkvRuntimeArtifactCatalog,
  RwkvRuntimeArtifactCatalogItem,
  RwkvRuntimeInstallPlan,
  RwkvRuntimeInstallPlanItem,
  RwkvRuntimeInstallProgress,
  RwkvRuntimeInstallProgressItem,
  RwkvRuntimeStatus,
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
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [isCheckingRuntime, setIsCheckingRuntime] = useState(false);
  const [isPreparingRuntime, setIsPreparingRuntime] = useState(false);
  const [isPreparingInstall, setIsPreparingInstall] = useState(false);

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
      setRuntimeStatus(nextRuntimeStatus);
      setInstallPlan(nextInstallPlan);
      setInstallProgress(nextInstallProgress);
      setArtifactCatalog(nextArtifactCatalog);
    } catch (error) {
      setRuntimeStatus(null);
      setInstallPlan(null);
      setInstallProgress(null);
      setArtifactCatalog(null);
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
    } catch (error) {
      setRuntimeError(
        error instanceof Error ? error.message : "无法准备本地 RWKV 安装任务。"
      );
    } finally {
      setIsPreparingInstall(false);
    }
  }

  function updateTextField(field: keyof Pick<RwkvConnectionConfig, "baseUrl">) {
    return (event: ChangeEvent<HTMLInputElement>) => {
      updateRwkvConfig({ [field]: event.currentTarget.value });
    };
  }

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
              <CardTitle>本地 RWKV</CardTitle>
              <CardDescription>
                Rosetta 托管的本地翻译模型运行时
              </CardDescription>
            </div>
            <div className="flex items-center gap-2">
              <Button
                disabled={
                  isCheckingRuntime || isPreparingRuntime || isPreparingInstall
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
                  isCheckingRuntime || isPreparingRuntime || isPreparingInstall
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
                  isCheckingRuntime || isPreparingRuntime || isPreparingInstall
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
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>RWKV API</CardTitle>
          <CardDescription>配置翻译连接和默认调度模式</CardDescription>
        </CardHeader>

        <CardContent>
          <div className="flex flex-col gap-4">
            <div className="flex flex-col gap-2">
              <Label htmlFor="rwkv-base-url">API 地址</Label>
              <Input
                id="rwkv-base-url"
                onChange={updateTextField("baseUrl")}
                value={rwkv.baseUrl}
              />
            </div>

            <div className="flex flex-col gap-2">
              <Label>Batch 端点</Label>
              <Select
                onValueChange={(value) =>
                  updateRwkvConfig({
                    batchEndpoint: value as RwkvConnectionConfig["batchEndpoint"],
                  })
                }
                value={rwkv.batchEndpoint}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value="/translate/v1/batch-translate">
                      /translate/v1/batch-translate
                    </SelectItem>
                    <SelectItem value="/big_batch/completions">
                      /big_batch/completions
                    </SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
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
