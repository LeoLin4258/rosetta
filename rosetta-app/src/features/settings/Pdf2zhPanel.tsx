import { useEffect, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  Download,
  FileText,
  LoaderCircle,
  RefreshCw,
  X,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  useManagedPdf2zhRuntime,
  type UseManagedPdf2zhRuntime,
} from "@/lib/useManagedPdf2zhRuntime";
import type { Pdf2zhInstallPhase, Pdf2zhStatus } from "@/lib/pdf2zhRuntime";
import { cn } from "@/lib/utils";
import { useRosettaStore } from "@/store/useRosettaStore";

const INSTALL_ACTIVE_PHASES: ReadonlySet<Pdf2zhInstallPhase> = new Set([
  "preflight",
  "downloading",
  "verifying",
  "extracting",
]);

export function Pdf2zhPanel({ className }: { className?: string }) {
  const rt = useManagedPdf2zhRuntime();
  const [detailsOpen, setDetailsOpen] = useState(false);

  useEffect(() => {
    void rt.refreshStatus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const state = rt.status?.state ?? null;
  const installPhase = rt.progress?.phase ?? null;
  const isInstallActive = !!installPhase && INSTALL_ACTIVE_PHASES.has(installPhase);
  const isUnsupported = state === "unsupported";

  return (
    <section className={cn("flex flex-col gap-3", className)} id="pdf2zh">
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
            <FileText className="size-4" />
          </div>
          <div className="min-w-0">
            <h2 className="text-lg font-semibold tracking-normal">PDF 版面处理</h2>
            <p className="mt-1 text-sm text-muted-foreground">
              用于读取 PDF、保留原文排版，并生成可预览和导出的译文 PDF。
            </p>
          </div>
        </div>
        <Pdf2zhBadge state={state} isInstallActive={isInstallActive} />
      </div>

      <Card>
        <CardContent className="flex flex-col gap-4 py-5">
          <StatusRow
            state={state}
            status={rt.status}
            isRefreshing={rt.isRefreshing}
            isInstallActive={isInstallActive}
          />

          {isInstallActive && (
            <InstallProgressRow
              percent={installPercent(rt.progress)}
              message={rt.progress?.message ?? ""}
              speedBytesPerSec={rt.progress?.speedBytesPerSec ?? 0}
            />
          )}

          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() => void rt.refreshStatus()}
              disabled={rt.isRefreshing || isInstallActive}
            >
              {rt.isRefreshing ? (
                <LoaderCircle className="size-4 animate-spin" />
              ) : (
                <RefreshCw className="size-4" />
              )}
              刷新状态
            </Button>

            {isInstallActive ? (
              <Button size="sm" variant="outline" onClick={() => void rt.cancelInstall()}>
                <X className="size-4" /> 取消安装
              </Button>
            ) : null}
          </div>

          {showProxyInput(state, isInstallActive) && (
            <DownloadProxyField disabled={isInstallActive} />
          )}

          {rt.lastError && (
            <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
              <AlertCircle className="mt-0.5 size-4 shrink-0" />
              <span className="break-all">{rt.lastError}</span>
            </div>
          )}

          {!isUnsupported && (
            <Collapsible open={detailsOpen} onOpenChange={setDetailsOpen}>
              <CollapsibleTrigger asChild>
                <button
                  type="button"
                  className="-ml-1 flex items-center gap-1 rounded px-1 py-0.5 text-xs text-muted-foreground/60 transition-colors hover:text-muted-foreground"
                >
                  <ChevronDown
                    className={cn(
                      "size-3.5 transition-transform",
                      detailsOpen && "rotate-180"
                    )}
                  />
                  详细信息
                </button>
              </CollapsibleTrigger>
              <CollapsibleContent>
                <div className="mt-3 flex flex-col gap-4 rounded-md border bg-muted/20 p-4">
                  <RepairActions
                    state={state}
                    isInstallActive={isInstallActive}
                    isInstalling={rt.isInstalling}
                    onInstall={() => void rt.install({ repair: false })}
                    onRepair={() => void rt.install({ repair: true })}
                  />
                  {rt.status && <Pdf2zhInfoRows status={rt.status} />}
                </div>
              </CollapsibleContent>
            </Collapsible>
          )}
        </CardContent>
      </Card>
    </section>
  );
}

function StatusRow({
  state,
  status,
  isRefreshing,
  isInstallActive,
}: {
  state: Pdf2zhStatus["state"] | null;
  status: Pdf2zhStatus | null;
  isRefreshing: boolean;
  isInstallActive: boolean;
}) {
  const { dot, label, sub } = resolveStatus(state, status, isRefreshing, isInstallActive);
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2.5">
        <div className={cn("size-2 shrink-0 rounded-full", dot)} />
        <span className="text-sm font-medium">{label}</span>
      </div>
      {sub && <p className="pl-4 text-xs text-muted-foreground">{sub}</p>}
    </div>
  );
}

