import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
  packPath?: string | null;
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

export function subscribePdf2zhInstallProgress(
  handler: (progress: Pdf2zhInstallProgress) => void
): Promise<UnlistenFn> {
  return listen<Pdf2zhInstallProgress>(
    "managed-pdf2zh://install-progress",
    (event) => handler(event.payload)
  );
}

/// Start the persistent pdf2zh worker so its heavy Python import (~13 s) is
/// already paid by the time the user clicks 翻译. Idempotent; resolves true
/// when the worker is warm. Fire-and-forget — failure just means the next
/// translate falls back to a cold start.
export function prewarmPdf2zhWorker() {
  return invoke<boolean>("prewarm_pdf2zh_worker");
}

/// Lifecycle states the backend broadcasts for the header indicator.
/// - `idle`: never started this session (transient)
/// - `not-installed`: pdf2zh pack missing — indicator hides itself
/// - `starting`: handshake in flight (~13 s torch import)
/// - `ready`: warm worker, accepting jobs immediately
/// - `translating`: a job is running; flips back to `ready` when it returns
/// - `failed`: last spawn errored; `message` carries the user-facing reason
export type Pdf2zhWorkerStatusState =
  | "idle"
  | "not-installed"
  | "starting"
  | "ready"
  | "translating"
  | "failed";

export type Pdf2zhWorkerStatus = {
  state: Pdf2zhWorkerStatusState;
  message: string | null;
  importMs: number | null;
  /// Populated only while `state === "starting"` so the UI can show
  /// "[N/M label]" — without this, a 30 s+ first-launch warmup sits on a
  /// single static "PDF 引擎预热中" label and looks frozen.
  warmupStep: number | null;
  warmupTotalSteps: number | null;
  warmupLabel: string | null;
};

export function getPdf2zhWorkerStatus() {
  return invoke<Pdf2zhWorkerStatus>("get_pdf2zh_worker_status");
}

export function subscribePdf2zhWorkerStatus(
  handler: (status: Pdf2zhWorkerStatus) => void
): Promise<UnlistenFn> {
  return listen<Pdf2zhWorkerStatus>(
    "rosetta-pdf2zh-worker-status",
    (event) => handler(event.payload)
  );
}
