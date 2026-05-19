use std::{
    path::PathBuf,
    sync::Arc,
};

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, task::JoinHandle};

use crate::rwkv_providers::{
    mobile_batch_chat::{self, MobileBatchChatConfig},
    ProviderTranslateBatch,
};

#[derive(Debug)]
pub struct OpenAiShim {
    port: u16,
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

#[derive(Debug, Clone)]
struct ShimState {
    rwkv: MobileBatchChatConfig,
    target_lang: String,
    log_file: PathBuf,
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
) -> Result<OpenAiShim, String> {
    let rwkv = MobileBatchChatConfig {
        base_url: rwkv_base_url,
        timeout_ms,
    };
    mobile_batch_chat::set_chat_roles_for_pair(&rwkv, &source_lang, &target_lang, None).await?;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("无法启动 OpenAI shim: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("无法读取 OpenAI shim 端口: {error}"))?
        .port();
    append_log(
        &log_file,
        &format!(
            "spawn shim base_url={} source_lang={} target_lang={}",
            rwkv.base_url, source_lang, target_lang
        ),
    );
    let state = Arc::new(ShimState {
        rwkv,
        target_lang,
        log_file,
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);
    let join_handle = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("[pdf2zh-shim] server exited: {error}");
        }
    });
    Ok(OpenAiShim { port, join_handle })
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
            &state.log_file,
            &format!("placeholder passthrough={}", preview(&text)),
        );
        return Ok(openai_response(text));
    }
    if text.trim().is_empty() {
        append_log(&state.log_file, "empty source passthrough");
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
    let source_texts = vec![text];
    let result = mobile_batch_chat::translate_batch(
        &state.rwkv,
        ProviderTranslateBatch {
            source_texts: &source_texts,
            target_lang: &state.target_lang,
            timeout_ms: state.rwkv.timeout_ms,
            cancel: None,
        },
    )
    .await;

    if !result.ok {
        append_log(
            &state.log_file,
            &format!("rwkv error status={:?} message={}", result.status_code, result.message),
        );
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("RWKV 翻译失败: {}", result.message),
        ));
    }

    let content = result.translations.into_iter().next().unwrap_or_default();
    append_log(
        &state.log_file,
        &format!("rwkv translation_preview={}", preview(&content)),
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

fn append_log(path: &PathBuf, line: &str) {
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
