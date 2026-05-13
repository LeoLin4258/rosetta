use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::{ProviderTranslateBatch, ProviderTranslateResult};

const SET_ROLES_PATH: &str = "/v1/chat/roles";
const BATCH_CHAT_PATH: &str = "/v1/batch/chat";
const RESPONSE_PREVIEW_CHARS: usize = 2_000;
const POLL_INTERVAL_MS: u64 = 50;
const MAX_TOKENS_PER_SEGMENT: u32 = 1024;

pub const PROBE_TEXTS: [&str; 2] = [
    "After a blissful two weeks, Jane encounters Rochester in the gardens.",
    "That night, a bolt of lightning splits the same chestnut tree.",
];

#[derive(Debug, Clone)]
pub struct MobileBatchChatConfig {
    pub base_url: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Serialize)]
struct SetRolesRequest<'a> {
    user_role: &'a str,
    assistant_role: &'a str,
}

#[derive(Debug, Serialize)]
struct BatchChatRequest<'a> {
    conversations: Vec<Conversation<'a>>,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct Conversation<'a> {
    messages: Vec<Message<'a>>,
}

#[derive(Debug, Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct BatchChatResponse {
    choices: Vec<BatchChatChoice>,
}

#[derive(Debug, Deserialize)]
struct BatchChatChoice {
    index: usize,
    message: BatchChatMessage,
}

#[derive(Debug, Deserialize)]
struct BatchChatMessage {
    content: String,
}

pub async fn translate_batch(
    config: &MobileBatchChatConfig,
    batch: ProviderTranslateBatch<'_>,
) -> ProviderTranslateResult {
    let started_at = Instant::now();

    if batch.source_texts.is_empty() {
        return ProviderTranslateResult {
            ok: true,
            status_code: None,
            translations: Vec::new(),
            raw_response_preview: String::new(),
            message: "没有需要翻译的文本。".to_string(),
            latency_ms: 0,
        };
    }

    let user_role = role_label_for_lang(batch.source_lang);
    let assistant_role = role_label_for_lang(batch.target_lang);

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(batch.timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return error_result(
                None,
                started_at,
                format!("无法创建 RWKV HTTP client: {error}"),
            );
        }
    };

    if is_cancelled(batch.cancel.as_ref()) {
        return error_result(None, started_at, "RWKV 翻译请求已取消。".to_string());
    }

    // Step 1 — set chat roles on the server. /v1/chat/roles is global server
    // state, so the orchestrator must keep one translation direction per run.
    let roles_url = join_url(&config.base_url, SET_ROLES_PATH);
    let roles_body = SetRolesRequest {
        user_role,
        assistant_role,
    };
    let roles_resp = match send_request_with_cancel(
        client.post(&roles_url).json(&roles_body),
        batch.cancel.clone(),
    )
    .await
    {
        Ok(resp) => resp,
        Err(message) => return error_result(None, started_at, message),
    };
    let roles_status = roles_resp.status().as_u16();
    if !(200..300).contains(&roles_status) {
        let body = roles_resp.text().await.unwrap_or_default();
        return error_result(
            Some(roles_status),
            started_at,
            format!("/v1/chat/roles 返回 HTTP {roles_status}: {body}"),
        );
    }
    // Drain to free the connection back to the pool.
    let _ = roles_resp.bytes().await;

    if is_cancelled(batch.cancel.as_ref()) {
        return error_result(None, started_at, "RWKV 翻译请求已取消。".to_string());
    }

    // Step 2 — issue the batch chat.
    let conversations: Vec<Conversation> = batch
        .source_texts
        .iter()
        .map(|text| Conversation {
            messages: vec![Message {
                role: "user",
                content: text.as_str(),
            }],
        })
        .collect();
    let chat_body = BatchChatRequest {
        conversations,
        max_tokens: MAX_TOKENS_PER_SEGMENT,
    };
    let chat_url = join_url(&config.base_url, BATCH_CHAT_PATH);
    let chat_resp = match send_request_with_cancel(
        client.post(&chat_url).json(&chat_body),
        batch.cancel.clone(),
    )
    .await
    {
        Ok(resp) => resp,
        Err(message) => return error_result(None, started_at, message),
    };
    let chat_status = chat_resp.status().as_u16();

    let response_text = match read_text_with_cancel(chat_resp, batch.cancel).await {
        Ok(text) => text,
        Err(message) => {
            return error_result_with_status(chat_status, started_at, "", message);
        }
    };

    if !(200..300).contains(&chat_status) {
        return error_result_with_status(
            chat_status,
            started_at,
            &response_text,
            format!("/v1/batch/chat 返回 HTTP {chat_status}。"),
        );
    }

    match parse_translations(&response_text, batch.source_texts.len(), assistant_role) {
        Ok(translations) => ProviderTranslateResult {
            ok: true,
            status_code: Some(chat_status),
            translations,
            raw_response_preview: String::new(),
            message: format!("RWKV /v1/batch/chat 已翻译 {} 条。", batch.source_texts.len()),
            latency_ms: started_at.elapsed().as_millis(),
        },
        Err(error) => error_result_with_status(
            chat_status,
            started_at,
            &response_text,
            format!("RWKV /v1/batch/chat 响应格式不可用: {error}"),
        ),
    }
}

