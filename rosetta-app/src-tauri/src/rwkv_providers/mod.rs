use std::sync::{atomic::AtomicBool, Arc};

pub mod mobile_batch_chat;

/// Provider-neutral batch translation input.
///
/// Captures the per-batch parameters every provider needs to actually issue a
/// translate request, plus an optional cancellation flag shared with the run
/// orchestrator. The same shape is used by probes and full translation runs.
///
/// `source_lang` deliberately is **not** in this struct: with the
/// `rwkv-mobile-batch-chat` provider, language direction is global server
/// state set once per run via `set_chat_roles_for_pair`. Mixing it into the
/// per-batch struct invites callers to set roles on every batch (wasteful) or
/// to forget to set them at run start (silent direction bug). The orchestrator
/// owns direction; providers only need `target_lang` here for response-prefix
/// stripping.
pub struct ProviderTranslateBatch<'a> {
    pub source_texts: &'a [String],
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
