import { useState } from "react";
import { ArrowRight, Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useRosettaStore } from "@/store/useRosettaStore";

type WelcomeStepProps = {
  onBeginInstall: () => void;
  onSkipToExternal: () => void;
  isInstalling: boolean;
  /** Remaining bytes for runtime + model + PDF component. */
  downloadSizeBytes: number | null;
  /** From `OnboardingDecision.isReturningUser` — `true` when the user
   *  previously completed onboarding and is now seeing it again because
   *  their model went missing (almost always = they upgraded to a release
   *  that swapped the model). Used to show "欢迎回来" copy. */
  isReturningUser: boolean;
  localInstallSupported: boolean;
  supportMessage: string | null;
};

/**
 * Format a model size in bytes as a coarse "约 X" string for the Welcome
 * CTA subline. Buckets are deliberately rough so we don't show
 * "359.7 MB" — users care whether it's a Wi-Fi download or a "find
 * unlimited internet" download.
 */
function formatModelSize(bytes: number | null): string {
  if (bytes == null || bytes <= 0) return "下载量未知";
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) {
    const gb = mb / 1024;
    return `约 ${gb >= 10 ? gb.toFixed(0) : gb.toFixed(1)} GB`;
  }
  return `约 ${Math.round(mb)} MB`;
}

export function WelcomeStep({
  onBeginInstall,
  onSkipToExternal,
  isInstalling,
  downloadSizeBytes,
  isReturningUser,
  localInstallSupported,
  supportMessage,
}: WelcomeStepProps) {
  const proxyUrl = useRosettaStore((s) => s.downloadProxy.url);
  const setProxyUrl = useRosettaStore((s) => s.setDownloadProxyUrl);
  const [showProxyConfig, setShowProxyConfig] = useState(false);
  const [confirmingSkip, setConfirmingSkip] = useState(false);

  const sizeLabel = formatModelSize(downloadSizeBytes);

  return (
    <div className="flex h-full flex-col justify-between px-14 py-10">
      {/* Group 1: Icon + App name + Description.
        *
        * Returning users get a different headline + a one-line "why are you
        * seeing this again" explanation, instead of being treated like a
        * first-time visitor. Otherwise upgraders see the giant "Rosetta"
        * marketing splash and think we lost their previous setup.
        */}
      <div className="flex flex-col items-center gap-5 text-center mt-10">
        <div className="space-y-3">
          {isReturningUser ? (
            <>
              <h1 className="text-5xl font-bold tracking-tight">欢迎回来</h1>
              <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">
                新版本更换了更小更快的本地翻译模型。下载完成后即可恢复使用，旧模型已自动清理。
              </p>
            </>
          ) : (
            <>
              <h1 className="text-6xl font-bold tracking-tight">Rosetta</h1>
              <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">
                在本机翻译文档。文件不会上传，组件安装完成后可离线使用。
              </p>
            </>
          )}
        </div>
      </div>

      {/* Group 2: Download CTA + size estimate */}
      <div className="flex flex-col items-center gap-3">
        <Button
          size="lg"
          disabled={isInstalling}
          className="min-w-52 gap-2"
          onClick={
            localInstallSupported ? onBeginInstall : () => setConfirmingSkip(true)
          }
        >
          {localInstallSupported ? (
            <Download className="size-4" />
          ) : (
            <ArrowRight className="size-4" />
          )}
          {localInstallSupported
            ? isReturningUser
              ? "下载新模型"
              : "安装本地翻译引擎"
            : "继续使用外部翻译 API"}
        </Button>
        <p className="max-w-sm text-center text-xs text-muted-foreground/60">
          {localInstallSupported
            ? `${sizeLabel} · 包含翻译引擎、模型和 PDF 组件`
            : supportMessage ?? "当前设备不支持本地翻译引擎"}
        </p>
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
