import { useCallback, useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  cancelPdf2zhInstall,
  getPdf2zhStatus,
  installPdf2zhPack,
  subscribePdf2zhInstallProgress,
  type Pdf2zhInstallOptions,
  type Pdf2zhInstallProgress,
  type Pdf2zhInstallResult,
  type Pdf2zhStatus,
} from "@/lib/pdf2zhRuntime";
import { useRosettaStore } from "@/store/useRosettaStore";

export function useManagedPdf2zhRuntime() {
  const [status, setStatus] = useState<Pdf2zhStatus | null>(null);
  const [progress, setProgress] = useState<Pdf2zhInstallProgress | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isInstalling, setIsInstalling] = useState(false);
  const proxyUrl = useRosettaStore((state) => state.downloadProxy.url);

  const refreshStatus = useCallback(async (): Promise<Pdf2zhStatus | null> => {
    setIsRefreshing(true);
    try {
      const next = await getPdf2zhStatus();
      setStatus(next);
      return next;
    } catch (error) {
      setLastError(toMessage(error));
      return null;
    } finally {
      setIsRefreshing(false);
    }
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | null = null;

    subscribePdf2zhInstallProgress((next) => {
      if (!active) return;
      setProgress(next);
    })
      .then((fn) => {
        if (!active) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch((error) => setLastError(toMessage(error)));

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const install = useCallback(
    async (options?: Pdf2zhInstallOptions): Promise<Pdf2zhInstallResult> => {
      setIsInstalling(true);
      setLastError(null);
      try {
        const result = await installPdf2zhPack({
          ...options,
          proxyUrl: options?.proxyUrl ?? proxyUrl,
        });
        await refreshStatus();
        return result;
      } catch (error) {
        const message = toMessage(error);
        setLastError(message);
        await refreshStatus();
        throw new Error(message);
      } finally {
        setIsInstalling(false);
      }
    },
    [proxyUrl, refreshStatus]
  );

  const cancelInstall = useCallback(async (): Promise<boolean> => {
    try {
      const result = await cancelPdf2zhInstall();
      return result.cancelled;
    } catch (error) {
      setLastError(toMessage(error));
      return false;
    }
  }, []);

  return {
    status,
    progress,
    lastError,
    isRefreshing,
    isInstalling,
    refreshStatus,
    install,
    cancelInstall,
  } as const;
}

export type UseManagedPdf2zhRuntime = ReturnType<typeof useManagedPdf2zhRuntime>;

export function isPdf2zhReady(status: Pdf2zhStatus | null): boolean {
  return status?.state === "installed";
}

function toMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return JSON.stringify(error);
}