function resolveStatus(
  state: Pdf2zhStatus["state"] | null,
  status: Pdf2zhStatus | null,
  isRefreshing: boolean,
  isInstallActive: boolean
): { dot: string; label: string; sub?: string } {
  if (isInstallActive) {
    return {
      dot: "bg-blue-500 animate-pulse",
      label: "正在准备 PDF 版面处理组件…",
      sub: "完成后，PDF 文档会自动保留排版并生成译文 PDF。",
    };
  }
  if (isRefreshing || state === null) {
    return {
      dot: "bg-muted-foreground/30 animate-pulse",
      label: "正在检测 PDF 版面处理组件…",
    };
  }
  if (state === "installed") {
    return {
      dot: "bg-emerald-500",
      label: "PDF 版面处理已就绪",
      sub: status?.paths?.bin ? `使用 ${status.paths.bin}` : undefined,
    };
  }
  if (state === "unsupported") {
    return {
      dot: "bg-muted-foreground/40",
      label: "当前设备暂不支持自动处理 PDF 版面",
      sub: status?.message,
    };
  }
  return {
    dot: "bg-muted-foreground/30",
    label: "PDF 版面处理组件尚未安装",
    sub: status?.message,
  };
}

function RepairActions({
  state,
  isInstallActive,
  isInstalling,
  onInstall,
  onRepair,
}: {
  state: Pdf2zhStatus["state"] | null;
  isInstallActive: boolean;
  isInstalling: boolean;
  onInstall: () => void;
  onRepair: () => void;
}) {
  if (isInstallActive) return null;
  if (state === "installed") {
    return (
      <Button
        variant="outline"
        size="sm"
        onClick={onRepair}
        disabled={isInstalling}
        className="w-fit"
      >
        <RefreshCw className="size-4" /> 重新安装 / 修复
      </Button>
    );
  }
  return (
    <Button
      size="sm"
      variant="outline"
      onClick={onInstall}
      disabled={isInstalling}
      className="w-fit"
    >
      {isInstalling ? (
        <LoaderCircle className="size-4 animate-spin" />
      ) : (
        <Download className="size-4" />
      )}
      安装 PDF 版面处理组件
    </Button>
  );
}

function Pdf2zhBadge({
  state,
  isInstallActive,
}: {
  state: Pdf2zhStatus["state"] | null;
  isInstallActive: boolean;
}) {
  if (isInstallActive) {
    return (
      <Badge variant="secondary" className="gap-1">
        <LoaderCircle className="size-3 animate-spin" /> 安装中
      </Badge>
    );
  }
  if (state === "installed") {
    return (
      <Badge variant="secondary" className="gap-1 text-emerald-600 dark:text-emerald-400">
        <CheckCircle2 className="size-3" /> 已就绪
      </Badge>
    );
  }
  if (state === "not-installed") {
    return <Badge variant="outline">未安装</Badge>;
  }
  return null;
}

function showProxyInput(
  state: Pdf2zhStatus["state"] | null,
  isInstallActive: boolean
): boolean {
  if (isInstallActive) return true;
  return state === "not-installed";
}

function DownloadProxyField({ disabled }: { disabled: boolean }) {
  const proxyUrl = useRosettaStore((s) => s.downloadProxy.url);
  const setProxyUrl = useRosettaStore((s) => s.setDownloadProxyUrl);

  return (
    <div className="flex flex-col gap-1.5 rounded-md border bg-muted/30 p-3">
      <div className="flex items-baseline justify-between gap-3">
        <Label htmlFor="pdf2zh-download-proxy" className="text-xs font-medium">
          下载代理（可选）
        </Label>
        <span className="text-[11px] text-muted-foreground">仅用于下载组件</span>
      </div>
      <Input
        id="pdf2zh-download-proxy"
        type="text"
        placeholder="例如 http://127.0.0.1:7897 或留空"
        value={proxyUrl}
        disabled={disabled}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => setProxyUrl(event.target.value)}
        className="h-8 font-mono text-xs"
      />
    </div>
  );
}

function Pdf2zhInfoRows({ status }: { status: Pdf2zhStatus }) {
  const rows: Array<{ label: string; value: string }> = [];
  if (status.profile) {
    rows.push(
      { label: "平台", value: `${status.profile.platformOs}/${status.profile.platformArch}` },
      { label: "组件版本", value: status.profile.packDirectoryName }
    );
  }
  if (status.paths) {
    if (status.paths.bin) rows.push({ label: "程序路径", value: status.paths.bin });
    rows.push(
      { label: "组件路径", value: status.paths.packDir },
      { label: "日志路径", value: status.paths.logsDir }
    );
  }
  if (status.installPlan) {
    rows.push({ label: "状态说明", value: status.installPlan.message });
  }
  if (rows.length === 0) return null;

  return (
    <dl className="grid gap-1.5 text-xs">
      {rows.map((row) => (
        <div key={row.label} className="grid grid-cols-[6rem_1fr] gap-3">
          <dt className="text-muted-foreground">{row.label}</dt>
          <dd className="truncate font-mono text-[11px] text-foreground/70">{row.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function InstallProgressRow({
  percent,
  message,
  speedBytesPerSec,
}: {
  percent: number;
  message: string;
  speedBytesPerSec: number;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span className="truncate">{message}</span>
        <span className="shrink-0 tabular-nums">
          {percent}%{speedBytesPerSec > 0 ? ` · ${formatSpeed(speedBytesPerSec)}` : ""}
        </span>
      </div>
      <div className="relative h-1.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          className="absolute inset-y-0 left-0 rounded-full bg-primary transition-[width] duration-200"
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

function installPercent(progress: UseManagedPdf2zhRuntime["progress"]): number {
  if (!progress) return 0;
  if (progress.phase === "done") return 100;
  if (progress.bytesTotal === 0) return 0;
  return Math.min(100, Math.floor((progress.bytesDone * 100) / progress.bytesTotal));
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let i = 1; i < units.length && value >= 1024; i++) {
    value /= 1024;
    unit = units[i];
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return "-";
  return `${formatBytes(bytesPerSec)}/s`;
}
