use std::{path::PathBuf, sync::Arc};

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{timeout_at, Duration, Instant},
};

use crate::rwkv_providers::{
    mobile_batch_chat::{self, MobileBatchChatConfig},
    ProviderTranslateBatch,
};

const DEFAULT_MAX_BATCH_SIZE: usize = 4;

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
pub enum ShimProviderConfig {
    MobileBatch(MobileBatchChatConfig),
    Lightning(LightningApiConfig),
}

const BATCH_WINDOW_MS: u64 = 80;

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

struct ShimState {
    batch_tx: mpsc::Sender<PendingTranslation>,
    log_file: PathBuf,
    debug: bool,
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
) -> Result<OpenAiShim, String> {
    let (max_batch_size, batch_handle) = match provider {
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
                target_lang.clone(),
                max_batch_size,
            ));
            (max_batch_size, (batch_tx, handle))
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
            ));
            (max_batch_size, (batch_tx, handle))
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
            "spawn shim source_lang={} target_lang={} max_batch_size={}",
            source_lang, target_lang, max_batch_size
        ),
    );
    let state = Arc::new(ShimState {
        batch_tx,
        log_file,
        debug,
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
        server_handle,
        batch_handle,
    })
}

async fn mobile_batch_processor(
    mut rx: mpsc::Receiver<PendingTranslation>,
    rwkv: MobileBatchChatConfig,
    target_lang: String,
    max_batch_size: usize,
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
        let result = mobile_batch_chat::translate_batch(
            &rwkv,
            ProviderTranslateBatch {
                source_texts: &source_texts,
                target_lang: &target_lang,
                timeout_ms: rwkv.timeout_ms,
                cancel: None,
            },
        )
        .await;

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
        match crate::rwkv_api::translate_batch_via_lightning(
            &config.base_url,
            &config.endpoint,
            &config.internal_token,
            &config.body_password,
            config.timeout_ms,
            &source_lang,
            &target_lang,
            &source_texts,
        )
        .await
        {
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

    let (result_tx, result_rx) = oneshot::channel();
    state
        .batch_tx
        .send(PendingTranslation { text, result_tx })
        .await
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "翻译批处理队列已关闭。".to_string(),
            )
        })?;

    let content = result_rx
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "批处理结果接收失败。".to_string(),
            )
        })?
        .map_err(|error| (StatusCode::BAD_GATEWAY, format!("RWKV 翻译失败: {error}")))?;

    append_log(
        state.debug,
        &state.log_file,
        &format!("translation_preview={}", preview(&content)),
    );
    Ok(openai_response(content))
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
}
