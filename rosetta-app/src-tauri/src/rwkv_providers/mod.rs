use std::sync::{atomic::AtomicBool, Arc};

pub mod llama_cpp_chat;
pub mod mobile_batch_chat;

/// Provider-neutral batch translation input.
///
/// Captures the per-batch parameters every provider needs to actually issue a
/// translate request, plus an optional cancellation flag shared with the run
/// orchestrator. The same shape is used by probes and full translation runs.
///
/// `source_lang` is optional: the `rwkv-mobile-batch-chat` provider sets
/// direction globally via `set_chat_roles_for_pair` and ignores it, while the
/// `llama-cpp` provider needs it per-request to build the raw prompt.
pub struct ProviderTranslateBatch<'a> {
    pub source_texts: &'a [String],
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    pub timeout_ms: u64,
    pub cancel: Option<Arc<AtomicBool>>,
    pub debug_context: Option<&'a str>,
}

/// Provider-neutral batch translation result.
///
/// Mirrors the public `RwkvTranslationApi*Result` shapes so that the dispatch
/// layer in `rwkv_api` can convert provider output into the existing Tauri
/// command return type without adapter friction.
#[derive(Debug, Clone)]
pub struct ProviderTranslateResult {
    pub ok: bool,
    pub status_code: Option<u16>,
    pub translations: Vec<String>,
    pub raw_response_preview: String,
    pub message: String,
    pub latency_ms: u128,
}
