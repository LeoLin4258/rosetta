use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{timeout_at, Duration, Instant},
};

use crate::rwkv_providers::{
    llama_cpp_chat,
    mobile_batch_chat::{self, MobileBatchChatConfig},
    ProviderTranslateBatch,
};

/// Default paragraph batch width for PDF providers that do not report their
/// own supported sizes. Also drives pdf2zh's `thread` count below. Generic
/// providers stay at 8 because that was the measured stable first-page point;
/// llama.cpp follows the managed runtime's parallel setting to keep client
/// concurrency aligned with llama-server slots during benchmark experiments.
const DEFAULT_MAX_BATCH_SIZE: usize = 8;
const PDF_SHIM_BODY_TARGET_PROMPT_TOKENS: usize = 150;
const PDF_SHIM_BODY_HARD_PROMPT_TOKENS: usize = 190;
const PDF_SHIM_CAPTION_TARGET_PROMPT_TOKENS: usize = 150;
const PDF_SHIM_CAPTION_HARD_PROMPT_TOKENS: usize = 190;
const PDF_SHIM_REFERENCE_TARGET_PROMPT_TOKENS: usize = 130;
const PDF_SHIM_REFERENCE_HARD_PROMPT_TOKENS: usize = 170;
const PDF_SHIM_LLAMA_BODY_TARGET_PROMPT_TOKENS: usize = 56;
const PDF_SHIM_LLAMA_BODY_HARD_PROMPT_TOKENS: usize = 72;
const PDF_SHIM_LLAMA_CAPTION_TARGET_PROMPT_TOKENS: usize = 56;
const PDF_SHIM_LLAMA_CAPTION_HARD_PROMPT_TOKENS: usize = 72;
const PDF_SHIM_LLAMA_REFERENCE_TARGET_PROMPT_TOKENS: usize = 42;
const PDF_SHIM_LLAMA_REFERENCE_HARD_PROMPT_TOKENS: usize = 56;
const PDF_SHIM_LLAMA_WIDE_SLOT_CONTEXT_TOKENS: usize = 1024;
const PDF_SHIM_LLAMA_WIDE_BODY_TARGET_PROMPT_TOKENS: usize = 72;
const PDF_SHIM_LLAMA_WIDE_BODY_HARD_PROMPT_TOKENS: usize = 88;
const PDF_SHIM_LLAMA_WIDE_CAPTION_TARGET_PROMPT_TOKENS: usize = 72;
const PDF_SHIM_LLAMA_WIDE_CAPTION_HARD_PROMPT_TOKENS: usize = 88;
const PDF_SHIM_LLAMA_WIDE_REFERENCE_TARGET_PROMPT_TOKENS: usize = 42;
const PDF_SHIM_LLAMA_WIDE_REFERENCE_HARD_PROMPT_TOKENS: usize = 56;
const PDF_SHIM_REFERENCE_PASSTHROUGH_MAX_CHARS: usize = 40;
const PDF_SHIM_RETRY_TARGET_PROMPT_TOKENS: usize = 36;
const PDF_SHIM_RETRY_HARD_PROMPT_TOKENS: usize = 36;
const PDF_SHIM_FINAL_RETRY_TARGET_PROMPT_TOKENS: usize = 24;
const PDF_SHIM_FINAL_RETRY_HARD_PROMPT_TOKENS: usize = 24;
const PDF_SHIM_LLAMA_BODY_TARGET_ENV: &str = "ROSETTA_PDF_SHIM_LLAMA_BODY_TARGET";
const PDF_SHIM_LLAMA_BODY_HARD_ENV: &str = "ROSETTA_PDF_SHIM_LLAMA_BODY_HARD";
const PDF_SHIM_LLAMA_CAPTION_TARGET_ENV: &str = "ROSETTA_PDF_SHIM_LLAMA_CAPTION_TARGET";
const PDF_SHIM_LLAMA_CAPTION_HARD_ENV: &str = "ROSETTA_PDF_SHIM_LLAMA_CAPTION_HARD";
const PDF_SHIM_LLAMA_REFERENCE_TARGET_ENV: &str = "ROSETTA_PDF_SHIM_LLAMA_REFERENCE_TARGET";
const PDF_SHIM_LLAMA_REFERENCE_HARD_ENV: &str = "ROSETTA_PDF_SHIM_LLAMA_REFERENCE_HARD";

/// Upper bound on `pdf2zh --thread`. pdf2zh.py spawns this many Python
/// multiprocessing workers; pushing it too high makes the worker-pool setup
/// dominate the per-page cost and at the extreme can deadlock before any
/// worker reaches the OpenAI shim. Decoupled from the RWKV server's
/// reported `supported_batch_sizes` ceiling — the MLX backend truthfully
/// reports up to 16, but pdf2zh's multiprocessing setup hits its own
/// scaling limit well before that.
///
/// History:
/// - 4 — initial cap after the 2026-06-10 MLX switch. At the time `threads=16`
///   appeared to hang on small PDFs and the failure was attributed to the
///   Python multiprocessing pool. *Caveat*: that diagnosis predated the
///   `NO_PROXY` fix in `pdf2zh_invoke.rs`, so the hang may actually have been
///   pdf2zh's OpenAI calls all 502'ing through Clash rather than a real
///   pool-scaling issue. Retained for reference, not as evidence against 16.
/// - 8 — bumped 2026-06-10 to let MLX run wider batches per `/v1/batch/chat`.
///   Verified stable on M4 mini.
/// - 16 — bumped 2026-06-10 to match markdown's RWKV-side batch ceiling
///   (markdown plans batches up to 16 directly via `plan_batches`). At 8
///   PDF was running batches half the size of markdown's, leaving half the
///   MLX throughput idle. Now that the `NO_PROXY` env scrubbing is in place
///   in `pdf2zh_invoke.rs`, this should let pdf2zh's 16 workers all reach
///   the shim and let MLX run at full batch width.
///
/// If you bump this further (or have to back off from 16):
/// 1. Watch the dev terminal for `[pdf2zh-batch] assembled N item(s)` lines.
///    If N stays well below the cap, workers aren't actually arriving in
///    parallel (and raising the cap won't help — investigate why).
/// 2. Watch `rosetta-pdf2zh-live.log` for repeated errors. The 2026-06-10
///    hang at `threads=16` left zero live.log artifacts because live.log
///    didn't exist yet; now you should see actual stderr if anything's wrong.
/// 3. If shim.log stays at only the `spawn shim` line for >60s with no
///    `request messages=...`, the multiprocessing pool genuinely deadlocked —
///    back off to 12 or 14.
/// 4. Check `runtime.log` "active batches" stays bounded by the new ceiling.
/// 5. Test against a 1-2 page PDF first; small inputs expose pool-startup
///    overhead the worst.
const PDF2ZH_THREAD_CEILING: usize = 16;

