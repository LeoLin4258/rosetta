import { invoke } from "@tauri-apps/api/core";
import type {
  RwkvRuntimeInstallPlan,
  RwkvRuntimeStatus,
} from "../types/rosetta";

export function getRwkvRuntimeStatus() {
  return invoke<RwkvRuntimeStatus>("get_rwkv_runtime_status");
}

export function getRwkvRuntimeInstallPlan() {
  return invoke<RwkvRuntimeInstallPlan>("get_rwkv_runtime_install_plan");
}

export function initializeRwkvRuntimeLayout() {
  return invoke<RwkvRuntimeStatus>("initialize_rwkv_runtime_layout");
}
