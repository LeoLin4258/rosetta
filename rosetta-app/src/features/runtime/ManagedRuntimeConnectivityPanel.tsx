import { useState } from "react";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  ChevronDown,
  Copy,
  LoaderCircle,
  Network,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { ManagedRuntimeConnectivityDiagnostics } from "@/types/rosetta";

type ManagedRuntimeConnectivityPanelProps = {
  diagnostics: ManagedRuntimeConnectivityDiagnostics | null;
  isLoading: boolean;
  isRepairing?: boolean;
  repairMessage?: string | null;
  onDiagnose: () => void;
  onRepair?: () => void;
  className?: string;
};

export function ManagedRuntimeConnectivityPanel({
  diagnostics,
  isLoading,
  isRepairing = false,
  repairMessage,
  onDiagnose,
  onRepair,
  className,
}: ManagedRuntimeConnectivityPanelProps) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">(
    "idle"
  );
  const failed = diagnostics && !diagnostics.targetLoopbackOk;
  const needsAttention = !diagnostics || failed;
  const networkSummary = formatNetworkSummary(diagnostics);
  const canRepair = !!failed && !!onRepair;
  const isBusy = isLoading || isRepairing;

  async function copyPowerShellHint() {
    if (!diagnostics?.powershellHint) return;
    try {
      await navigator.clipboard.writeText(diagnostics.powershellHint);
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 1600);
    } catch {
      setCopyState("failed");
      window.setTimeout(() => setCopyState("idle"), 2200);
    }
  }

  return (
    <div
      className={cn(
        "w-full rounded-lg border bg-muted/20 p-3 text-sm",
        needsAttention
          ? "border-amber-500/35 bg-amber-500/8"
          : "border-border bg-muted/20",
        className
      )}
    >
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex min-w-0 items-start gap-2">
          <div
            className={cn(
              "mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-md",
              needsAttention
                ? "bg-amber-500/12 text-amber-700 dark:text-amber-300"
                : "bg-emerald-500/12 text-emerald-700 dark:text-emerald-300"
            )}
          >
            {needsAttention ? (
              <AlertTriangle className="size-3.5" />
            ) : (
              <Network className="size-3.5" />
            )}
          </div>
          <div className="min-w-0">
            <p className="font-medium">
              {diagnostics
                ? failed
                  ? "Windows 拦住了本机连接"
                  : "本机连接检查通过"
                : "检查本机连接"}
            </p>
            <p className="mt-1 text-xs leading-5 text-muted-foreground">
              {summaryText(diagnostics)}
            </p>
            {repairMessage ? (
              <p className="mt-2 text-xs leading-5 text-foreground">
                {repairMessage}
              </p>
            ) : null}
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap gap-2 sm:justify-end">
          {canRepair ? (
            <Button
              type="button"
              size="sm"
              onClick={onRepair}
              disabled={isBusy}
              className="gap-1.5"
            >
              {isRepairing ? (
                <LoaderCircle className="size-3.5 animate-spin" />
              ) : (
                <ShieldCheck className="size-3.5" />
              )}
              修复连接并重试
            </Button>
          ) : null}
          <Button
            type="button"
            size="sm"
            variant={canRepair ? "ghost" : "outline"}
            onClick={onDiagnose}
            disabled={isBusy}
            className="gap-1.5"
          >
            {isLoading ? (
              <LoaderCircle className="size-3.5 animate-spin" />
            ) : (
              <RefreshCw className="size-3.5" />
            )}
            {diagnostics ? "重新检查" : "检查"}
          </Button>
        </div>
      </div>

      {diagnostics ? (
        <details className="group mt-3">
          <summary className="flex w-fit cursor-pointer list-none items-center gap-1.5 rounded-md px-1 text-xs text-muted-foreground transition-colors hover:text-foreground">
            <ChevronDown className="size-3.5 transition-transform group-open:rotate-180" />
            技术信息
          </summary>
          <div className="mt-3 space-y-3">
            <div className="flex flex-wrap gap-1.5">
              <StatusPill
                label="127.0.0.1"
                ok={diagnostics.loopbackIpv4Ok}
              />
              <StatusPill
                label="::1"
                ok={diagnostics.loopbackIpv6Ok ?? false}
                muted={diagnostics.loopbackIpv6Ok == null}
              />
              {networkSummary ? <InfoPill label={networkSummary} /> : null}
            </div>

            {diagnostics.powershellHint ? (
              <div className="flex flex-col gap-2 rounded-md border border-border/70 bg-background/70 p-2">
                <p className="text-xs leading-5 text-muted-foreground">
                  手动修复命令
                </p>
                <div className="flex items-center gap-2">
                  <code className="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1.5 font-mono text-[11px] text-foreground">
                    {diagnostics.powershellHint}
                  </code>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={copyPowerShellHint}
                    className="shrink-0 gap-1.5"
                  >
                    {copyState === "copied" ? (
                      <Check className="size-3.5" />
                    ) : (
                      <Copy className="size-3.5" />
                    )}
                    {copyState === "copied"
                      ? "已复制"
                      : copyState === "failed"
                        ? "失败"
                        : "复制"}
                  </Button>
                </div>
              </div>
            ) : null}

            {diagnostics.recommendedActions.length > 0 ? (
              <ul className="space-y-1.5 text-xs leading-5 text-muted-foreground">
                {diagnostics.recommendedActions.slice(0, 3).map((action) => (
                  <li key={action} className="flex gap-2">
                    <span className="mt-2 size-1 shrink-0 rounded-full bg-muted-foreground/45" />
                    <span>{action}</span>
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        </details>
      ) : null}
    </div>
  );
}

function summaryText(
  diagnostics: ManagedRuntimeConnectivityDiagnostics | null
): string {
  if (!diagnostics) {
    return "Rosetta 会检查 Windows 是否允许连接本机翻译服务。";
  }
  if (diagnostics.targetLoopbackOk) {
    return "Windows 本机连接正常。现在可以重新启动本地翻译引擎。";
  }
  if (hasPublicNetwork(diagnostics)) {
    return "Rosetta 已启动本地翻译服务，但 Windows 无法连接它。通常把当前网络设为专用网络即可恢复。";
  }
  return "Rosetta 已启动本地翻译服务，但 Windows 无法连接它。请先点击修复，若仍失败再检查安全软件或网络防护。";
}

function hasPublicNetwork(
  diagnostics: ManagedRuntimeConnectivityDiagnostics
): boolean {
  return diagnostics.networkProfiles.some(
    (profile) => profile.networkCategory === "Public"
  );
}

function StatusPill({
  label,
  ok,
  muted = false,
}: {
  label: string;
  ok: boolean;
  muted?: boolean;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] leading-none",
        muted
          ? "bg-muted text-muted-foreground"
          : ok
            ? "bg-emerald-500/12 text-emerald-700 dark:text-emerald-300"
            : "bg-amber-500/12 text-amber-800 dark:text-amber-300"
      )}
    >
      {muted ? null : ok ? (
        <CheckCircle2 className="size-3" />
      ) : (
        <AlertTriangle className="size-3" />
      )}
      {muted ? `${label} 未检测` : `${label} ${ok ? "可连接" : "失败"}`}
    </span>
  );
}

function InfoPill({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center rounded-md bg-muted px-2 py-1 text-[11px] leading-none text-muted-foreground">
      {label}
    </span>
  );
}

function formatNetworkSummary(
  diagnostics: ManagedRuntimeConnectivityDiagnostics | null
): string | null {
  if (!diagnostics || diagnostics.networkProfiles.length === 0) {
    return null;
  }
  const labels = diagnostics.networkProfiles
    .map((profile) => {
      const alias = profile.interfaceAlias ?? profile.name ?? "网络";
      const category = profile.networkCategory ?? "Unknown";
      return `${alias}: ${category}`;
    })
    .slice(0, 2);
  return labels.join(" · ");
}
