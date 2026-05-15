import { useState } from "react";
import { FileText, FolderOpen, Upload } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  importRosettaDocumentFromPath,
  importRosettaProjectFromDirectory,
  pickRosettaImportDirectory,
  pickRosettaImportPath,
} from "@/lib/rosettaJobs";
import { useRosettaStore } from "@/store/useRosettaStore";
import type { RosettaJobBundle } from "@/types/rosetta";
import { formatRelativeTime } from "@/lib/formatRelativeTime";

type WorkspaceEmptyProps = {
  onImported: (bundle: RosettaJobBundle) => void;
  isDraggingOver: boolean;
};

export function WorkspaceEmpty({ onImported, isDraggingOver }: WorkspaceEmptyProps) {
  const jobs = useRosettaStore((s) => s.jobs);
  const setActiveBundle = useRosettaStore((s) => s.setActiveBundle);
  const [isImporting, setIsImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);

  const recentJobs = [...jobs]
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
    .slice(0, 5);

  async function chooseFile() {
    setIsImporting(true);
    setImportError(null);
    try {
      const path = await pickRosettaImportPath();
      if (!path) return;
      const bundle = await importRosettaDocumentFromPath(path);
      setActiveBundle(bundle);
      onImported(bundle);
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "无法导入这个文件。");
    } finally {
      setIsImporting(false);
    }
  }

  async function chooseFolder() {
    setIsImporting(true);
    setImportError(null);
    try {
      const path = await pickRosettaImportDirectory();
      if (!path) return;
      const bundle = await importRosettaProjectFromDirectory(path);
      setActiveBundle(bundle);
      onImported(bundle);
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "无法导入这个文件夹。");
    } finally {
      setIsImporting(false);
    }
  }

  async function openRecentJob(jobId: string) {
    const { loadRosettaJob } = await import("@/lib/rosettaJobs");
    try {
      const bundle = await loadRosettaJob(jobId);
      setActiveBundle(bundle);
      onImported(bundle);
    } catch (err) {
      setImportError(err instanceof Error ? err.message : "无法加载文档。");
    }
  }

  return (
    <div className="flex h-full flex-col items-center justify-center gap-10 px-10">
      {/* Drop zone */}
      <div
        className={`flex w-full max-w-lg flex-col items-center gap-5 rounded-2xl border-2 border-dashed px-10 py-12 text-center transition-colors ${
          isDraggingOver
            ? "border-primary bg-primary/5"
            : "border-border/50 bg-muted/10"
        }`}
      >
        <div className="flex size-14 items-center justify-center rounded-2xl bg-muted/40 text-muted-foreground">
          <Upload className="size-7" strokeWidth={1.5} />
        </div>
        <div className="space-y-1.5">
          <p className="text-sm font-medium">拖入文件或文件夹</p>
          <p className="text-xs text-muted-foreground">支持 TXT · Markdown · PDF</p>
        </div>
        <div className="flex gap-2">
          <Button
            size="sm"
            onClick={() => void chooseFile()}
            disabled={isImporting}
          >
            <FileText className="size-4" /> 选择文件
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void chooseFolder()}
            disabled={isImporting}
          >
            <FolderOpen className="size-4" /> 选择文件夹
          </Button>
        </div>
        {importError && (
          <p className="text-xs text-destructive">{importError}</p>
        )}
      </div>

      {/* Recent docs */}
      {recentJobs.length > 0 && (
        <div className="w-full max-w-lg space-y-1.5">
          <p className="text-xs text-muted-foreground/60">最近</p>
          {recentJobs.map((job) => (
            <button
              key={job.id}
              type="button"
              onClick={() => void openRecentJob(job.id)}
              className="flex w-full items-center justify-between rounded-lg px-3 py-2 text-left transition-colors hover:bg-muted/40"
            >
              <span className="truncate text-sm">{job.filename}</span>
              <span className="ml-4 shrink-0 text-xs text-muted-foreground/50">
                {formatRelativeTime(job.updatedAt)}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
