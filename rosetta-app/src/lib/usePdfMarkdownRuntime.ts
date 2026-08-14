import { useCallback, useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

import {
  cancelPdfMarkdownExtraction,
  cancelPdfMarkdownInstall,
  getPdfMarkdownExtractionStatus,
  getPdfMarkdownInstallProgress,
  getPdfMarkdownStatus,
  installPdfMarkdownComponent,
  pdfMarkdownErrorMessage,
  repairPdfMarkdownComponent,
  startPdfMarkdownExtraction,
  subscribePdfMarkdownExtractionProgress,
  type PdfMarkdownComponentStatus,
  type PdfMarkdownExtractionStatus,
  type PdfMarkdownInstallProgress,
} from "@/lib/rosettaJobs";

export function usePdfMarkdownRuntime(jobId: string | null) {
  const [componentStatus, setComponentStatus] =
    useState<PdfMarkdownComponentStatus | null>(null);
  const [installProgress, setInstallProgress] =
    useState<PdfMarkdownInstallProgress | null>(null);
  const [extractionStatus, setExtractionStatus] =
    useState<PdfMarkdownExtractionStatus | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [isRefreshingComponent, setIsRefreshingComponent] = useState(false);
  const [isInstalling, setIsInstalling] = useState(false);
  const [isStartingExtraction, setIsStartingExtraction] = useState(false);

  const refreshComponentStatus = useCallback(async () => {
    setIsRefreshingComponent(true);
    setLastError(null);
    try {
      const next = await getPdfMarkdownStatus();
      setComponentStatus(next);
      return next;
    } catch (error) {
      setLastError(toMessage(error));
      return null;
    } finally {
      setIsRefreshingComponent(false);
    }
  }, []);

  const refreshExtractionStatus = useCallback(async () => {
    if (!jobId) {
      setExtractionStatus(null);
      return null;
    }
    try {
      const next = await getPdfMarkdownExtractionStatus(jobId);
      setExtractionStatus(next);
      return next;
    } catch (error) {
      setLastError(toMessage(error));
      return null;
    }
  }, [jobId]);

  useEffect(() => {
    void refreshComponentStatus();
    if (!jobId) {
      setExtractionStatus(null);
      return;
    }
    void refreshExtractionStatus();
  }, [jobId, refreshComponentStatus, refreshExtractionStatus]);

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | null = null;
    subscribePdfMarkdownExtractionProgress((next) => {
      if (active && next.jobId === jobId) setExtractionStatus(next);
    })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch((error) => setLastError(toMessage(error)));
    return () => {
      active = false;
      unlisten?.();
    };
  }, [jobId]);

  useEffect(() => {
    if (!isInstalling) return;
    let active = true;
    const poll = () => {
      void getPdfMarkdownInstallProgress()
        .then((next) => {
          if (active) setInstallProgress(next);
        })
        .catch((error) => {
          if (active) setLastError(toMessage(error));
        });
    };
    poll();
    const interval = window.setInterval(poll, 300);
    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, [isInstalling]);

  const install = useCallback(
    async (repair: boolean, archivePath?: string | null) => {
      setIsInstalling(true);
      setLastError(null);
      try {
        const result = archivePath
          ? await installPdfMarkdownComponent({ force: true, archivePath })
          : repair
          ? await repairPdfMarkdownComponent()
          : await installPdfMarkdownComponent();
        await refreshComponentStatus();
        return result;
      } catch (error) {
        const message = toMessage(error);
        await refreshComponentStatus();
        setLastError(message);
        throw new Error(message);
      } finally {
        setIsInstalling(false);
        void getPdfMarkdownInstallProgress().then(setInstallProgress).catch(() => {});
      }
    },
    [refreshComponentStatus],
  );

  const importFromFile = useCallback(async () => {
    try {
      const selection = await openFileDialog({
        title: "选择 PDF 转 Markdown 组件压缩包",
        multiple: false,
        directory: false,
        filters: [
          {
            name: "Markdown 组件 (.zip / .tar.gz / .tgz)",
            extensions: ["zip", "gz", "tgz"],
          },
          { name: "全部文件", extensions: ["*"] },
        ],
      });
      if (selection == null) return null;
      const isAbsolutePath =
        selection.startsWith("/") || /^[A-Za-z]:[\\/]/.test(selection);
      if (!isAbsolutePath) throw new Error(`文件路径不是绝对路径: ${selection}`);
      return await install(true, selection);
    } catch (error) {
      const message = toMessage(error);
      setLastError(message);
      throw new Error(message);
    }
  }, [install]);

  const cancelInstall = useCallback(async () => {
    try {
      return await cancelPdfMarkdownInstall();
    } catch (error) {
      setLastError(toMessage(error));
      return false;
    }
  }, []);

  const startExtraction = useCallback(async () => {
    if (!jobId) return null;
    setIsStartingExtraction(true);
    setLastError(null);
    try {
      const result = await startPdfMarkdownExtraction(jobId);
      setExtractionStatus(result);
      return result;
    } catch (error) {
      const errorCode = toMessage(error);
      setLastError(pdfMarkdownErrorMessage(errorCode) ?? errorCode);
      await refreshExtractionStatus();
      return null;
    } finally {
      setIsStartingExtraction(false);
    }
  }, [jobId, refreshExtractionStatus]);

  const cancelExtraction = useCallback(async () => {
    if (!jobId) return false;
    try {
      const cancelled = await cancelPdfMarkdownExtraction(jobId);
      if (cancelled) await refreshExtractionStatus();
      return cancelled;
    } catch (error) {
      setLastError(toMessage(error));
      return false;
    }
  }, [jobId, refreshExtractionStatus]);

  return {
    componentStatus,
    installProgress,
    extractionStatus,
    lastError:
      lastError ?? pdfMarkdownErrorMessage(extractionStatus?.errorCode),
    isRefreshingComponent,
    isInstalling,
    isStartingExtraction,
    refreshComponentStatus,
    refreshExtractionStatus,
    install,
    importFromFile,
    cancelInstall,
    startExtraction,
    cancelExtraction,
  } as const;
}

function toMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "PDF Markdown 操作失败。";
}