#[derive(Debug, Clone)]
pub struct LightningApiConfig {
    pub base_url: String,
    pub endpoint: String,
    pub internal_token: String,
    pub body_password: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct LlamaCppApiConfig {
    pub base_url: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub enum ShimProviderConfig {
    MobileBatch(MobileBatchChatConfig),
    Lightning(LightningApiConfig),
    LlamaCpp(LlamaCppApiConfig),
}

const BATCH_WINDOW_MS: u64 = 80;

/// Aggregated RWKV request timing for one shim lifetime. Written by the batch
/// processors, read after the run to build the diagnostics profile. Counts
/// and durations only — never text content.
#[derive(Debug, Default)]
pub struct ShimRwkvMetrics {
    pub request_count: AtomicU64,
    pub failed_request_count: AtomicU64,
    pub total_request_ms: AtomicU64,
    pub max_request_ms: AtomicU64,
    pub total_input_chars: AtomicU64,
    pub total_output_chars: AtomicU64,
}

impl ShimRwkvMetrics {
    fn record(&self, elapsed_ms: u64, ok: bool, input_chars: u64, output_chars: u64) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        if !ok {
            self.failed_request_count.fetch_add(1, Ordering::Relaxed);
        }
        self.total_request_ms
            .fetch_add(elapsed_ms, Ordering::Relaxed);
        self.max_request_ms.fetch_max(elapsed_ms, Ordering::Relaxed);
        self.total_input_chars
            .fetch_add(input_chars, Ordering::Relaxed);
        self.total_output_chars
            .fetch_add(output_chars, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ShimRwkvMetricsSnapshot {
        let request_count = self.request_count.load(Ordering::Relaxed);
        let total_request_ms = self.total_request_ms.load(Ordering::Relaxed);
        ShimRwkvMetricsSnapshot {
            request_count,
            failed_request_count: self.failed_request_count.load(Ordering::Relaxed),
            total_request_ms,
            average_request_ms: if request_count > 0 {
                total_request_ms / request_count
            } else {
                0
            },
            max_request_ms: self.max_request_ms.load(Ordering::Relaxed),
            total_input_chars: self.total_input_chars.load(Ordering::Relaxed),
            total_output_chars: self.total_output_chars.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShimRwkvMetricsSnapshot {
    pub request_count: u64,
    pub failed_request_count: u64,
    pub total_request_ms: u64,
    pub average_request_ms: u64,
    pub max_request_ms: u64,
    pub total_input_chars: u64,
    pub total_output_chars: u64,
}

#[derive(Debug)]
pub struct OpenAiShim {
    port: u16,
    /// How many texts the shim will assemble into a single `/v1/batch/chat`
    /// call to the RWKV server. Tracks the server's reported `supported_batch_sizes`
    /// ceiling so MLX can run wider batches when there's enough demand.
    /// Currently informational — kept for diagnostics + future code paths that
    /// want to align downstream concurrency with the RWKV ceiling. Suppress
    /// the dead-code warning because the value flows through `mobile_batch_processor`
    /// at spawn time, not through field reads.
    #[allow(dead_code)]
    pub batch_size: usize,
    /// What to pass as pdf2zh's `--thread` argument. Capped lower than
    /// `batch_size` so pdf2zh's Python multiprocessing pool doesn't blow up
    /// on small inputs. See `PDF2ZH_THREAD_CEILING`.
    pub pdf2zh_thread_count: usize,
    /// RWKV request timing collected while this shim is alive. Snapshot it
    /// before dropping the shim to feed the diagnostics profile.
    pub metrics: Arc<ShimRwkvMetrics>,
    server_handle: JoinHandle<()>,
    batch_handle: JoinHandle<()>,
}

impl OpenAiShim {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }
}

impl Drop for OpenAiShim {
    fn drop(&mut self) {
        self.server_handle.abort();
        self.batch_handle.abort();
    }
}

struct PendingTranslation {
    text: String,
    result_tx: oneshot::Sender<Result<String, String>>,
}

#[derive(Debug, Clone, Copy)]
struct PdfChunkProfile {
    body: PdfChunkBudget,
    caption: PdfChunkBudget,
    reference: PdfChunkBudget,
}

struct ShimState {
    batch_tx: mpsc::Sender<PendingTranslation>,
    log_file: PathBuf,
    debug: bool,
    target_lang: String,
    chunk_profile: PdfChunkProfile,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: &'static str,
    model: &'static str,
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Serialize)]
struct ChatChoice {
    index: u32,
    message: ChatMessageResponse,
    finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct ChatMessageResponse {
    role: &'static str,
    content: String,
}

pub async fn spawn_shim(
    provider: ShimProviderConfig,
    source_lang: String,
    target_lang: String,
    log_file: PathBuf,
    debug: bool,
    debug_context: Option<String>,
) -> Result<OpenAiShim, String> {
    let metrics = Arc::new(ShimRwkvMetrics::default());
    let (max_batch_size, batch_handle, chunk_profile) = match provider {
        ShimProviderConfig::MobileBatch(rwkv) => {
            mobile_batch_chat::set_chat_roles_for_pair(&rwkv, &source_lang, &target_lang, None)
                .await?;
            let max_batch_size = mobile_batch_chat::query_supported_batch_sizes(&rwkv)
                .await
                .map(|sizes| mobile_batch_chat::pick_batch_size(&sizes, 0))
                .unwrap_or(DEFAULT_MAX_BATCH_SIZE)
                .max(1);
            let (batch_tx, batch_rx) = mpsc::channel(max_batch_size * 4);
            let handle = tokio::spawn(mobile_batch_processor(
                batch_rx,
                rwkv,
                source_lang.clone(),
                target_lang.clone(),
                max_batch_size,
                Arc::clone(&metrics),
                debug_context.clone(),
            ));
            (
                max_batch_size,
                (batch_tx, handle),
                standard_pdf_chunk_profile(),
            )
        }
        ShimProviderConfig::Lightning(lightning) => {
            let max_batch_size = DEFAULT_MAX_BATCH_SIZE;
            let (batch_tx, batch_rx) = mpsc::channel(max_batch_size * 4);
            let handle = tokio::spawn(lightning_batch_processor(
                batch_rx,
                lightning,
                source_lang.clone(),
                target_lang.clone(),
                max_batch_size,
                Arc::clone(&metrics),
                debug_context.clone(),
            ));
            (
                max_batch_size,
                (batch_tx, handle),
                standard_pdf_chunk_profile(),
            )
        }
        ShimProviderConfig::LlamaCpp(llama) => {
            let max_batch_size = llama_cpp_chat::managed_runtime_settings_from_env()
                .parallel_requests
                .max(1);
            let (batch_tx, batch_rx) = mpsc::channel(max_batch_size * 4);
            let handle = tokio::spawn(llama_cpp_batch_processor(
                batch_rx,
                llama,
                source_lang.clone(),
                target_lang.clone(),
                max_batch_size,
                Arc::clone(&metrics),
                debug_context.clone(),
            ));
            (
                max_batch_size,
                (batch_tx, handle),
                llama_cpp_pdf_chunk_profile(),
            )
        }
    };
    let (batch_tx, batch_handle) = (batch_handle.0, batch_handle.1);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("无法启动 OpenAI shim: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("无法读取 OpenAI shim 端口: {error}"))?
        .port();
    append_log(
        debug,
        &log_file,
        &format!(
            "spawn shim source_lang={} target_lang={} max_batch_size={} chunk_profile=body:{}/{} caption:{}/{} reference:{}/{}",
            source_lang,
            target_lang,
            max_batch_size,
            chunk_profile.body.target,
            chunk_profile.body.hard,
            chunk_profile.caption.target,
            chunk_profile.caption.hard,
            chunk_profile.reference.target,
            chunk_profile.reference.hard
        ),
    );
    let state = Arc::new(ShimState {
        batch_tx,
        log_file,
        debug,
        target_lang,
        chunk_profile,
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);
    let server_handle = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("[pdf2zh-shim] server exited: {error}");
        }
    });
    let pdf2zh_thread_count = max_batch_size.min(PDF2ZH_THREAD_CEILING).max(1);
    Ok(OpenAiShim {
        port,
        batch_size: max_batch_size,
        pdf2zh_thread_count,
        metrics,
        server_handle,
        batch_handle,
    })
}

async fn mobile_batch_processor(
    mut rx: mpsc::Receiver<PendingTranslation>,
    rwkv: MobileBatchChatConfig,
    source_lang: String,
    target_lang: String,
    max_batch_size: usize,
    metrics: Arc<ShimRwkvMetrics>,
    debug_context: Option<String>,
) {
    loop {
        let Some(first) = rx.recv().await else {
            break;
        };
        let mut batch = vec![first];

        let deadline = Instant::now() + Duration::from_millis(BATCH_WINDOW_MS);
        while batch.len() < max_batch_size {
            match timeout_at(deadline, rx.recv()).await {
                Ok(Some(item)) => batch.push(item),
                _ => break,
            }
        }

        eprintln!("[pdf2zh-batch] assembled {} item(s) in batch", batch.len());
        let source_texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();
        let request_started = Instant::now();
        let result = mobile_batch_chat::translate_batch(
            &rwkv,
            ProviderTranslateBatch {
                source_texts: &source_texts,
                source_lang: &source_lang,
                target_lang: &target_lang,
                timeout_ms: rwkv.timeout_ms,
                cancel: None,
                debug_context: debug_context.as_deref().or(Some("pdf2zh-shim")),
            },
        )
        .await;
        metrics.record(
            request_started.elapsed().as_millis() as u64,
            result.ok,
            source_texts.iter().map(|t| t.chars().count() as u64).sum(),
            result
                .translations
                .iter()
                .map(|t| t.chars().count() as u64)
                .sum(),
        );

        eprintln!(
            "[pdf2zh-batch] result: ok={}, translations={}",
            result.ok,
            result.translations.len()
        );
        if result.ok && result.translations.len() == batch.len() {
            for (pending, translation) in batch.into_iter().zip(result.translations) {
                let _ = pending.result_tx.send(Ok(translation));
            }
        } else {
            let error_msg = if result.ok {
                format!(
                    "翻译结果数量不匹配（期望 {}，实际 {}）",
                    batch.len(),
                    result.translations.len()
                )
            } else {
                result.message.clone()
            };
            for pending in batch {
                let _ = pending.result_tx.send(Err(error_msg.clone()));
            }
        }
    }
}

async fn lightning_batch_processor(
    mut rx: mpsc::Receiver<PendingTranslation>,
    config: LightningApiConfig,
    source_lang: String,
    target_lang: String,
    max_batch_size: usize,
    metrics: Arc<ShimRwkvMetrics>,
    debug_context: Option<String>,
) {
    loop {
        let Some(first) = rx.recv().await else {
            break;
        };
        let mut batch = vec![first];

        let deadline = Instant::now() + Duration::from_millis(BATCH_WINDOW_MS);
        while batch.len() < max_batch_size {
            match timeout_at(deadline, rx.recv()).await {
                Ok(Some(item)) => batch.push(item),
                _ => break,
            }
        }

        eprintln!(
            "[pdf2zh-lightning] assembled {} item(s) in batch",
            batch.len()
        );
        let source_texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();
        let request_started = Instant::now();
        let result = crate::rwkv_api::translate_batch_via_lightning(
            &config.base_url,
            &config.endpoint,
            &config.internal_token,
            &config.body_password,
            config.timeout_ms,
            &source_lang,
            &target_lang,
            &source_texts,
            debug_context.as_deref().or(Some("pdf2zh-shim")),
        )
        .await;
        metrics.record(
            request_started.elapsed().as_millis() as u64,
            result.is_ok(),
            source_texts.iter().map(|t| t.chars().count() as u64).sum(),
            result
                .as_ref()
                .map(|translations| translations.iter().map(|t| t.chars().count() as u64).sum())
                .unwrap_or(0),
        );
        match result {
            Ok(translations) if translations.len() == batch.len() => {
                for (pending, translation) in batch.into_iter().zip(translations) {
                    let _ = pending.result_tx.send(Ok(translation));
                }
            }
            Ok(translations) => {
                let error_msg = format!(
                    "翻译结果数量不匹配（期望 {}，实际 {}）",
                    batch.len(),
                    translations.len()
                );
                for pending in batch {
                    let _ = pending.result_tx.send(Err(error_msg.clone()));
                }
            }
            Err(message) => {
                for pending in batch {
                    let _ = pending.result_tx.send(Err(message.clone()));
                }
            }
        }
    }
}

async fn llama_cpp_batch_processor(
    mut rx: mpsc::Receiver<PendingTranslation>,
    config: LlamaCppApiConfig,
    source_lang: String,
    target_lang: String,
    max_batch_size: usize,
    metrics: Arc<ShimRwkvMetrics>,
    debug_context: Option<String>,
) {
    loop {
        let Some(first) = rx.recv().await else {
            break;
        };
        let mut batch = vec![first];

        let deadline = Instant::now() + Duration::from_millis(BATCH_WINDOW_MS);
        while batch.len() < max_batch_size {
            match timeout_at(deadline, rx.recv()).await {
                Ok(Some(item)) => batch.push(item),
                _ => break,
            }
        }

        eprintln!(
            "[pdf2zh-llama-cpp] assembled {} item(s) in batch",
            batch.len()
        );
        let source_texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();
        let request_started = Instant::now();
        let result = translate_llama_cpp_batch_with_backstop(
            &config,
            &source_lang,
            &target_lang,
            &source_texts,
            debug_context.as_deref().or(Some("pdf2zh-shim")),
        )
        .await;
        metrics.record(
            request_started.elapsed().as_millis() as u64,
            result.is_ok(),
            source_texts.iter().map(|t| t.chars().count() as u64).sum(),
            result
                .as_ref()
                .map(|translations| translations.iter().map(|t| t.chars().count() as u64).sum())
                .unwrap_or(0),
        );
        match result {
            Ok(translations) if translations.len() == batch.len() => {
                for (pending, translation) in batch.into_iter().zip(translations) {
                    let _ = pending.result_tx.send(Ok(translation));
                }
            }
            Ok(translations) => {
                let error_msg = format!(
                    "翻译结果数量不匹配（期望 {}，实际 {}）",
                    batch.len(),
                    translations.len()
                );
                for pending in batch {
                    let _ = pending.result_tx.send(Err(error_msg.clone()));
                }
            }
            Err(message) => {
                for pending in batch {
                    let _ = pending.result_tx.send(Err(message.clone()));
                }
            }
        }
    }
}

async fn translate_llama_cpp_batch_with_backstop(
    config: &LlamaCppApiConfig,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    debug_context: Option<&str>,
) -> Result<Vec<String>, String> {
    let result = request_llama_cpp_translations(
        config,
        source_lang,
        target_lang,
        source_texts,
        debug_context,
    )
    .await;
    match result {
        Ok(translations) => Ok(translations),
        Err(first_error) => {
            eprintln!(
                "[pdf2zh-llama-cpp] batch failed, retrying {} item(s) with split backstop: {}",
                source_texts.len(),
                first_error
            );
            let mut translations = Vec::with_capacity(source_texts.len());
            for source in source_texts {
                translations.push(
                    translate_llama_cpp_text_with_backstop(
                        config,
                        source_lang,
                        target_lang,
                        source,
                        debug_context,
                    )
                    .await
                    .map_err(|retry_error| {
                        format!("{first_error}; split retry failed: {retry_error}")
                    })?,
                );
            }
            Ok(translations)
        }
    }
}

async fn translate_llama_cpp_text_with_backstop(
    config: &LlamaCppApiConfig,
    source_lang: &str,
    target_lang: &str,
    source_text: &str,
    debug_context: Option<&str>,
) -> Result<String, String> {
    let retry_chunks = split_pdf_shim_text_for_retry(source_text, 0);
    if retry_chunks.len() > 1 {
        eprintln!(
            "[pdf2zh-llama-cpp] retry split item into {} chunk(s)",
            retry_chunks.len()
        );
        match request_llama_cpp_translations(
            config,
            source_lang,
            target_lang,
            &retry_chunks,
            debug_context,
        )
        .await
        {
            Ok(translations) => return Ok(join_translated_chunks(translations, target_lang)),
            Err(error) => {
                eprintln!(
                    "[pdf2zh-llama-cpp] retry split batch failed, using final serial split: {error}"
                );
            }
        }

        let final_chunks = split_pdf_shim_text_for_retry(source_text, 1);
        if final_chunks.len() > retry_chunks.len() {
            let translations = request_llama_cpp_translations_serial(
                config,
                source_lang,
                target_lang,
                &final_chunks,
                debug_context,
            )
            .await?;
            return Ok(join_translated_chunks(translations, target_lang));
        }
    }

    let single = vec![source_text.to_string()];
    let translations =
        request_llama_cpp_translations(config, source_lang, target_lang, &single, debug_context)
            .await?;
    translations
        .into_iter()
        .next()
        .ok_or_else(|| "llama.cpp split retry returned no translation".to_string())
}

async fn request_llama_cpp_translations_serial(
    config: &LlamaCppApiConfig,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    debug_context: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut translations = Vec::with_capacity(source_texts.len());
    for source_text in source_texts {
        let single = vec![source_text.clone()];
        let mut result = request_llama_cpp_translations(
            config,
            source_lang,
            target_lang,
            &single,
            debug_context,
        )
        .await?;
        translations.push(
            result.pop().ok_or_else(|| {
                "llama.cpp serial split retry returned no translation".to_string()
            })?,
        );
    }
    Ok(translations)
}

async fn request_llama_cpp_translations(
    config: &LlamaCppApiConfig,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    debug_context: Option<&str>,
) -> Result<Vec<String>, String> {
    let translations = crate::rwkv_api::translate_batch_via_llama_cpp(
        &config.base_url,
        config.timeout_ms,
        source_lang,
        target_lang,
        source_texts,
        None,
        debug_context,
    )
    .await?;
    if translations.len() == source_texts.len() {
        Ok(translations)
    } else {
        Err(format!(
            "翻译结果数量不匹配（期望 {}，实际 {}）",
            source_texts.len(),
            translations.len()
        ))
    }
}

async fn chat_completions(
    State(state): State<Arc<ShimState>>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, (StatusCode, String)> {
    let text = extract_text(&request.messages).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "OpenAI shim 请求里没有可翻译文本。".to_string(),
        )
    })?;
    let raw_user = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.as_str())
        .unwrap_or("");
    append_log(
        state.debug,
        &state.log_file,
        &format!(
            "request messages={} raw_user_preview={} extracted_preview={}",
            request.messages.len(),
            preview(raw_user),
            preview(&text),
        ),
    );
    if is_pdf2zh_placeholder_only(&text) {
        append_log(
            state.debug,
            &state.log_file,
            &format!("placeholder passthrough={}", preview(&text)),
        );
        return Ok(openai_response(text));
    }
    if should_passthrough_pdf_reference_fragment(&text) {
        append_log(
            state.debug,
            &state.log_file,
            &format!("reference fragment passthrough={}", preview(&text)),
        );
        return Ok(openai_response(text));
    }
    if text.trim().is_empty() {
        append_log(state.debug, &state.log_file, "empty source passthrough");
        return Ok(Json(ChatCompletionResponse {
            id: "chatcmpl-rosetta-pdf2zh".to_string(),
            object: "chat.completion",
            model: "rwkv",
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessageResponse {
                    role: "assistant",
                    content: String::new(),
                },
                finish_reason: "stop",
            }],
        }));
    }

    let chunks = split_pdf_shim_text_for_profile(&text, state.chunk_profile);
    if chunks.len() > 1 {
        let chunk_tokens = chunks
            .iter()
            .map(|chunk| estimate_prompt_tokens(chunk))
            .collect::<Vec<_>>();
        let max_chunk_tokens = chunk_tokens.iter().copied().max().unwrap_or(0);
        let avg_chunk_tokens = chunk_tokens.iter().sum::<usize>() / chunk_tokens.len().max(1);
        append_log(
            state.debug,
            &state.log_file,
            &format!(
                "split long request into {} chunk(s), estimated_tokens={}, avg_chunk_tokens={}, max_chunk_tokens={}",
                chunks.len(),
                estimate_prompt_tokens(&text),
                avg_chunk_tokens,
                max_chunk_tokens
            ),
        );
    }

    let content = translate_chunks(&state, chunks)
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, format!("RWKV 翻译失败: {error}")))?;

    append_log(
        state.debug,
        &state.log_file,
        &format!("translation_preview={}", preview(&content)),
    );
    Ok(openai_response(content))
}

