import { useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { getCurrentWindow, type Theme } from "@tauri-apps/api/window";

import { DocumentPreview } from "./DocumentPreview";
import { cn } from "@/lib/utils";
import { loadRosettaJob } from "@/lib/rosettaJobs";
import { useRosettaStore } from "@/store/useRosettaStore";
import type { AppThemeMode, RosettaJobBundle } from "@/types/rosetta";

const appWindow = getCurrentWindow();

export function SourcePreviewPage() {
  const { jobId, sourceFileId } = useParams();
  const themeMode = useRosettaStore((state) => state.themeMode);
  const [systemPrefersDark, setSystemPrefersDark] = useState(true);
  const [jobBundle, setJobBundle] = useState<RosettaJobBundle | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const isDark = resolveIsDark(themeMode, systemPrefersDark);

  const sourceFile =
    jobBundle?.document.files.find((file) => file.id === sourceFileId) ?? null;
  const sourceSegments = useMemo(
    () =>
      jobBundle?.segments.filter(
        (segment) => (segment.fileId ?? "file-1") === (sourceFile?.id ?? "")
      ) ?? [],
    [jobBundle?.segments, sourceFile?.id]
  );

  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    function syncSystemTheme() {
      setSystemPrefersDark(mediaQuery.matches);
    }

    syncSystemTheme();
    mediaQuery.addEventListener("change", syncSystemTheme);

    return () => mediaQuery.removeEventListener("change", syncSystemTheme);
  }, []);

  useEffect(() => {
    const windowTheme: Theme | null = themeMode === "system" ? null : themeMode;

    void appWindow.setTheme(windowTheme).catch(() => {
      // Plain browser dev mode does not expose the Tauri window API.
    });
  }, [themeMode]);

  useEffect(() => {
    if (!jobId || !sourceFileId) {
      setError("缺少原文预览参数。");
      setIsLoading(false);
      return;
    }

    let cancelled = false;
    setIsLoading(true);
    setError(null);

    void loadRosettaJob(jobId)
      .then((loadedJob) => {
        if (cancelled) {
          return;
        }
        setJobBundle(loadedJob);
        const source = loadedJob.document.files.find(
          (file) => file.id === sourceFileId
        );
        void appWindow
          .setTitle(source?.relativePath ?? source?.filename ?? "原文预览")
          .catch(() => {});
      })
      .catch((loadError) => {
        if (cancelled) {
          return;
        }
        setError(
          loadError instanceof Error ? loadError.message : "原文预览加载失败。"
        );
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [jobId, sourceFileId]);

  return (
    <div
      className={cn(
        "flex h-screen flex-col bg-background text-foreground",
        isDark && "dark"
      )}
    >
      <header className="flex h-14 shrink-0 items-center border-b bg-[#f3f1e9] px-4 dark:bg-stone-900">
        <div className="min-w-0">
          <h1 className="truncate text-base font-semibold">
            {sourceFile?.relativePath ?? "原文预览"}
          </h1>
          <p className="mt-0.5 text-sm text-muted-foreground">
            {sourceSegments.length} 段
          </p>
        </div>
      </header>

      <main className="min-h-0 flex-1 bg-[#f3f1e9] p-4 dark:bg-stone-900">
        {isLoading ? (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            正在加载原文预览...
          </div>
        ) : error ? (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            {error}
          </div>
        ) : (
          <DocumentPreview
            document={jobBundle?.document ?? null}
            layout="source"
            sourceFile={sourceFile}
            sourceSegments={sourceSegments}
            translationFile={null}
            translationSegments={[]}
          />
        )}
      </main>
    </div>
  );
}

function resolveIsDark(themeMode: AppThemeMode, systemPrefersDark: boolean) {
  return themeMode === "system" ? systemPrefersDark : themeMode === "dark";
}
