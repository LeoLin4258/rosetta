import { useState } from "react";
import { Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useRosettaStore } from "@/store/useRosettaStore";

type WelcomeStepProps = {
  onBeginInstall: () => void;
  onSkipToExternal: () => void;
  isInstalling: boolean;
};

export function WelcomeStep({ onBeginInstall, onSkipToExternal, isInstalling }: WelcomeStepProps) {
  const proxyUrl = useRosettaStore((s) => s.downloadProxy.url);
  const setProxyUrl = useRosettaStore((s) => s.setDownloadProxyUrl);
  const [showProxyConfig, setShowProxyConfig] = useState(false);
  const [confirmingSkip, setConfirmingSkip] = useState(false);

  return (
    <div className="flex h-full flex-col justify-between px-14 py-10">
      {/* Group 1: Icon + App name + Description */}
      <div className="flex flex-col items-center gap-5 text-center mt-10">
        {/* <div className="flex size-16 items-center justify-center rounded-2xl bg-primary/10 text-primary p-2">
          <Sparkles className="size-8" strokeWidth={1.5} />
        </div> */}
        <div className="space-y-3">
          <h1 className="text-6xl font-bold tracking-tight">Rosetta</h1>
          <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">
            在本机翻译文档。文件不离开你的 Mac，不联网也能用。
          </p>
        </div>
      </div>

      {/* Group 2: Download CTA + size estimate */}
      <div className="flex flex-col items-center gap-3">
        <Button
          size="lg"
          onClick={onBeginInstall}
          disabled={isInstalling}
          className="min-w-52 gap-2"
        >
          <Download className="size-4" /> 安装本地翻译引擎
        </Button>
        <p className="text-xs text-muted-foreground/60">约 1.3 GB · 一次下载</p>
      </div>

      {/* Group 3: Network proxy + API option */}
      <div className="w-full">
        {showProxyConfig ? (
          <div className="space-y-2">
            <ProxyField
              value={proxyUrl}
              onChange={setProxyUrl}
              onHide={() => setShowProxyConfig(false)}
            />
            <button
              type="button"
              onClick={() => setConfirmingSkip(true)}
              className="block w-full text-center text-xs text-muted-foreground/35 transition-colors hover:text-muted-foreground/60"
            >
              使用自己的翻译 API →
            </button>
          </div>
        ) : confirmingSkip ? (
          <div className="flex items-center justify-between">
            <span className="text-xs text-muted-foreground/50">跳过后可在设置中配置 API</span>
            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={onSkipToExternal}
                className="text-xs text-primary/80 transition-colors hover:text-primary"
              >
                确认
              </button>
              <button
                type="button"
                onClick={() => setConfirmingSkip(false)}
                className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground/70"
              >
                取消
              </button>
            </div>
          </div>
        ) : (
          <div className="flex items-center justify-between">
            <button
              type="button"
              onClick={() => setShowProxyConfig(true)}
              className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground/70"
            >
              {proxyUrl ? `代理：${proxyUrl} · 调整` : "配置下载代理"}
            </button>
            <button
              type="button"
              onClick={() => setConfirmingSkip(true)}
              className="text-xs text-muted-foreground/35 transition-colors hover:text-muted-foreground/60"
            >
              使用自己的翻译 API →
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function ProxyField({
  value,
  onChange,
  onHide,
}: {
  value: string;
  onChange: (next: string) => void;
  onHide: () => void;
}) {
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-border/40 bg-muted/10 p-3.5">
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground/60">下载代理</span>
        <button
          type="button"
          onClick={onHide}
          className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground"
        >
          收起
        </button>
      </div>
      <Input
        id="onboarding-proxy"
        type="text"
        placeholder="http://127.0.0.1:7897（留空 = 自动检测）"
        value={value}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => onChange(event.target.value)}
        className="h-8 border-border/40 bg-transparent font-mono text-xs"
      />
    </div>
  );
}
