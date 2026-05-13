import { invoke } from "@tauri-apps/api/core";
import type {
  RwkvMobileBatchChatProbeRequest,
  RwkvMobileBatchChatRunStartRequest,
  RwkvMobileBatchChatTranslateRequest,
  RwkvTranslationApiProbeRequest,
  RwkvTranslationApiProbeResult,
  RwkvTranslationApiTranslateRequest,
  RwkvTranslationApiTranslateResult,
  RwkvTranslationRunStartRequest,
  RwkvTranslationRunStatus,
} from "../types/rosetta";

// -----------------------------------------------------------------------------
// rwkv-lightning-contents provider (current external API path)
// -----------------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// rwkv-mobile-batch-chat provider (local managed sidecar via /v1/batch/chat)
// -----------------------------------------------------------------------------

export function probeRwkvMobileBatchChat(
  request: RwkvMobileBatchChatProbeRequest
) {
  return invoke<RwkvTranslationApiProbeResult>(
    "probe_rwkv_mobile_batch_chat",
    { request }
  );
}

export function translateRwkvMobileBatchChatTexts(
  request: RwkvMobileBatchChatTranslateRequest
) {
  return invoke<RwkvTranslationApiTranslateResult>(
    "translate_rwkv_mobile_batch_chat_texts",
    { request }
  );
}

export function startRwkvMobileBatchChatRun(
  request: RwkvMobileBatchChatRunStartRequest
) {
  return invoke<RwkvTranslationRunStatus>("start_rwkv_mobile_batch_chat_run", {
    request,
  });
}

// -----------------------------------------------------------------------------
// Shared run lifecycle commands (provider-agnostic, keyed by runId)
// -----------------------------------------------------------------------------

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
