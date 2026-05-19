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

export function installPdf2zhPack() {
  return invoke<string>("install_pdf2zh_pack");
}

export function cancelPdf2zhInstall() {
  return invoke<string>("cancel_pdf2zh_install");
}

export function getPdf2zhInstallProgress() {
  return invoke<{ state: string; message: string }>("get_pdf2zh_install_progress");
}
