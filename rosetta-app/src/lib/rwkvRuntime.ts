import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ManagedRuntimeCancelInstallResult,
  ManagedRuntimeInstallOptions,
  ManagedRuntimeInstallPlan,
  ManagedRuntimeInstallProgress,
  ManagedRuntimeInstallResult,
  ManagedRuntimeLogsSummary,
  ManagedRuntimeProbeResult,
  ManagedRuntimeStartResult,
  ManagedRuntimeStatus,
  RwkvRuntimeArtifactCatalog,
  RwkvRuntimeArtifactScanResult,
  RwkvRuntimeExtractionResult,
  RwkvRuntimeInstallPlan,
  RwkvRuntimeInstallProgress,
  RwkvRuntimeProcessStatus,
  RwkvRuntimeStartResult,
  RwkvRuntimeStatus,
  RwkvRuntimeTranslationProbeResult,
} from "../types/rosetta";

const PAUSED_RUNTIME_MESSAGE =
  "Managed RWKV runtime commands are paused. Configure an existing RWKV translation API in Settings instead.";

function rejectPausedRuntime<T>(): Promise<T> {
  return Promise.reject(new Error(PAUSED_RUNTIME_MESSAGE));
}

// -----------------------------------------------------------------------------
// Managed local RWKV runtime (Phase 3, ADR 0003).
//
// These wrap the seven Tauri commands defined in `src-tauri/src/managed_rwkv/`.
// They are intentionally separate from the legacy `*RwkvRuntime*` stubs above,
// which remain "paused" until the Windows path resumes in Phase 8.
// -----------------------------------------------------------------------------

export function getManagedRwkvRuntimeStatus(profileId?: string | null) {
  return invoke<ManagedRuntimeStatus>("get_managed_rwkv_runtime_status", {
    profileId,
  });
}

export function getManagedRwkvInstallPlan(profileId?: string | null) {
  return invoke<ManagedRuntimeInstallPlan>("get_managed_rwkv_install_plan", {
    profileId,
  });
}

export function installManagedRwkvRuntime(
  options?: ManagedRuntimeInstallOptions
) {
  return invoke<ManagedRuntimeInstallResult>("install_managed_rwkv_runtime", {
    options,
  });
}

export function getManagedRwkvInstallProgress() {
  return invoke<ManagedRuntimeInstallProgress>(
    "get_managed_rwkv_install_progress"
  );
}

export function cancelManagedRwkvInstall() {
  return invoke<ManagedRuntimeCancelInstallResult>(
    "cancel_managed_rwkv_install"
  );
}

/**
 * Subscribe to live install-progress events emitted by the Rust install
 * pipeline (`managed-rwkv://install-progress`). Calls `handler` with the
 * latest `ManagedRuntimeInstallProgress` each time the Rust side emits.
 *
 * Returns an unlisten function — call it in the React effect cleanup to
 * avoid leaking subscriptions across mounts. Rust throttles emissions to
 * roughly 10/sec, so it's safe to render on each call.
 */
export function subscribeManagedRwkvInstallProgress(
  handler: (progress: ManagedRuntimeInstallProgress) => void
): Promise<UnlistenFn> {
  return listen<ManagedRuntimeInstallProgress>(
    "managed-rwkv://install-progress",
    (event) => handler(event.payload)
  );
}

export function startManagedRwkvRuntime(profileId?: string | null) {
  return invoke<ManagedRuntimeStartResult>("start_managed_rwkv_runtime", {
    profileId,
  });
}

export function stopManagedRwkvRuntime(profileId?: string | null) {
  return invoke<string>("stop_managed_rwkv_runtime", { profileId });
}

export function probeManagedRwkvRuntime(profileId?: string | null) {
  return invoke<ManagedRuntimeProbeResult>("probe_managed_rwkv_runtime", {
    profileId,
  });
}

export function getManagedRwkvRuntimeLogsSummary(profileId?: string | null) {
  return invoke<ManagedRuntimeLogsSummary>(
    "get_managed_rwkv_runtime_logs_summary",
    { profileId }
  );
}

// -----------------------------------------------------------------------------
// Legacy paused stubs — preserve existing call-site behavior unchanged.
// -----------------------------------------------------------------------------

export function getRwkvRuntimeArtifactCatalog() {
  return rejectPausedRuntime<RwkvRuntimeArtifactCatalog>();
}

export function getRwkvRuntimeStatus() {
  return rejectPausedRuntime<RwkvRuntimeStatus>();
}

export function getRwkvRuntimeInstallPlan() {
  return rejectPausedRuntime<RwkvRuntimeInstallPlan>();
}

export function getRwkvRuntimeInstallProgress() {
  return rejectPausedRuntime<RwkvRuntimeInstallProgress>();
}

export function initializeRwkvRuntimeLayout() {
  return rejectPausedRuntime<RwkvRuntimeStatus>();
}

export function prepareRwkvRuntimeInstall() {
  return rejectPausedRuntime<RwkvRuntimeInstallProgress>();
}

export function scanRwkvRuntimeArtifacts() {
  return rejectPausedRuntime<RwkvRuntimeArtifactScanResult>();
}

export function extractRwkvRuntimeArtifact() {
  return rejectPausedRuntime<RwkvRuntimeExtractionResult>();
}

export function getRwkvRuntimeProcessStatus() {
  return rejectPausedRuntime<RwkvRuntimeProcessStatus>();
}

export function startRwkvRuntime() {
  return rejectPausedRuntime<RwkvRuntimeStartResult>();
}

export function probeRwkvRuntimeTranslation() {
  return rejectPausedRuntime<RwkvRuntimeTranslationProbeResult>();
}
