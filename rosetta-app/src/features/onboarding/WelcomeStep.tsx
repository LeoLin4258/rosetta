import { useState } from "react";
import { ArrowRight, Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useRosettaStore } from "@/store/useRosettaStore";

import { OnboardingStepShell } from "./OnboardingStepShell";

type WelcomeStepProps = {
  stepLabel: string;
  progressValue: number;
  title: string;
  description: string;
  primaryLabel: string;
  primaryCaption: string;
  onPrimary: () => void;
  onSkip: () => void;
  isPrimaryDisabled?: boolean;
  primaryIcon?: "download" | "arrow";
  skipLabel: string;
  showProxyConfig?: boolean;
};

export function WelcomeStep({
  stepLabel,
  progressValue,
  title,
  description,
  primaryLabel,
  primaryCaption,
  onPrimary,
  onSkip,
  isPrimaryDisabled = false,
  primaryIcon = "download",
  skipLabel,
  showProxyConfig = true,
}: WelcomeStepProps) {
  const proxyUrl = useRosettaStore((s) => s.downloadProxy.url);
  const setProxyUrl = useRosettaStore((s) => s.setDownloadProxyUrl);
  const [showProxyField, setShowProxyField] = useState(false);

  return (
    <OnboardingStepShell
      stepLabel={stepLabel}
      progressValue={progressValue}
      title={title}
      description={description}
      align="start"
    >
      <div className="space-y-5">
        <Button
          size="lg"
          disabled={isPrimaryDisabled}
          className="h-11 w-full gap-2"
          onClick={onPrimary}
        >
          {primaryIcon === "download" ? (
            <Download className="size-4" />
          ) : (
            <ArrowRight className="size-4" />
          )}
          {primaryLabel}
        </Button>
        <p className="text-xs leading-5 text-muted-foreground">
          {primaryCaption}
        </p>
      </div>

      {showProxyConfig && showProxyField && (
        <ProxyField
          value={proxyUrl}
          onChange={setProxyUrl}
          onHide={() => setShowProxyField(false)}
        />
      )}

      <div className="flex flex-col items-start gap-4 pt-1">
        {showProxyConfig && !showProxyField && (
          <button
            type="button"
            onClick={() => setShowProxyField(true)}
            className="text-xs text-muted-foreground/45 transition-colors hover:text-muted-foreground/70"
          >
            {proxyUrl ? "调整下载代理" : "下载慢时设置代理"}
          </button>
        )}
        <button
          type="button"
          onClick={onSkip}
          className="text-xs text-muted-foreground/35 transition-colors hover:text-muted-foreground/60"
        >
          {skipLabel}
        </button>
      </div>
    </OnboardingStepShell>
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
    <div className="flex w-full flex-col gap-2 rounded-lg border border-border/50 bg-muted/20 p-3">
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
        className="h-9 border-border/50 bg-background font-mono text-xs"
      />
    </div>
  );
}
