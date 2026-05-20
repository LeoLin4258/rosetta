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

const BATCH_WINDOW_MS: u64 = 80;

#[derive(Debug)]
pub struct OpenAiShim {
    port: u16,
    pub batch_size: usize,
    join_handle: JoinHandle<()>,
}

impl OpenAiShim {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }
}

impl Drop for OpenAiShim {
    fn drop(&mut self) {
        self.join_handle.abort();
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
    rwkv_base_url: String,
    source_lang: String,
    target_lang: String,
    timeout_ms: u64,
    log_file: PathBuf,
    debug: bool,
) -> Result<OpenAiShim, String> {
    let rwkv = MobileBatchChatConfig {
        base_url: rwkv_base_url,
        timeout_ms,
    };
    mobile_batch_chat::set_chat_roles_for_pair(&rwkv, &source_lang, &target_lang, None).await?;

    let max_batch_size = mobile_batch_chat::query_supported_batch_sizes(&rwkv)
        .await
        .map(|sizes| mobile_batch_chat::pick_batch_size(&sizes, 0))
        .unwrap_or(DEFAULT_MAX_BATCH_SIZE)
        .max(1);

    let (batch_tx, batch_rx) = mpsc::channel(max_batch_size * 4);
    tokio::spawn(batch_processor(
        batch_rx,
        rwkv.clone(),
        target_lang.clone(),
        max_batch_size,
    ));

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
            "spawn shim base_url={} source_lang={} target_lang={} max_batch_size={}",
            rwkv.base_url, source_lang, target_lang, max_batch_size
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
    let join_handle = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("[pdf2zh-shim] server exited: {error}");
        }
    });
    Ok(OpenAiShim { port, batch_size: max_batch_size, join_handle })
}

async fn batch_processor(
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

        eprintln!("[pdf2zh-batch] result: ok={}, translations={}", result.ok, result.translations.len());
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
        .or_else(|| messages.iter().rev().find(|message| !message.content.trim().is_empty()))
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
        let token = token.trim_matches(|ch: char| matches!(ch, ',' | '.' | ';' | ':' | '，' | '。' | '；' | '：'));
        let Some(inner) = token.strip_prefix("$v").and_then(|value| value.strip_suffix('$')) else {
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
            ChatMessage { role: "system".to_string(), content: "rules".to_string() },
            ChatMessage { role: "user".to_string(), content: "first".to_string() },
            ChatMessage { role: "assistant".to_string(), content: "ignored".to_string() },
            ChatMessage { role: "user".to_string(), content: " second ".to_string() },
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
