import { invoke } from "@tauri-apps/api/core";
import type {
  RwkvTranslationApiProbeRequest,
  RwkvTranslationApiProbeResult,
} from "../types/rosetta";

export function probeRwkvTranslationApi(request: RwkvTranslationApiProbeRequest) {
  return invoke<RwkvTranslationApiProbeResult>(
    "probe_rwkv_translation_api",
    { request }
  );
}
