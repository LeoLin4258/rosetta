import { invoke } from "@tauri-apps/api/core";

export type Pdf2zhStatus = {
  state: "unsupported" | "not-installed" | "installed";
  message: string;
  profile: {
    id: string;
    platformOs: string;
    platformArch: string;
    packDirectoryName: string;
  } | null;
  paths: {
    bin: string | null;
    packDir: string;
    logsDir: string;
  } | null;
  installPlan: {
    ready: boolean;
    message: string;
  } | null;
};

export function getPdf2zhStatus() {
  return invoke<Pdf2zhStatus>("get_pdf2zh_status");
}

export type Pdf2zhInstallPhase =
  | "idle"
  | "preflight"
  | "downloading"
  | "verifying"
  | "extracting"
  | "done"
  | "failed"
  | "cancelled";

export type Pdf2zhInstallOptions = {
  repair?: boolean;
  proxyUrl?: string | null;
  packUrl?: string | null;
  packSha256?: string | null;
  packSizeBytes?: number | null;
};

export type Pdf2zhInstallProgress = {
  phase: Pdf2zhInstallPhase;
  bytesDone: number;
  bytesTotal: number;
  sourceUrl: string | null;
  speedBytesPerSec: number;
  startedAt: string | null;
  message: string;
  lastError: string | null;
};

export type Pdf2zhInstallResult = {
  ready: boolean;
  installed: boolean;
  phase: Pdf2zhInstallPhase;
  bytesDone: number;
  bytesTotal: number;
  sourceUrl: string | null;
  message: string;
  manifestPath: string;
};

export type Pdf2zhCancelInstallResult = {
  cancelled: boolean;
  message: string;
};

export function installPdf2zhPack(options?: Pdf2zhInstallOptions) {
  return invoke<Pdf2zhInstallResult>("install_pdf2zh_pack", { options });
}

export function cancelPdf2zhInstall() {
  return invoke<Pdf2zhCancelInstallResult>("cancel_pdf2zh_install");
}

export function getPdf2zhInstallProgress() {
  return invoke<Pdf2zhInstallProgress>("get_pdf2zh_install_progress");
}
