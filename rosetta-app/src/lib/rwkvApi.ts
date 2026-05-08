import { invoke } from "@tauri-apps/api/core";
import type {
  RwkvTranslationApiProbeRequest,
  RwkvTranslationApiProbeResult,
  RwkvTranslationApiTranslateRequest,
  RwkvTranslationApiTranslateResult,
} from "../types/rosetta";

export function probeRwkvTranslationApi(request: RwkvTranslationApiProbeRequest) {
  return invoke<RwkvTranslationApiProbeResult>(
    "probe_rwkv_translation_api",
    { request }
  );
}

export function translateRwkvTextsWithApi(
  request: RwkvTranslationApiTranslateRequest
) {
  return invoke<RwkvTranslationApiTranslateResult>(
    "translate_rwkv_texts_with_api",
    { request }
  );
}
