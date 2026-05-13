use std::sync::{atomic::AtomicBool, Arc};

pub mod mobile_batch_chat;

/// Provider-neutral batch translation input.
///
/// Captures the per-batch parameters every provider needs, plus an optional
/// cancellation flag shared with the run orchestrator. The same shape is used
/// by probes and full translation runs; probes just pass canned source texts.
pub struct ProviderTranslateBatch<'a> {
    pub source_texts: &'a [String],
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    pub timeout_ms: u64,
    pub cancel: Option<Arc<AtomicBool>>,
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
