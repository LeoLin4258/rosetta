import { invoke } from "@tauri-apps/api/core";
import type {
  RwkvTranslationApiProbeRequest,
  RwkvTranslationApiProbeResult,
  RwkvTranslationApiTranslateRequest,
  RwkvTranslationApiTranslateResult,
  RwkvTranslationRunStartRequest,
  RwkvTranslationRunStatus,
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

export function startRwkvTranslationRun(request: RwkvTranslationRunStartRequest) {
  return invoke<RwkvTranslationRunStatus>("start_rwkv_translation_run", {
    request,
  });
}

export function cancelRwkvTranslationRun(runId: string) {
  return invoke<RwkvTranslationRunStatus>("cancel_rwkv_translation_run", {
    runId,
  });
}

export function getRwkvTranslationRunStatus(runId: string) {
  return invoke<RwkvTranslationRunStatus>("get_rwkv_translation_run_status", {
    runId,
  });
}