pub async fn probe(
    config: &MobileBatchChatConfig,
    source_lang: &str,
    target_lang: &str,
) -> ProviderTranslateResult {
    let texts: Vec<String> = PROBE_TEXTS.iter().map(|s| s.to_string()).collect();
    let mut result = translate_batch(
        config,
        ProviderTranslateBatch {
            source_texts: &texts,
            source_lang,
            target_lang,
            timeout_ms: config.timeout_ms,
            cancel: None,
        },
    )
    .await;

    if result.ok {
        result.message = "RWKV 本地 /v1/batch/chat 探测成功。".to_string();
    }
    result
}

fn parse_translations(
    response_text: &str,
    expected_count: usize,
    assistant_role: &str,
) -> Result<Vec<String>, String> {
    let response: BatchChatResponse = serde_json::from_str(response_text)
        .map_err(|error| format!("JSON parse failed: {error}"))?;

    let mut translations: Vec<Option<String>> = vec![None; expected_count];
    for choice in response.choices {
        if choice.index >= expected_count {
            continue;
        }

        let stripped = strip_response_prefix(&choice.message.content, assistant_role);
        if stripped.is_empty() {
            return Err(format!("choice {} returned empty content", choice.index));
        }
        translations[choice.index] = Some(stripped);
    }

    translations
        .into_iter()
        .enumerate()
        .map(|(index, translation)| {
            translation.ok_or_else(|| format!("missing translation for choice index {index}"))
        })
        .collect()
}

/// Strip rwkv-mobile's echoed source + `<assistant_role>:` prefix and keep only
/// the translated text.
///
/// Real responses observed on RWKV_v7_G1c with WebRWKV backend look like:
///
/// ```text
/// The quick brown fox jumps over the lazy dog.
///
/// Chinese: 这只快速的棕色狐狸跳过了懒惰的狗。
/// ```
///
/// We split on the **assistant role label** rather than a hardcoded "Chinese:"
/// so the same parser works when the target language flips (e.g., ZH→EN gives
/// `English:` instead).
///
/// If the marker is absent (degenerate response), the entire content is
/// returned trimmed, on the theory that some models return raw translation
/// without the role echo.
fn strip_response_prefix(content: &str, assistant_role: &str) -> String {
    let marker = format!("\n{}:", assistant_role);
    match content.find(&marker) {
        Some(idx) => content[idx + marker.len()..].trim().to_string(),
        None => content.trim().to_string(),
    }
}