async fn translate_chunks(state: &Arc<ShimState>, chunks: Vec<String>) -> Result<String, String> {
    let mut receivers = Vec::with_capacity(chunks.len());
    for text in chunks {
        let (result_tx, result_rx) = oneshot::channel();
        state
            .batch_tx
            .send(PendingTranslation { text, result_tx })
            .await
            .map_err(|_| "翻译批处理队列已关闭。".to_string())?;
        receivers.push(result_rx);
    }

    let mut translations = Vec::with_capacity(receivers.len());
    for receiver in receivers {
        let translation = receiver
            .await
            .map_err(|_| "批处理结果接收失败。".to_string())??;
        translations.push(translation);
    }

    Ok(join_translated_chunks(translations, &state.target_lang))
}

fn openai_response(content: String) -> Json<ChatCompletionResponse> {
    Json(ChatCompletionResponse {
        id: "chatcmpl-rosetta-pdf2zh".to_string(),
        object: "chat.completion",
        model: "rwkv",
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessageResponse {
                role: "assistant",
                content,
            },
            finish_reason: "stop",
        }],
    })
}

fn extract_text(messages: &[ChatMessage]) -> Option<String> {
    let content = messages
        .iter()
        .rev()
        .find(|message| message.role == "user" && !message.content.trim().is_empty())
        .or_else(|| {
            messages
                .iter()
                .rev()
                .find(|message| !message.content.trim().is_empty())
        })
        .map(|message| message.content.trim().to_string())?;
    Some(extract_pdf2zh_source_text(&content).unwrap_or(content))
}

