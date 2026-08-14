import { useState } from "react";
import {
  AlertCircle,
  ChevronDown,
  Download,
  FolderInput,
  LoaderCircle,
  RefreshCw,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import type { PdfMarkdownComponentStatus } from "@/lib/rosettaJobs";
import { cn } from "@/lib/utils";
import { usePdfMarkdownRuntime } from "@/lib/usePdfMarkdownRuntime";

export function PdfMarkdownPanel({ className }: { className?: string }) {
  const runtime = usePdfMarkdownRuntime(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const state = runtime.componentStatus?.state ?? null;
  const isInstallActive =
    runtime.isInstalling || runtime.installProgress?.state === "installing";

  function runInstall(repair: boolean) {
    void runtime.install(repair).catch(() => {});
  }

  function importFromFile() {
    void runtime.importFromFile().catch(() => {});
  }

  return (
    <section className={cn("flex flex-col gap-4", className)} id="pdf-markdown">
      <StatusRow
        isInstallActive={isInstallActive}
        isRefreshing={runtime.isRefreshingComponent}
        state={state}
        status={runtime.componentStatus}
      />

      {isInstallActive ? (
        <InstallProgress progress={runtime.installProgress} />
      ) : null}

      <div className="flex flex-wrap items-center gap-2">
        {state !== "unsupported" ? (
          isInstallActive ? (
            <Button
              onClick={() => void runtime.cancelInstall()}
              size="sm"
              type="button"
              variant="outline"
            >
              <X data-icon="inline-start" />
              取消安装
            </Button>
          ) : state === "installed" ? (
            <>
              <Button
                onClick={() => runInstall(true)}
                size="sm"
                type="button"
                variant="outline"
              >
                <RefreshCw data-icon="inline-start" />
                重新安装
              </Button>
              <Button
                className="text-muted-foreground"
                onClick={importFromFile}
                size="sm"
                type="button"
                variant="ghost"
              >
                <FolderInput data-icon="inline-start" />
                导入文件
              </Button>
            </>
          ) : (
            <>
              <Button
                onClick={() => runInstall(state === "needs-repair")}
                size="sm"
                type="button"
              >
                {state === "needs-repair" ? (
                  <RefreshCw data-icon="inline-start" />
                ) : (
                  <Download data-icon="inline-start" />
                )}
                {state === "needs-repair" ? "修复组件" : "安装组件"}
              </Button>
              <Button
                onClick={importFromFile}
                size="sm"
                type="button"
                variant="outline"
              >
                <FolderInput data-icon="inline-start" />
                导入安装包
              </Button>
            </>
          )
        ) : null}

        <Button
          className="text-muted-foreground"
          disabled={runtime.isRefreshingComponent || isInstallActive}
          onClick={() => void runtime.refreshComponentStatus()}
          size="sm"
          type="button"
          variant="ghost"
        >
          {runtime.isRefreshingComponent ? (
            <LoaderCircle className="animate-spin" data-icon="inline-start" />
          ) : (
            <RefreshCw data-icon="inline-start" />
          )}
          检查
        </Button>
      </div>

      {runtime.lastError ? (
        <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
          <AlertCircle className="mt-0.5 size-4 shrink-0" />
          <span className="break-all">{runtime.lastError}</span>
        </div>
      ) : null}

      {runtime.componentStatus?.profile ? (
        <Collapsible onOpenChange={setDetailsOpen} open={detailsOpen}>
          <CollapsibleTrigger asChild>
            <button
              className="flex h-8 w-fit items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              type="button"
            >
              <ChevronDown
                className={cn(
                  "size-3.5 transition-transform duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]",
                  detailsOpen && "rotate-180",
                )}
              />
              详情
            </button>
          </CollapsibleTrigger>
          <CollapsibleContent className="rosetta-settings-collapsible-content">
            <ComponentDetails status={runtime.componentStatus} />
          </CollapsibleContent>
        </Collapsible>
      ) : null}
    </section>
  );
}

function StatusRow({
  isInstallActive,
  isRefreshing,
  state,
  status,
}: {
  isInstallActive: boolean;
  isRefreshing: boolean;
  state: PdfMarkdownComponentStatus["state"] | null;
  status: PdfMarkdownComponentStatus | null;
}) {
  const resolved = resolveStatus(state, status, isRefreshing, isInstallActive);
  return (
    <div className="flex min-w-0 items-start gap-2.5">
      {resolved.spinning ? (
        <LoaderCircle className="mt-0.5 size-3.5 shrink-0 animate-spin text-muted-foreground" />
      ) : (
        <span className={cn("mt-1.5 size-2 shrink-0 rounded-full", resolved.dot)} />
      )}
      <div className="min-w-0 flex-1">
        <p className="text-[0.95rem] font-semibold leading-5">{resolved.label}</p>
        {resolved.detail ? (
          <p className="mt-0.5 max-w-[72ch] text-[0.85rem] leading-5 text-muted-foreground">
            {resolved.detail}
          </p>
        ) : null}
      </div>
    </div>
  );
}

function resolveStatus(
  state: PdfMarkdownComponentStatus["state"] | null,
  status: PdfMarkdownComponentStatus | null,
  isRefreshing: boolean,
  isInstallActive: boolean,
) {
  if (isInstallActive) {
    return { dot: "bg-primary", label: "安装中", spinning: true };
  }
  if (isRefreshing || state === null) {
    return { dot: "bg-muted-foreground/30", label: "检查中", spinning: true };
  }
  if (state === "installed") {
    return {
      dot: "bg-emerald-500",
      label: "已就绪",
      detail: "用于把文字型 PDF 提取为可翻译的结构化 Markdown。",
    };
  }
  if (state === "needs-repair") {
    return { dot: "bg-amber-500", label: "需要修复", detail: status?.message };
  }
  if (state === "unsupported") {
    return { dot: "bg-muted-foreground/40", label: "不支持", detail: status?.message };
  }
  return {
    dot: "bg-muted-foreground/30",
    label: "未安装",
    detail: "安装后可从 PDF 生成 Markdown，并保留标题、段落和图片引用。",
  };
}

function InstallProgress({
  progress,
}: {
  progress: ReturnType<typeof usePdfMarkdownRuntime>["installProgress"];
}) {
  const total = progress?.expectedBytes ?? 0;
  const done = progress?.downloadedBytes ?? 0;
  const percent = total > 0 ? Math.min(100, Math.floor((done * 100) / total)) : 0;
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>正在准备 PDF 转 Markdown 组件</span>
        <span className="shrink-0 tabular-nums">{percent}%</span>
      </div>
      <div className="relative h-1.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          className="absolute inset-y-0 left-0 rounded-full bg-primary transition-[width] duration-200 motion-reduce:transition-none"
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

function ComponentDetails({ status }: { status: PdfMarkdownComponentStatus }) {
  const profile = status.profile;
  if (!profile) return null;
  const rows = [
    { label: "平台", value: `${profile.platformOs}/${profile.platformArch}` },
    {
      label: "组件版本",
      value: `pymupdf4llm ${status.versions.pymupdf4llm}`,
    },
    { label: "PyMuPDF", value: status.versions.pymupdf },
    { label: "运行方式", value: status.cpuOnly ? "本机 CPU" : "本机" },
    { label: "安装包", value: formatBytes(profile.archiveBytes) },
    { label: "安装后", value: formatBytes(profile.unpackedBytes) },
  ];
  return (
    <dl className="mt-2 grid gap-1.5 border-t border-border/70 pt-4 text-xs">
      {rows.map((row) => (
        <div className="grid grid-cols-[6rem_1fr] gap-3" key={row.label}>
          <dt className="text-muted-foreground">{row.label}</dt>
          <dd className="truncate font-mono text-[11px] text-foreground/70">{row.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
}
