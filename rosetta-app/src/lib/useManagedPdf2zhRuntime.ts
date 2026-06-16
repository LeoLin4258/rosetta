import { useCallback, useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

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

  /**
   * Manual-import path for users who can't reach GitHub Releases (mainland
   * China without VPN, etc). Opens a native file picker scoped to `.tar.gz`,
   * then feeds the chosen path to the install pipeline as a `file://` URL.
   *
   * Reuses every other invariant of the regular install flow — SHA256 check,
   * size check, extraction, manifest write. The only difference is the
   * "download" step copies bytes from the user's chosen file instead of
   * pulling them over HTTP. See `managed_pdf2zh::install::copy_file_url` for
   * the backend half.
   *
   * Returns `null` when the user cancels the picker; throws on install
   * failure (caller surfaces `lastError` from state).
   */
  const importFromFile = useCallback(
    async (): Promise<Pdf2zhInstallResult | null> => {
      const selection = await openFileDialog({
        title: "选择 PDF 组件压缩包",
        multiple: false,
        directory: false,
        // macOS / Windows file pickers look at the LAST extension only, so
        // `.tar.gz` is reported as `gz` and we need to match that, not the
        // compound form. `tgz` is the alternative single-extension naming.
        // Fall back to `*` so a user with a renamed file (or unexpected
        // double-extension behavior on some Linux DEs) isn't locked out.
        filters: [
          { name: "PDF 组件 (.tar.gz / .tgz)", extensions: ["gz", "tgz"] },
          { name: "全部文件", extensions: ["*"] },
        ],
      });
      // `open` returns `null` when the user cancels. (Tauri 2's typed
      // signature already excludes the array form because we passed
      // `multiple: false`.)
      if (selection == null) {
        return null;
      }
      const localPath = selection;
      // file:// URLs need an absolute path. macOS file picker always returns
      // absolute paths, but we sanity-check rather than letting a malformed
      // URL silently fall through to the HTTP branch.
      if (!localPath.startsWith("/")) {
        throw new Error(`文件路径不是绝对路径: ${localPath}`);
      }
      return await install({
        repair: true,
        // `repair: true` clears any partial download / mismatched cache from a
        // previous failed attempt — we want this import to start clean,
        // because the user is here specifically because the normal download
        // path didn't work.
        packUrl: `file://${localPath}`,
      });
    },
    [install]
  );

  return {
    status,
    progress,
    lastError,
    isRefreshing,
    isInstalling,
    refreshStatus,
    install,
    importFromFile,
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