fn extract_pdf2zh_source_text(content: &str) -> Option<String> {
    let lower = content.to_ascii_lowercase();
    let source_marker = "source text:";
    let translated_marker = "translated text:";
    let source_start = lower.find(source_marker)? + source_marker.len();
    let after_source = &content[source_start..];
    let after_source_lower = &lower[source_start..];
    let source_end = after_source_lower
        .find(translated_marker)
        .unwrap_or(after_source.len());
    Some(after_source[..source_end].trim().to_string())
}

#[cfg(test)]
fn split_pdf_shim_text(text: &str) -> Vec<String> {
    split_pdf_shim_text_for_profile(text, standard_pdf_chunk_profile())
}

fn split_pdf_shim_text_for_profile(text: &str, profile: PdfChunkProfile) -> Vec<String> {
    let normalized = normalize_pdf_source_text(text);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return vec![trimmed.to_string()];
    }

    let budget = pdf_chunk_budget_for_text(trimmed, profile);
    if estimate_prompt_tokens(trimmed) <= budget.hard {
        return vec![trimmed.to_string()];
    }

    let units = split_pdf_semantic_units(trimmed);
    let mut chunks = Vec::new();
    let mut current = String::new();
    for unit in units {
        push_pdf_text_unit(&mut chunks, &mut current, &unit, budget, profile);
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
        .into_iter()
        .flat_map(|chunk| {
            let chunk_budget =
                min_pdf_chunk_budget(budget, pdf_chunk_budget_for_text(&chunk, profile));
            split_oversized_pdf_chunk(chunk, chunk_budget)
        })
        .filter(|chunk| !chunk.trim().is_empty())
        .collect()
}

