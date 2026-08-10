import { useCallback, useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  cancelPdfMarkdownExtraction,
  cancelPdfMarkdownInstall,
  getPdfMarkdownExtractionStatus,
  getPdfMarkdownInstallProgress,
  getPdfMarkdownStatus,
  installPdfMarkdownComponent,
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
  const [isInstalling, setIsInstalling] = useState(false);
  const [isStartingExtraction, setIsStartingExtraction] = useState(false);

  const refreshComponentStatus = useCallback(async () => {
    try {
      const next = await getPdfMarkdownStatus();
      setComponentStatus(next);
      return next;
    } catch (error) {
      setLastError(toMessage(error));
      return null;
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
    if (!jobId) {
      setComponentStatus(null);
      setExtractionStatus(null);
      return;
    }
    void refreshComponentStatus();
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
    async (repair: boolean) => {
      setIsInstalling(true);
      setLastError(null);
      try {
        const result = repair
          ? await repairPdfMarkdownComponent()
          : await installPdfMarkdownComponent();
        await refreshComponentStatus();
        return result;
      } catch (error) {
        const message = toMessage(error);
        setLastError(message);
        await refreshComponentStatus();
        throw new Error(message);
      } finally {
        setIsInstalling(false);
        void getPdfMarkdownInstallProgress().then(setInstallProgress).catch(() => {});
      }
    },
    [refreshComponentStatus],
  );

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
      setLastError(toMessage(error));
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
    lastError,
    isInstalling,
    isStartingExtraction,
    refreshComponentStatus,
    refreshExtractionStatus,
    install,
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
