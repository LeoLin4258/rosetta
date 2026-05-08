import { invoke } from "@tauri-apps/api/core";
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

export function getRwkvRuntimeArtifactCatalog() {
  return invoke<RwkvRuntimeArtifactCatalog>("get_rwkv_runtime_artifact_catalog");
}

export function getRwkvRuntimeStatus() {
  return invoke<RwkvRuntimeStatus>("get_rwkv_runtime_status");
}

export function getRwkvRuntimeInstallPlan() {
  return invoke<RwkvRuntimeInstallPlan>("get_rwkv_runtime_install_plan");
}

export function getRwkvRuntimeInstallProgress() {
  return invoke<RwkvRuntimeInstallProgress>(
    "get_rwkv_runtime_install_progress"
  );
}

export function initializeRwkvRuntimeLayout() {
  return invoke<RwkvRuntimeStatus>("initialize_rwkv_runtime_layout");
}

export function prepareRwkvRuntimeInstall() {
  return invoke<RwkvRuntimeInstallProgress>("prepare_rwkv_runtime_install");
}

export function scanRwkvRuntimeArtifacts() {
  return invoke<RwkvRuntimeArtifactScanResult>("scan_rwkv_runtime_artifacts");
}

export function extractRwkvRuntimeArtifact() {
  return invoke<RwkvRuntimeExtractionResult>("extract_rwkv_runtime_artifact");
}

export function getRwkvRuntimeProcessStatus() {
  return invoke<RwkvRuntimeProcessStatus>("get_rwkv_runtime_process_status");
}

export function startRwkvRuntime() {
  return invoke<RwkvRuntimeStartResult>("start_rwkv_runtime");
}

export function probeRwkvRuntimeTranslation() {
  return invoke<RwkvRuntimeTranslationProbeResult>(
    "probe_rwkv_runtime_translation"
  );
}