fn split_pdf_shim_text_for_retry(text: &str, retry_index: usize) -> Vec<String> {
    let budget = if retry_index == 0 {
        PdfChunkBudget {
            target: PDF_SHIM_RETRY_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_RETRY_HARD_PROMPT_TOKENS,
        }
    } else {
        PdfChunkBudget {
            target: PDF_SHIM_FINAL_RETRY_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_FINAL_RETRY_HARD_PROMPT_TOKENS,
        }
    };
    split_pdf_shim_text_for_profile(
        text,
        PdfChunkProfile {
            body: budget,
            caption: budget,
            reference: budget,
        },
    )
}

fn push_pdf_text_unit(
    chunks: &mut Vec<String>,
    current: &mut String,
    unit: &str,
    parent_budget: PdfChunkBudget,
    profile: PdfChunkProfile,
) {
    let unit = unit.trim();
    if unit.is_empty() {
        return;
    }
    let budget = min_pdf_chunk_budget(parent_budget, pdf_chunk_budget_for_text(unit, profile));
    if estimate_prompt_tokens(unit) > budget.hard {
        let sub_units = split_pdf_sentences(unit);
        if sub_units.len() > 1 {
            for sub_unit in sub_units {
                push_pdf_text_unit(chunks, current, &sub_unit, budget, profile);
            }
            return;
        }
        if !current.trim().is_empty() {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        chunks.extend(split_oversized_pdf_chunk(unit.to_string(), budget));
        return;
    }
    let candidate = if current.is_empty() {
        unit.to_string()
    } else {
        format!("{} {}", current.trim(), unit)
    };
    if !current.is_empty() && estimate_prompt_tokens(&candidate) > budget.target {
        chunks.push(current.trim().to_string());
        current.clear();
    }
    if !current.is_empty() {
        current.push(' ');
    }
    current.push_str(unit);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PdfChunkBudget {
    target: usize,
    hard: usize,
}

fn standard_pdf_chunk_profile() -> PdfChunkProfile {
    PdfChunkProfile {
        body: PdfChunkBudget {
            target: PDF_SHIM_BODY_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_BODY_HARD_PROMPT_TOKENS,
        },
        caption: PdfChunkBudget {
            target: PDF_SHIM_CAPTION_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_CAPTION_HARD_PROMPT_TOKENS,
        },
        reference: PdfChunkBudget {
            target: PDF_SHIM_REFERENCE_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_REFERENCE_HARD_PROMPT_TOKENS,
        },
    }
}

fn llama_cpp_pdf_chunk_profile() -> PdfChunkProfile {
    let settings = llama_cpp_chat::managed_runtime_settings_from_env();
    let effective_slot_context = settings.server_ctx_size / settings.parallel_requests.max(1);
    apply_llama_cpp_pdf_chunk_env_overrides(llama_cpp_pdf_chunk_profile_for_effective_context(
        effective_slot_context,
    ))
}

fn llama_cpp_pdf_chunk_profile_for_effective_context(
    effective_slot_context: usize,
) -> PdfChunkProfile {
    if effective_slot_context >= PDF_SHIM_LLAMA_WIDE_SLOT_CONTEXT_TOKENS {
        return wide_llama_cpp_pdf_chunk_profile();
    }
    conservative_llama_cpp_pdf_chunk_profile()
}

fn conservative_llama_cpp_pdf_chunk_profile() -> PdfChunkProfile {
    PdfChunkProfile {
        body: PdfChunkBudget {
            target: PDF_SHIM_LLAMA_BODY_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_LLAMA_BODY_HARD_PROMPT_TOKENS,
        },
        caption: PdfChunkBudget {
            target: PDF_SHIM_LLAMA_CAPTION_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_LLAMA_CAPTION_HARD_PROMPT_TOKENS,
        },
        reference: PdfChunkBudget {
            target: PDF_SHIM_LLAMA_REFERENCE_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_LLAMA_REFERENCE_HARD_PROMPT_TOKENS,
        },
    }
}

fn wide_llama_cpp_pdf_chunk_profile() -> PdfChunkProfile {
    PdfChunkProfile {
        body: PdfChunkBudget {
            target: PDF_SHIM_LLAMA_WIDE_BODY_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_LLAMA_WIDE_BODY_HARD_PROMPT_TOKENS,
        },
        caption: PdfChunkBudget {
            target: PDF_SHIM_LLAMA_WIDE_CAPTION_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_LLAMA_WIDE_CAPTION_HARD_PROMPT_TOKENS,
        },
        reference: PdfChunkBudget {
            target: PDF_SHIM_LLAMA_WIDE_REFERENCE_TARGET_PROMPT_TOKENS,
            hard: PDF_SHIM_LLAMA_WIDE_REFERENCE_HARD_PROMPT_TOKENS,
        },
    }
}

fn apply_llama_cpp_pdf_chunk_env_overrides(profile: PdfChunkProfile) -> PdfChunkProfile {
    PdfChunkProfile {
        body: pdf_chunk_budget_from_env(
            profile.body,
            PDF_SHIM_LLAMA_BODY_TARGET_ENV,
            PDF_SHIM_LLAMA_BODY_HARD_ENV,
        ),
        caption: pdf_chunk_budget_from_env(
            profile.caption,
            PDF_SHIM_LLAMA_CAPTION_TARGET_ENV,
            PDF_SHIM_LLAMA_CAPTION_HARD_ENV,
        ),
        reference: pdf_chunk_budget_from_env(
            profile.reference,
            PDF_SHIM_LLAMA_REFERENCE_TARGET_ENV,
            PDF_SHIM_LLAMA_REFERENCE_HARD_ENV,
        ),
    }
}

fn pdf_chunk_budget_from_env(
    fallback: PdfChunkBudget,
    target_env: &str,
    hard_env: &str,
) -> PdfChunkBudget {
    let target = positive_usize_env(target_env).unwrap_or(fallback.target);
    let hard = positive_usize_env(hard_env).unwrap_or(fallback.hard);
    PdfChunkBudget {
        target,
        hard: hard.max(target),
    }
}

fn positive_usize_env(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .as_deref()
        .and_then(parse_positive_usize)
}

fn parse_positive_usize(raw: &str) -> Option<usize> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<usize>().ok().filter(|value| *value > 0)
}

fn pdf_chunk_budget_for_text(text: &str, profile: PdfChunkProfile) -> PdfChunkBudget {
    if is_reference_item(text) {
        return profile.reference;
    }
    if starts_with_caption_label(text) {
        return profile.caption;
    }
    profile.body
}

fn min_pdf_chunk_budget(left: PdfChunkBudget, right: PdfChunkBudget) -> PdfChunkBudget {
    PdfChunkBudget {
        target: left.target.min(right.target),
        hard: left.hard.min(right.hard),
    }
}

fn split_oversized_pdf_chunk(chunk: String, budget: PdfChunkBudget) -> Vec<String> {
    if estimate_prompt_tokens(&chunk) <= budget.target {
        return vec![chunk];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in chunk.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", current, word)
        };
        if !current.is_empty() && estimate_prompt_tokens(&candidate) > budget.target {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        if estimate_prompt_tokens(word) > budget.target {
            if !current.trim().is_empty() {
                chunks.push(current.trim().to_string());
                current.clear();
            }
            chunks.extend(hard_split_pdf_token(word, budget));
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

fn hard_split_pdf_token(token: &str, budget: PdfChunkBudget) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in token.chars() {
        let candidate = format!("{current}{ch}");
        if !current.is_empty() && estimate_prompt_tokens(&candidate) > budget.target {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn normalize_pdf_source_text(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '-' {
            let mut next = index + 1;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            if index > 0
                && next < chars.len()
                && chars[index - 1].is_ascii_alphabetic()
                && chars[next].is_ascii_lowercase()
            {
                index = next;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    collapse_pdf_whitespace(&out)
}

fn collapse_pdf_whitespace(text: &str) -> String {
    let mut out = String::new();
    let mut last_was_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

fn split_pdf_semantic_units(text: &str) -> Vec<String> {
    let reference_units = split_reference_items(text);
    if reference_units.len() > 1 {
        return reference_units;
    }

    let sentence_units = split_pdf_sentences(text);
    merge_caption_labels(sentence_units)
}

fn split_reference_items(text: &str) -> Vec<String> {
    let refs = reference_item_starts(text);
    if refs.len() < 2 {
        return Vec::new();
    }

    refs.iter()
        .enumerate()
        .filter_map(|(index, &start)| {
            let end = refs.get(index + 1).copied().unwrap_or(text.len());
            let item = text[start..end].trim();
            (!item.is_empty()).then(|| item.to_string())
        })
        .collect()
}

fn reference_item_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let at_boundary = index == 0
            || text[..index]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_whitespace() || ch == '.');
        if !at_boundary {
            index += 1;
            continue;
        }
        let mut cursor = index + 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor > index + 1 && cursor < bytes.len() && bytes[cursor] == b']' {
            starts.push(index);
            index = cursor + 1;
        } else {
            index += 1;
        }
    }
    starts
}

fn is_reference_item(text: &str) -> bool {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return false;
    };
    let Some(close_index) = rest.find(']') else {
        return false;
    };
    close_index > 0 && close_index <= 4 && rest[..close_index].chars().all(|ch| ch.is_ascii_digit())
}

fn split_pdf_sentences(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let end = index + ch.len_utf8();
        if ch == '\n' || is_sentence_boundary(text, index, ch) {
            if start < end {
                units.push(text[start..end].trim().to_string());
            }
            start = end;
        }
    }
    if start < text.len() {
        units.push(text[start..].trim().to_string());
    }
    units.into_iter().filter(|unit| !unit.is_empty()).collect()
}

fn merge_caption_labels(units: Vec<String>) -> Vec<String> {
    let mut merged = Vec::new();
    let mut index = 0;
    while index < units.len() {
        if index + 1 < units.len() && is_caption_label(&units[index]) {
            merged.push(format!(
                "{} {}",
                units[index].trim(),
                units[index + 1].trim()
            ));
            index += 2;
        } else {
            merged.push(units[index].clone());
            index += 1;
        }
    }
    merged
}

fn is_caption_label(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    let Some(prefix) = lower.strip_suffix('.') else {
        return false;
    };
    let mut parts = prefix.split_whitespace();
    let Some(label) = parts.next() else {
        return false;
    };
    let Some(number) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && matches!(label, "fig" | "fig." | "figure" | "table" | "tab" | "tab.")
        && number.chars().any(|ch| ch.is_ascii_digit())
}

fn starts_with_caption_label(text: &str) -> bool {
    let trimmed = text.trim_start();
    let mut parts = trimmed.split_whitespace();
    let Some(label) = parts.next() else {
        return false;
    };
    let label = label
        .trim_matches(|ch: char| ch == '(' || ch == '[')
        .to_ascii_lowercase();
    if !matches!(
        label.as_str(),
        "fig" | "fig." | "figure" | "table" | "tab" | "tab."
    ) {
        return false;
    }
    let Some(number) = parts.next() else {
        return false;
    };
    number
        .trim_end_matches(|ch: char| ch == '.' || ch == ':' || ch == ')')
        .chars()
        .any(|ch| ch.is_ascii_digit())
}

fn is_sentence_boundary(text: &str, index: usize, ch: char) -> bool {
    if matches!(ch, '。' | '？' | '！' | '；') {
        return true;
    }
    if !matches!(ch, '.' | '?' | '!' | ';') {
        return false;
    }
    let prev = text[..index].chars().next_back();
    let next = text[index + ch.len_utf8()..].chars().next();
    if ch == '.'
        && prev.is_some_and(|c| c.is_ascii_digit())
        && next.is_some_and(|c| c.is_ascii_digit())
    {
        return false;
    }
    if ch == '.' && ends_with_known_abbreviation(&text[..=index]) {
        return false;
    }
    next.is_none_or(char::is_whitespace)
}

fn ends_with_known_abbreviation(text: &str) -> bool {
    let lower = text.trim_end().to_ascii_lowercase();
    let token = lower
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_matches(|ch: char| ch == '(' || ch == '[' || ch == '"');
    matches!(
        token,
        "fig."
            | "tab."
            | "eq."
            | "sec."
            | "ch."
            | "no."
            | "dr."
            | "prof."
            | "mr."
            | "mrs."
            | "ms."
            | "vs."
            | "e.g."
            | "i.e."
            | "etc."
            | "al."
    )
}

fn estimate_prompt_tokens(text: &str) -> usize {
    // Conservative estimate for the tiny-context llama.cpp profile: CJK
    // characters often behave close to one token each, while English prose is
    // roughly four characters per token. Add room for language labels.
    let mut units = 12.0f32;
    for ch in text.chars() {
        units += if ch.is_ascii_whitespace() {
            0.1
        } else if ch.is_ascii_alphanumeric() {
            0.25
        } else if ch.is_ascii() {
            0.35
        } else {
            1.0
        };
    }
    units.ceil() as usize
}

fn join_translated_chunks(translations: Vec<String>, target_lang: &str) -> String {
    let separator = if is_compact_target_lang(target_lang) {
        ""
    } else {
        " "
    };
    translations
        .into_iter()
        .map(|translation| translation.trim().to_string())
        .filter(|translation| !translation.is_empty())
        .collect::<Vec<_>>()
        .join(separator)
}

fn is_compact_target_lang(target_lang: &str) -> bool {
    let normalized = target_lang.trim().to_ascii_lowercase();
    normalized == "zh"
        || normalized.starts_with("zh-")
        || normalized == "ja"
        || normalized.starts_with("ja-")
        || normalized == "ko"
        || normalized.starts_with("ko-")
}

fn preview(text: &str) -> String {
    text.chars()
        .take(400)
        .collect::<String>()
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn is_pdf2zh_placeholder_only(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let mut saw_placeholder = false;
    for token in trimmed.split_whitespace() {
        let token = token.trim_matches(|ch: char| {
            matches!(ch, ',' | '.' | ';' | ':' | '，' | '。' | '；' | '：')
        });
        let Some(inner) = token
            .strip_prefix("$v")
            .and_then(|value| value.strip_suffix('$'))
        else {
            return false;
        };
        if inner.is_empty() || !inner.chars().all(|ch| ch.is_ascii_digit()) {
            return false;
        }
        saw_placeholder = true;
    }
    saw_placeholder
}

fn should_passthrough_pdf_reference_fragment(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() > PDF_SHIM_REFERENCE_PASSTHROUGH_MAX_CHARS {
        return false;
    }
    if !is_reference_item(trimmed) {
        return false;
    }
    let words = trimmed.split_whitespace().count();
    words <= 6 && trimmed.chars().any(|ch| ch.is_ascii_digit())
}

fn append_log(enabled: bool, path: &PathBuf, line: &str) {
    if !enabled {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = format!("{}\n", line);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(line.as_bytes())
        });
    eprintln!("[pdf2zh-shim] {line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_prefers_last_user_message() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "rules".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "first".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "ignored".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: " second ".to_string(),
            },
        ];
        assert_eq!(extract_text(&messages).as_deref(), Some("second"));
    }

    #[test]
    fn extract_text_unwraps_pdf2zh_prompt() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "Translate the following markdown source text to zh-CN. Keep the formula notation $v*$ unchanged. Output translation directly without any additional text.\nSource Text: Beautiful is better than ugly.\nTranslated Text:".to_string(),
        }];
        assert_eq!(
            extract_text(&messages).as_deref(),
            Some("Beautiful is better than ugly.")
        );
    }

    #[test]
    fn extract_text_keeps_empty_pdf2zh_source_empty() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "Translate the following markdown source text to zh.\nSource Text:    \nTranslated Text:".to_string(),
        }];
        assert_eq!(extract_text(&messages).as_deref(), Some(""));
    }

    #[test]
    fn placeholder_only_text_is_not_sent_to_model() {
        assert!(is_pdf2zh_placeholder_only("$v0$"));
        assert!(is_pdf2zh_placeholder_only("$v0$ $v12$"));
        assert!(!is_pdf2zh_placeholder_only("Value is $v0$"));
    }

    #[test]
    fn short_reference_fragments_are_preserved_without_model_translation() {
        assert!(should_passthrough_pdf_reference_fragment(
            "[41] Nature 2024."
        ));
        assert!(should_passthrough_pdf_reference_fragment(
            "[7] Proc. CVPR 2025."
        ));
        assert!(!should_passthrough_pdf_reference_fragment(
            "[41] This reference title contains enough prose that it should still go through the translator."
        ));
        assert!(!should_passthrough_pdf_reference_fragment(
            "Value is [41] Nature 2024."
        ));
    }

    #[test]
    fn long_pdf_text_is_split_before_reaching_tiny_context_provider() {
        let text = "$v25$ Record quarterly revenue of $15.2 million, a 46% increase compared to the prior quarter and up 17x from the same period last year, driven by increased production volumes. Factory shipments increased 122% quarter-over-quarter, with 50% of the production volume delivered for a strategic customer project. $v26$ Gross loss of $31.0 million, a 32-point margin improvement from the prior quarter, driven by increased production volumes and operational efficiencies partially offset by one-time lower than average selling prices. $v27$ Operating expenses totaled $32.9 million, a decrease from prior quarter excluding $5.4 million in one-time non-recurring items. $v28$ $222.9 million net loss attributable to shareholders driven by $151.8 million non-cash changes in fair value tied to mark-to-market adjustments related to the Company’s increased stock price as of June 30, 2025, loss recorded for the repurchase of the Company’s outstanding 2026 convertible notes, and loss recorded as part of the prepayment under the Delayed Draw Term Loan.";

        let chunks = split_pdf_shim_text(text);

        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| estimate_prompt_tokens(chunk) <= PDF_SHIM_BODY_TARGET_PROMPT_TOKENS));
        assert!(chunks.concat().contains("Record quarterly revenue"));
    }

    #[test]
    fn medium_pdf_paragraph_stays_whole_for_speed_first_chunking() {
        let text = "For visual feature extraction, we replaced the RWKV module with a purely convolutional counterpart, as specified in config (I) of Table 3. The results clearly indicate a significant performance decline with the convolutional architecture, underscoring the limitations of traditional convolution module in visual feature modeling. This also highlights the effectiveness of RWKV in capturing longrange dependencies, contributing to more accurate pest recognition.";

        assert_eq!(split_pdf_shim_text(text), vec![text]);
    }

    #[test]
    fn llama_cpp_pdf_profile_splits_medium_text_for_output_room() {
        let text = "For visual feature extraction, we replaced the RWKV module with a purely convolutional counterpart, as specified in config (I) of Table 3. The results clearly indicate a significant performance decline with the convolutional architecture, underscoring the limitations of traditional convolution module in visual feature modeling. This also highlights the effectiveness of RWKV in capturing longrange dependencies, contributing to more accurate pest recognition.";

        let chunks =
            split_pdf_shim_text_for_profile(text, conservative_llama_cpp_pdf_chunk_profile());

        assert!(chunks.len() > 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| estimate_prompt_tokens(chunk)
                    <= PDF_SHIM_LLAMA_BODY_TARGET_PROMPT_TOKENS)
        );
    }

    #[test]
    fn llama_cpp_pdf_profile_uses_conservative_budget_for_512_token_slots() {
        let profile = llama_cpp_pdf_chunk_profile_for_effective_context(512);

        assert_eq!(
            profile.body,
            PdfChunkBudget {
                target: PDF_SHIM_LLAMA_BODY_TARGET_PROMPT_TOKENS,
                hard: PDF_SHIM_LLAMA_BODY_HARD_PROMPT_TOKENS,
            }
        );
        assert_eq!(
            profile.reference,
            PdfChunkBudget {
                target: PDF_SHIM_LLAMA_REFERENCE_TARGET_PROMPT_TOKENS,
                hard: PDF_SHIM_LLAMA_REFERENCE_HARD_PROMPT_TOKENS,
            }
        );
    }

    #[test]
    fn llama_cpp_pdf_profile_uses_wider_budget_for_1024_token_slots() {
        let profile = llama_cpp_pdf_chunk_profile_for_effective_context(1024);

        assert_eq!(
            profile.body,
            PdfChunkBudget {
                target: PDF_SHIM_LLAMA_WIDE_BODY_TARGET_PROMPT_TOKENS,
                hard: PDF_SHIM_LLAMA_WIDE_BODY_HARD_PROMPT_TOKENS,
            }
        );
        assert_eq!(
            profile.caption,
            PdfChunkBudget {
                target: PDF_SHIM_LLAMA_WIDE_CAPTION_TARGET_PROMPT_TOKENS,
                hard: PDF_SHIM_LLAMA_WIDE_CAPTION_HARD_PROMPT_TOKENS,
            }
        );
        assert_eq!(
            profile.reference,
            PdfChunkBudget {
                target: PDF_SHIM_LLAMA_WIDE_REFERENCE_TARGET_PROMPT_TOKENS,
                hard: PDF_SHIM_LLAMA_WIDE_REFERENCE_HARD_PROMPT_TOKENS,
            }
        );
        assert_eq!(
            profile.reference,
            PdfChunkBudget {
                target: PDF_SHIM_LLAMA_REFERENCE_TARGET_PROMPT_TOKENS,
                hard: PDF_SHIM_LLAMA_REFERENCE_HARD_PROMPT_TOKENS,
            }
        );
    }

    #[test]
    fn wider_llama_cpp_profile_merges_medium_text_for_fewer_completions() {
        let text = "For visual feature extraction, we replaced the RWKV module with a purely convolutional counterpart, as specified in config (I) of Table 3. The results clearly indicate a significant performance decline with the convolutional architecture, underscoring the limitations of traditional convolution module in visual feature modeling. This also highlights the effectiveness of RWKV in capturing longrange dependencies, contributing to more accurate pest recognition.";

        let conservative_chunks =
            split_pdf_shim_text_for_profile(text, conservative_llama_cpp_pdf_chunk_profile());
        let wide_chunks = split_pdf_shim_text_for_profile(text, wide_llama_cpp_pdf_chunk_profile());

        assert!(conservative_chunks.len() > wide_chunks.len());
        assert!(wide_chunks
            .iter()
            .all(|chunk| estimate_prompt_tokens(chunk)
                <= PDF_SHIM_LLAMA_WIDE_BODY_HARD_PROMPT_TOKENS));
    }

    #[test]
    fn positive_usize_parser_rejects_empty_zero_and_invalid_values() {
        assert_eq!(parse_positive_usize("112"), Some(112));
        assert_eq!(parse_positive_usize(" 84 "), Some(84));
        assert_eq!(parse_positive_usize(""), None);
        assert_eq!(parse_positive_usize("0"), None);
        assert_eq!(parse_positive_usize("-1"), None);
        assert_eq!(parse_positive_usize("wide"), None);
    }

    #[test]
    fn retry_split_uses_smaller_backstop_budget() {
        let text = "The results clearly indicate a significant performance decline with the convolutional architecture, underscoring the limitations of traditional convolution module in visual feature modeling and document translation workloads. This also highlights the effectiveness of RWKV in capturing longrange dependencies, contributing to more accurate pest recognition.";

        let chunks = split_pdf_shim_text_for_retry(text, 0);

        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| estimate_prompt_tokens(chunk) <= PDF_SHIM_RETRY_TARGET_PROMPT_TOKENS));
    }

    #[test]
    fn pdf_sentence_chunker_repairs_soft_hyphenated_line_breaks() {
        let chunks = split_pdf_shim_text(
            "In recent years, deep learn- ing has accelerated smart agri- culture applications.",
        );

        assert_eq!(
            chunks,
            vec!["In recent years, deep learning has accelerated smart agriculture applications."]
        );
    }

    #[test]
    fn pdf_sentence_chunker_keeps_caption_label_with_sentence() {
        let units = split_pdf_semantic_units(
            "Figure 1. The intricate morphology and texture of pests present challenges. Next sentence.",
        );

        assert_eq!(
            units[0],
            "Figure 1. The intricate morphology and texture of pests present challenges."
        );
        assert_eq!(units[1], "Next sentence.");
    }

    #[test]
    fn pdf_sentence_chunker_does_not_split_common_paper_abbreviations() {
        let units = split_pdf_semantic_units(
            "Smith et al. propose a compact model. Fig. 2 shows the result. It works.",
        );

        assert_eq!(units[0], "Smith et al. propose a compact model.");
        assert_eq!(units[1], "Fig. 2 shows the result.");
        assert_eq!(units[2], "It works.");
    }

    #[test]
    fn pdf_reference_block_splits_by_citation_items() {
        let units = split_pdf_semantic_units(
            "[26] Haiyun Liu et al. Plant diseases detection. 2024. [27] Jun Liu et al. Pest detection based on deep learning. 2021. [28] Yue Liu et al. Vmamba. 2024.",
        );

        assert_eq!(units.len(), 3);
        assert!(units[0].starts_with("[26]"));
        assert!(units[1].starts_with("[27]"));
        assert!(units[2].starts_with("[28]"));
    }

    #[test]
    fn medium_pdf_reference_stays_whole_for_speed_first_chunking() {
        let text = "[33] Mireille Gloria, Alice Researcher, Bob Scientist, Carol Author, Daniel Engineer, Elena Analyst, Frank Reviewer, Grace Writer, and Hannah Curator. A long citation title about visual pest detection, remote sensing, feature extraction, convolutional networks, recurrent models, agricultural benchmarks, multilingual document processing, robust evaluation protocols, and local translation systems. 2025.";

        assert_eq!(split_pdf_shim_text(text), vec![text]);
    }

    #[test]
    fn very_long_pdf_reference_uses_smaller_prompt_budget() {
        let text = "[33] Mireille Gloria, Alice Researcher, Bob Scientist, Carol Author, Daniel Engineer, Elena Analyst, Frank Reviewer, Grace Writer, Hannah Curator, Ian Maintainer, Julia Editor, Kevin Parser, Laura Reviewer, Mason Evaluator, Nora Architect, Owen Writer, Paula Designer, Quinn Translator, Riley Integrator, and Sam Engineer. A very long citation title about visual pest detection, remote sensing, feature extraction, convolutional networks, recurrent models, agricultural benchmarks, multilingual document processing, robust evaluation protocols, local translation systems, document layout preservation, multimodal large language models, pest recognition datasets, saliency guided window partitioning, and production PDF translation pipelines. 2025.";

        let chunks = split_pdf_shim_text(text);

        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| estimate_prompt_tokens(chunk) <= PDF_SHIM_REFERENCE_TARGET_PROMPT_TOKENS));
    }

    #[test]
    fn short_sentences_are_merged_until_target_budget() {
        let text = "First short sentence. Second short sentence. Third short sentence.";

        assert_eq!(split_pdf_shim_text(text), vec![text]);
    }

    #[test]
    fn translated_chunks_join_without_spaces_for_chinese() {
        assert_eq!(
            join_translated_chunks(
                vec!["第一段。".to_string(), "第二段。".to_string()],
                "zh-CN"
            ),
            "第一段。第二段。"
        );
        assert_eq!(
            join_translated_chunks(vec!["First.".to_string(), "Second.".to_string()], "en"),
            "First. Second."
        );
    }
}
