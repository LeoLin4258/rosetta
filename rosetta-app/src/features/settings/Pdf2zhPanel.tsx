import { useEffect, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  Download,
  FolderInput,
  LoaderCircle,
  RefreshCw,
  X,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
    <section
      className={cn("flex flex-col gap-4", className)}
      id="pdf2zh"
    >
      <div className="flex items-start justify-between gap-4">
        <StatusRow
          state={state}
          status={rt.status}
          isRefreshing={rt.isRefreshing}
          isInstallActive={isInstallActive}
        />
        <Pdf2zhBadge state={state} isInstallActive={isInstallActive} className="shrink-0" />
      </div>

      <div className="flex flex-col gap-4">
        {isInstallActive && (
          <InstallProgressRow
            percent={installPercent(rt.progress)}
            message={rt.progress?.message ?? ""}
            speedBytesPerSec={rt.progress?.speedBytesPerSec ?? 0}
          />
        )}

        <div className="flex flex-wrap items-center gap-2">
          {!isUnsupported && (
            <RepairActions
              state={state}
              isInstallActive={isInstallActive}
              isInstalling={rt.isInstalling}
              onCancelInstall={() => void rt.cancelInstall()}
              onInstall={() => void rt.install({ repair: false })}
              onRepair={() => void rt.install({ repair: true })}
              onImportFromFile={() => void rt.importFromFile()}
            />
          )}

          <Button
            size="sm"
            variant="ghost"
            onClick={() => void rt.refreshStatus()}
            disabled={rt.isRefreshing || isInstallActive}
            className="text-muted-foreground"
          >
            {rt.isRefreshing ? (
              <LoaderCircle className="size-4 animate-spin" />
            ) : (
              <RefreshCw className="size-4" />
            )}
            重新检查
          </Button>
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
                className="flex h-8 w-fit items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <ChevronDown
                  className={cn(
                    "size-3.5 transition-transform",
                    detailsOpen && "rotate-180"
                  )}
                />
                技术信息
              </button>
            </CollapsibleTrigger>
            <CollapsibleContent>
              <div className="mt-2 flex flex-col gap-4 border-t pt-4">
                {rt.status && <Pdf2zhInfoRows status={rt.status} />}
              </div>
            </CollapsibleContent>
          </Collapsible>
        )}
      </div>
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
  const { dot, label, sub, spinning } = resolveStatus(state, status, isRefreshing, isInstallActive);
  return (
    <div className="flex min-w-0 flex-1 items-start gap-2.5">
      {spinning ? (
        <LoaderCircle className="mt-0.5 size-3.5 shrink-0 animate-spin text-muted-foreground" />
      ) : (
        <div className={cn("mt-1.5 size-2 shrink-0 rounded-full", dot)} />
      )}
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium leading-5">{label}</p>
        {sub && (
          <p className="mt-0.5 max-w-[72ch] text-xs leading-5 text-muted-foreground">
            {sub}
          </p>
        )}
      </div>
    </div>
  );
}

function resolveStatus(
  state: Pdf2zhStatus["state"] | null,
  status: Pdf2zhStatus | null,
  isRefreshing: boolean,
  isInstallActive: boolean
): { dot: string; label: string; sub?: string; spinning?: boolean } {
  if (isInstallActive) {
    return {
      dot: "bg-blue-500",
      label: "正在安装 PDF 组件",
      sub: "安装完成后即可翻译 PDF。",
      spinning: true,
    };
  }
  if (isRefreshing || state === null) {
    return {
      dot: "bg-muted-foreground/30",
      label: "正在检查 PDF 组件",
      spinning: true,
    };
  }
  if (state === "installed") {
    return {
      dot: "bg-emerald-500",
      label: "PDF 组件已就绪",
    };
  }
  if (state === "unsupported") {
    return {
      dot: "bg-muted-foreground/40",
      label: "当前设备暂不支持 PDF 组件",
      sub: status?.message,
    };
  }
  if (status?.message.includes("需要更新")) {
    return {
      dot: "bg-amber-500",
      label: "PDF 组件需要更新",
      sub: "重新安装后即可使用新版内置版面模型。",
    };
  }
  return {
    dot: "bg-muted-foreground/30",
    label: "PDF 组件尚未安装",
    sub: status?.message,
  };
}

function RepairActions({
  state,
  isInstallActive,
  isInstalling,
  onCancelInstall,
  onInstall,
  onRepair,
  onImportFromFile,
}: {
  state: Pdf2zhStatus["state"] | null;
  isInstallActive: boolean;
  isInstalling: boolean;
  onCancelInstall: () => void;
  onInstall: () => void;
  onRepair: () => void;
  /** Manual-import escape hatch — see `useManagedPdf2zhRuntime.importFromFile`
   *  for why this exists (mainland China users blocked on GitHub Releases). */
  onImportFromFile: () => void;
}) {
  if (isInstallActive) {
    return (
      <Button size="sm" variant="outline" onClick={onCancelInstall}>
        <X className="size-4" /> 取消安装
      </Button>
    );
  }
  if (state === "installed") {
    return (
      <div className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={onRepair}
          disabled={isInstalling}
          className="w-fit"
        >
          <RefreshCw className="size-4" /> 重新安装 PDF 组件
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={onImportFromFile}
          disabled={isInstalling}
          className="w-fit text-muted-foreground"
        >
          <FolderInput className="size-4" /> 从本地文件导入
        </Button>
      </div>
    );
  }
  return (
    <div className="flex flex-wrap gap-2">
      <Button
        size="sm"
        onClick={onInstall}
        disabled={isInstalling}
        className="w-fit"
      >
        {isInstalling ? (
          <LoaderCircle className="size-4 animate-spin" />
        ) : (
          <Download className="size-4" />
        )}
        安装 PDF 组件
      </Button>
      <Button
        size="sm"
        variant="outline"
        onClick={onImportFromFile}
        disabled={isInstalling}
        className="w-fit"
      >
        <FolderInput className="size-4" /> 从本地安装包导入
      </Button>
    </div>
  );
}

function Pdf2zhBadge({
  state,
  isInstallActive,
  className,
}: {
  state: Pdf2zhStatus["state"] | null;
  isInstallActive: boolean;
  className?: string;
}) {
  if (isInstallActive) {
    return (
      <Badge
        variant="outline"
        className={cn(
          "gap-1 border-transparent bg-amber-500/15 text-amber-800 dark:text-amber-300",
          className
        )}
      >
        <LoaderCircle className="size-3 animate-spin" /> 安装中
      </Badge>
    );
  }
  if (state === "installed") {
    return (
      <Badge
        variant="outline"
        className={cn(
          "gap-1 border-transparent bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
          className
        )}
      >
        <CheckCircle2 className="size-3" /> 已就绪
      </Badge>
    );
  }
  if (state === "not-installed") {
    return (
      <Badge
        variant="outline"
        className={cn(
          "border-transparent bg-sky-500/15 text-sky-700 dark:text-sky-300",
          className
        )}
      >
        未安装
      </Badge>
    );
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
          组件下载代理（可选）
        </Label>
        <span className="text-[11px] text-muted-foreground">只影响 PDF 组件下载</span>
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
