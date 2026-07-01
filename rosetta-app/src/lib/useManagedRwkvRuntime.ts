import { useCallback, useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  cancelManagedRwkvInstall,
  getManagedRwkvRuntimeLogsSummary,
  getManagedRwkvRuntimeStatus,
  installManagedRwkvRuntime,
  probeManagedRwkvRuntime,
  startManagedRwkvRuntime,
  stopManagedRwkvRuntime,
  subscribeManagedRwkvInstallProgress,
} from "@/lib/rwkvRuntime";
import { selectManagedRuntimeProfileStatus } from "@/lib/managedRuntimeSelection";
import { useRosettaStore } from "@/store/useRosettaStore";
import type {
  ManagedRuntimeInstallOptions,
  ManagedRuntimeInstallResult,
  ManagedRuntimeLogsSummary,
  ManagedRuntimeProbeResult,
  ManagedRuntimeStartResult,
  ManagedRuntimeStatus,
} from "@/types/rosetta";

/**
 * Single hook that owns every UI-facing slice of managed RWKV runtime state:
 *
 * - Loads `get_managed_rwkv_runtime_status` once on mount + after each action.
 * - Subscribes to `managed-rwkv://install-progress` for live progress.
 * - Wraps the seven Tauri commands so callers don't repeat error-mapping
 *   boilerplate or forget to re-fetch status afterwards.
 *
 * Pure plumbing — no UI opinions; LocalRwkvPanel composes on top.
 */
export function useManagedRwkvRuntime() {
  const status = useRosettaStore((s) => s.managedRuntime.status);
  const selectedProfileId = useRosettaStore(
    (s) => s.rwkv.managedRuntimeProfileId
  );
  const progress = useRosettaStore((s) => s.managedRuntime.progress);
  const lastError = useRosettaStore((s) => s.managedRuntime.lastError);
  const setStatus = useRosettaStore((s) => s.setManagedRuntimeStatus);
  const setProgress = useRosettaStore((s) => s.setManagedRuntimeProgress);
  const setError = useRosettaStore((s) => s.setManagedRuntimeError);

  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isInstalling, setIsInstalling] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [isProbing, setIsProbing] = useState(false);
  const activeProfileId =
    selectManagedRuntimeProfileStatus(status, selectedProfileId)?.profile.id ??
    selectedProfileId ??
    undefined;

  const refreshStatus = useCallback(async (
    profileId?: string | null
  ): Promise<ManagedRuntimeStatus | null> => {
    setIsRefreshing(true);
    try {
      const next = await getManagedRwkvRuntimeStatus(profileId ?? selectedProfileId);
      setStatus(next);
      return next;
    } catch (error) {
      setError(toMessage(error));
      return null;
    } finally {
      setIsRefreshing(false);
    }
  }, [selectedProfileId, setStatus, setError]);

  // On mount: probe status + subscribe to progress events.
  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let active = true;
    subscribeManagedRwkvInstallProgress((next) => {
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
      .catch((error) => {
        setError(toMessage(error));
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [setProgress, setError]);

  const proxyUrl = useRosettaStore((s) => s.downloadProxy.url);

  const install = useCallback(
    async (options?: ManagedRuntimeInstallOptions): Promise<ManagedRuntimeInstallResult | null> => {
      setIsInstalling(true);
      setError(null);
      const targetProfileId = options?.profileId ?? activeProfileId;
      try {
        // Inject the user-configured download proxy unless the caller
        // explicitly passed `proxyUrl` (e.g. for a one-off override). Empty
        // string → no proxy, so the Rust side falls back to env / no-proxy.
        const merged: ManagedRuntimeInstallOptions = {
          ...options,
          profileId: targetProfileId,
          proxyUrl: options?.proxyUrl ?? proxyUrl,
        };
        const result = await installManagedRwkvRuntime(merged);
        // Refresh status after install — model file existence + install plan
        // both flip when this succeeds.
        await refreshStatus(targetProfileId);
        return result;
      } catch (error) {
        setError(toMessage(error));
        await refreshStatus(targetProfileId);
        return null;
      } finally {
        setIsInstalling(false);
      }
    },
    [activeProfileId, refreshStatus, setError, proxyUrl]
  );

  const cancelInstall = useCallback(async (): Promise<boolean> => {
    try {
      const result = await cancelManagedRwkvInstall();
      return result.cancelled;
    } catch (error) {
      setError(toMessage(error));
      return false;
    }
  }, [setError]);

  const start = useCallback(async (
    profileId?: string | null
  ): Promise<ManagedRuntimeStartResult | null> => {
    const targetProfileId = profileId ?? activeProfileId;
    setIsStarting(true);
    setError(null);
    try {
      const result = await startManagedRwkvRuntime(targetProfileId);
      await refreshStatus(targetProfileId);
      return result;
    } catch (error) {
      setError(toMessage(error));
      await refreshStatus(targetProfileId);
      return null;
    } finally {
      setIsStarting(false);
    }
  }, [activeProfileId, refreshStatus, setError]);

  const stop = useCallback(async (profileId?: string | null): Promise<boolean> => {
    const targetProfileId = profileId ?? activeProfileId;
    setIsStopping(true);
    setError(null);
    try {
      await stopManagedRwkvRuntime(targetProfileId);
      await refreshStatus(targetProfileId);
      return true;
    } catch (error) {
      setError(toMessage(error));
      return false;
    } finally {
      setIsStopping(false);
    }
  }, [activeProfileId, refreshStatus, setError]);

  const probe = useCallback(async (
    profileId?: string | null
  ): Promise<ManagedRuntimeProbeResult | null> => {
    const targetProfileId = profileId ?? activeProfileId;
    setIsProbing(true);
    try {
      const result = await probeManagedRwkvRuntime(targetProfileId);
      return result;
    } catch (error) {
      setError(toMessage(error));
      return null;
    } finally {
      setIsProbing(false);
    }
  }, [activeProfileId, setError]);

  const readLogs = useCallback(async (
    profileId?: string | null
  ): Promise<ManagedRuntimeLogsSummary | null> => {
    const targetProfileId = profileId ?? activeProfileId;
    try {
      return await getManagedRwkvRuntimeLogsSummary(targetProfileId);
    } catch (error) {
      setError(toMessage(error));
      return null;
    }
  }, [activeProfileId, setError]);

  return {
    status,
    progress,
    lastError,
    isRefreshing,
    isInstalling,
    isStarting,
    isStopping,
    isProbing,
    refreshStatus,
    install,
    cancelInstall,
    start,
    stop,
    probe,
    readLogs,
  } as const;
}

export type UseManagedRwkvRuntime = ReturnType<typeof useManagedRwkvRuntime>;

function toMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  return JSON.stringify(error);
}

/** Convenience selector for whether `runTranslationBatches` can dispatch to
 *  the managed sidecar provider. Mirrors `selectProvider`'s gate so call sites
 *  on the Jobs page can branch without duplicating logic. */
export function isManagedRuntimeReady(status: ManagedRuntimeStatus | null): boolean {
  return status?.state === "ready" && !!status.process.baseUrl;
}
