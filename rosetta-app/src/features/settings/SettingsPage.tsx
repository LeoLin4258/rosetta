import { useState, type ChangeEvent } from "react";
import {
  CheckCircle2,
  Cloud,
  Palette,
  Send,
  ServerOff,
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { probeRwkvTranslationApi } from "../../lib/rwkvApi";
import { cn } from "../../lib/utils";
import { useRosettaStore } from "../../store/useRosettaStore";
import type {
  AppThemeMode,
  RwkvConnectionConfig,
  RwkvTranslationApiProbeResult,
  TranslationMode,
} from "../../types/rosetta";

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

export function SettingsPage() {
  const themeMode = useRosettaStore((state) => state.themeMode);
  const setThemeMode = useRosettaStore((state) => state.setThemeMode);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const updateRwkvConfig = useRosettaStore((state) => state.updateRwkvConfig);
  const setTranslationMode = useRosettaStore((state) => state.setTranslationMode);
  const [apiProbeResult, setApiProbeResult] =
    useState<RwkvTranslationApiProbeResult | null>(null);
  const [apiError, setApiError] = useState<string | null>(null);
  const [isProbingApi, setIsProbingApi] = useState(false);

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
    <section className="mx-auto flex w-full max-w-4xl flex-col gap-6 px-6 py-6">
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

        <section className="flex flex-col gap-3" id="translation-service">
          <SettingsSectionHeader
            description="配置当前用于文档翻译的 RWKV API。"
            icon={<Cloud />}
            title="翻译服务"
          >
            <StatusBadge status={apiStatus} />
          </SettingsSectionHeader>

          <Card className="overflow-hidden">
            <CardContent className="flex flex-col gap-5 py-5">
              <div className="grid gap-4 md:grid-cols-2">
                <SettingField
                  description="例如 https://example.com"
                  htmlFor="rwkv-base-url"
                  label="API 地址"
                >
                  <Input
                    id="rwkv-base-url"
                    onChange={updateTextField("baseUrl")}
                    placeholder="https://..."
                    value={rwkv.baseUrl}
                  />
                </SettingField>

                <SettingField
                  description="当前接口使用 /v1/chat/completions"
                  htmlFor="rwkv-endpoint"
                  label="接口路径"
                >
                  <Input
                    id="rwkv-endpoint"
                    onChange={updateTextField("endpoint")}
                    placeholder="/v1/chat/completions"
                    value={rwkv.endpoint}
                  />
                </SettingField>

                <SettingField
                  description="X-Internal-Token"
                  htmlFor="rwkv-internal-token"
                  label="访问密钥"
                >
                  <Input
                    autoComplete="off"
                    id="rwkv-internal-token"
                    onChange={updateTextField("internalToken")}
                    type="password"
                    value={rwkv.internalToken}
                  />
                </SettingField>

                <SettingField
                  description="body password"
                  htmlFor="rwkv-body-password"
                  label="模型口令"
                >
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
                  label="超时时间"
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
                      if (value) {
                        setTranslationMode(value as TranslationMode);
                      }
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
                    <span className="text-muted-foreground">
                      会发送两句英文样本文本到当前接口。
                    </span>
                  </div>
                  <Button
                    disabled={!canProbeApi}
                    onClick={() => void probeApi()}
                    title="测试 RWKV API"
                    type="button"
                    variant={apiStatus === "connected" ? "outline" : "default"}
                  >
                    <Send data-icon="inline-start" />
                    {isProbingApi ? "测试中" : "测试连接"}
                  </Button>
                </div>

                {missingConnectionFields.length > 0 ? (
                  <p className="text-xs text-muted-foreground">
                    还需要填写：{missingConnectionFields.join("、")}。
                  </p>
                ) : null}

                {apiError ? (
                  <p className="text-sm text-destructive">{apiError}</p>
                ) : null}

                {apiProbeResult ? (
                  <ApiProbeResult result={apiProbeResult} />
                ) : null}
              </div>
            </CardContent>
          </Card>
        </section>

        <section className="flex flex-col gap-3" id="local-model">
          <SettingsSectionHeader
            description="本地一键运行 RWKV 会作为独立选项恢复。"
            icon={<ServerOff />}
            title="本地模型"
          />

          <Card>
            <CardHeader>
              <div className="flex items-start justify-between gap-3">
                <div>
                  <CardTitle>一键本地 RWKV</CardTitle>
                  <CardDescription>
                    当前版本优先使用已配置的翻译服务。
                  </CardDescription>
                </div>
                <Badge variant="outline">即将支持</Badge>
              </div>
            </CardHeader>
            <CardContent className="text-sm text-muted-foreground">
              后续恢复本地模型时，这里会提供清晰的安装、检测和启动入口；它不会和当前 API 配置混在一起。
            </CardContent>
          </Card>
        </section>
      </main>
    </section>
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
