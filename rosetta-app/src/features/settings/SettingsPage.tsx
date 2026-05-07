import { useEffect, useState, type ChangeEvent } from "react";
import { RefreshCw } from "lucide-react";
import { getRwkvRuntimeStatus } from "../../lib/rwkvRuntime";
import { useRosettaStore } from "../../store/useRosettaStore";
import type {
  AppThemeMode,
  RwkvConnectionConfig,
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
  installed: "已安装",
};

export function SettingsPage() {
  const themeMode = useRosettaStore((state) => state.themeMode);
  const setThemeMode = useRosettaStore((state) => state.setThemeMode);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const updateRwkvConfig = useRosettaStore((state) => state.updateRwkvConfig);
  const setTranslationMode = useRosettaStore((state) => state.setTranslationMode);
  const [runtimeStatus, setRuntimeStatus] =
    useState<RwkvRuntimeStatus | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [isCheckingRuntime, setIsCheckingRuntime] = useState(false);

  async function refreshRuntimeStatus() {
    setIsCheckingRuntime(true);
    setRuntimeError(null);

    try {
      setRuntimeStatus(await getRwkvRuntimeStatus());
    } catch (error) {
      setRuntimeStatus(null);
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
            <Button
              disabled={isCheckingRuntime}
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
                <RuntimePath label="Log" value={runtimeStatus.logFile} />
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
