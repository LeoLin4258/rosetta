import { invoke } from "@tauri-apps/api/core";
import type { RwkvRuntimeStatus } from "../types/rosetta";

export function getRwkvRuntimeStatus() {
  return invoke<RwkvRuntimeStatus>("get_rwkv_runtime_status");
}
