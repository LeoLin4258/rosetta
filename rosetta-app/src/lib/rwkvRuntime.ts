import { invoke } from "@tauri-apps/api/core";
import type {
  RwkvRuntimeArtifactCatalog,
  RwkvRuntimeInstallPlan,
  RwkvRuntimeInstallProgress,
  RwkvRuntimeStatus,
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
