import type { ChangeEvent } from "react";
import { useRosettaStore } from "../../store/useRosettaStore";
import type {
  AppThemeMode,
  RwkvConnectionConfig,
  TranslationMode,
} from "../../types/rosetta";
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

export function SettingsPage() {
  const themeMode = useRosettaStore((state) => state.themeMode);
  const setThemeMode = useRosettaStore((state) => state.setThemeMode);
  const rwkv = useRosettaStore((state) => state.rwkv);
  const updateRwkvConfig = useRosettaStore((state) => state.updateRwkvConfig);
  const setTranslationMode = useRosettaStore((state) => state.setTranslationMode);

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
          <CardTitle>RWKV 连接</CardTitle>
          <CardDescription>配置本地翻译 API 和默认调度模式</CardDescription>
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