fn role_label_for_lang(lang: &str) -> &'static str {
    match lang {
        "en" => "English",
        "zh-CN" | "zh-TW" | "zh" => "Chinese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "ru" => "Russian",
        "pt" => "Portuguese",
        "it" => "Italian",
        "vi" => "Vietnamese",
        "id" => "Indonesian",
        _ => "English",
    }
}

fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn is_cancelled(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.is_some_and(|cancel| cancel.load(Ordering::SeqCst))
}

async fn send_request_with_cancel(
    builder: reqwest::RequestBuilder,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<reqwest::Response, String> {
    let future = builder.send();
    let Some(cancel) = cancel else {
        return future
            .await
            .map_err(|error| format!("RWKV HTTP 请求失败: {error}"));
    };

    let handle = tokio::spawn(future);
    loop {
        if cancel.load(Ordering::SeqCst) {
            handle.abort();
            return Err("RWKV 翻译请求已取消。".to_string());
        }
        if handle.is_finished() {
            return match handle.await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(error)) => Err(format!("RWKV HTTP 请求失败: {error}")),
                Err(error) => Err(format!("RWKV HTTP 请求任务失败: {error}")),
            };
        }
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

async fn read_text_with_cancel(
    response: reqwest::Response,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, String> {
    let Some(cancel) = cancel else {
        return response
            .text()
            .await
            .map_err(|error| format!("无法读取 RWKV 响应: {error}"));
    };

    let handle = tokio::spawn(response.text());
    loop {
        if cancel.load(Ordering::SeqCst) {
            handle.abort();
            return Err("RWKV 翻译请求已取消。".to_string());
        }
        if handle.is_finished() {
            return match handle.await {
                Ok(Ok(text)) => Ok(text),
                Ok(Err(error)) => Err(format!("无法读取 RWKV 响应: {error}")),
                Err(error) => Err(format!("RWKV 响应读取任务失败: {error}")),
            };
        }
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

fn preview_text(text: &str) -> String {
    text.chars().take(RESPONSE_PREVIEW_CHARS).collect()
}

fn error_result(
    status_code: Option<u16>,
    started_at: Instant,
    message: String,
) -> ProviderTranslateResult {
    ProviderTranslateResult {
        ok: false,
        status_code,
        translations: Vec::new(),
        raw_response_preview: String::new(),
        message,
        latency_ms: started_at.elapsed().as_millis(),
    }
}

fn error_result_with_status(
    status_code: u16,
    started_at: Instant,
    response_text: &str,
    message: String,
) -> ProviderTranslateResult {
    ProviderTranslateResult {
        ok: false,
        status_code: Some(status_code),
        translations: Vec::new(),
        raw_response_preview: preview_text(response_text),
        message,
        latency_ms: started_at.elapsed().as_millis(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn strip_prefix_extracts_chinese_translation() {
        let content = "The quick brown fox jumps over the lazy dog.\n\nChinese: 这只快速的棕色狐狸跳过了懒惰的狗。";
        let stripped = strip_response_prefix(content, "Chinese");
        assert_eq!(stripped, "这只快速的棕色狐狸跳过了懒惰的狗。");
    }

    #[test]
    fn strip_prefix_handles_english_role_for_zh_to_en() {
        let content = "你好，世界。\n\nEnglish: Hello, world.";
        let stripped = strip_response_prefix(content, "English");
        assert_eq!(stripped, "Hello, world.");
    }

    #[test]
    fn strip_prefix_falls_back_when_marker_missing() {
        let content = "  pure translation without prefix  ";
        let stripped = strip_response_prefix(content, "Chinese");
        assert_eq!(stripped, "pure translation without prefix");
    }

    #[test]
    fn strip_prefix_does_not_match_role_inside_source_text() {
        // Source text could legitimately contain "Chinese:" as a substring;
        // we anchor on `\nChinese:` (newline + role + colon) so substrings
        // inside the echoed source don't trip the splitter.
        let content =
            "He said Chinese: that's interesting.\n\nChinese: 他说中文：那很有意思。";
        let stripped = strip_response_prefix(content, "Chinese");
        assert_eq!(stripped, "他说中文：那很有意思。");
    }

    #[test]
    fn strip_prefix_handles_multiline_source_echo() {
        let content = "Line one.\nLine two.\nLine three.\n\nChinese: 第一行。\n第二行。\n第三行。";
        let stripped = strip_response_prefix(content, "Chinese");
        assert_eq!(stripped, "第一行。\n第二行。\n第三行。");
    }

    #[test]
    fn parse_translations_restores_choice_index_order() {
        let response = json!({
            "choices": [
                {"index": 1, "message": {"content": "B en.\n\nChinese: 第二段"}},
                {"index": 0, "message": {"content": "A en.\n\nChinese: 第一段"}},
            ]
        });
        let translations = parse_translations(&response.to_string(), 2, "Chinese")
            .expect("translations should parse");
        assert_eq!(
            translations,
            vec!["第一段".to_string(), "第二段".to_string()]
        );
    }

    #[test]
    fn parse_translations_rejects_missing_index() {
        let response = json!({
            "choices": [
                {"index": 0, "message": {"content": "A.\n\nChinese: 一"}}
            ]
        });
        let error = parse_translations(&response.to_string(), 2, "Chinese")
            .expect_err("missing translation should fail");
        assert!(error.contains("missing translation for choice index 1"));
    }

    #[test]
    fn parse_translations_rejects_empty_after_stripping() {
        let response = json!({
            "choices": [
                {"index": 0, "message": {"content": "Source text.\n\nChinese:    "}}
            ]
        });
        let error = parse_translations(&response.to_string(), 1, "Chinese")
            .expect_err("empty translation should fail");
        assert!(error.contains("empty content"));
    }

    #[test]
    fn parse_translations_rejects_non_json() {
        let error = parse_translations("not json", 1, "Chinese")
            .expect_err("non-json should fail");
        assert!(error.contains("JSON parse failed"));
    }

    #[test]
    fn parse_translations_ignores_out_of_range_index() {
        // Server may someday return an extra choice with index >= expected;
        // we just ignore it instead of crashing, matching the lightning-contents
        // adapter's behavior.
        let response = json!({
            "choices": [
                {"index": 0, "message": {"content": "A.\n\nChinese: 一"}},
                {"index": 1, "message": {"content": "B.\n\nChinese: 二"}},
                {"index": 9, "message": {"content": "X.\n\nChinese: 九"}},
            ]
        });
        let translations = parse_translations(&response.to_string(), 2, "Chinese")
            .expect("should ignore extra choice");
        assert_eq!(translations, vec!["一".to_string(), "二".to_string()]);
    }

    #[test]
    fn parse_translations_falls_back_when_response_has_no_role_prefix() {
        // Some future models / configs might emit raw translation; we accept it.
        let response = json!({
            "choices": [
                {"index": 0, "message": {"content": "  raw translation only  "}}
            ]
        });
        let translations = parse_translations(&response.to_string(), 1, "Chinese")
            .expect("raw translation should pass");
        assert_eq!(translations, vec!["raw translation only".to_string()]);
    }

    #[test]
    fn join_url_does_not_double_slash() {
        assert_eq!(
            join_url("http://127.0.0.1:8765", "/v1/batch/chat"),
            "http://127.0.0.1:8765/v1/batch/chat"
        );
        assert_eq!(
            join_url("http://127.0.0.1:8765/", "v1/batch/chat"),
            "http://127.0.0.1:8765/v1/batch/chat"
        );
    }

    #[test]
    fn role_label_for_lang_maps_known_codes() {
        assert_eq!(role_label_for_lang("en"), "English");
        assert_eq!(role_label_for_lang("zh-CN"), "Chinese");
        assert_eq!(role_label_for_lang("zh"), "Chinese");
        assert_eq!(role_label_for_lang("ja"), "Japanese");
        assert_eq!(role_label_for_lang("unknown"), "English");
    }
}
