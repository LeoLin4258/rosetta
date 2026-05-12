import type {
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
