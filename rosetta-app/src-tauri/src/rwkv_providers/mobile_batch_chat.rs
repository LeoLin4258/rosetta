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
const SUPPORTED_BATCH_SIZES_PATH: &str = "/v1/batch/supported_batch_sizes";
const SUPPORTED_BATCH_SIZES_TIMEOUT_MS: u64 = 8_000;
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

#[derive(Debug, Deserialize)]
struct SupportedBatchSizesResponse {
    supported_batch_sizes: Vec<u32>,
}

/// Ask the sidecar what batch sizes its loaded model can serve.
///
/// Phase 0 observed `[1..12]` on M4 mini + 1.5B G1c nf4. The values are model
/// + backend specific; never hardcode them in the orchestrator. The
/// orchestrator is expected to call this once per run, pick `.iter().max()`
/// (clamped against the segment-length policy in the future), and reuse for
/// every batch in the run.
pub async fn query_supported_batch_sizes(
    config: &MobileBatchChatConfig,
) -> Result<Vec<u32>, String> {
    let client = loopback_client(SUPPORTED_BATCH_SIZES_TIMEOUT_MS)
        .map_err(|error| format!("无法创建 RWKV HTTP client: {error}"))?;
    let url = join_url(&config.base_url, SUPPORTED_BATCH_SIZES_PATH);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("GET {url} 失败: {error}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!(
            "{SUPPORTED_BATCH_SIZES_PATH} 返回 HTTP {}",
            status.as_u16()
        ));
    }
    let parsed: SupportedBatchSizesResponse = resp
        .json()
        .await
        .map_err(|error| format!("解析 {SUPPORTED_BATCH_SIZES_PATH} 响应失败: {error}"))?;
    if parsed.supported_batch_sizes.is_empty() {
        return Err(format!("{SUPPORTED_BATCH_SIZES_PATH} 返回了空数组。"));
    }
    Ok(parsed.supported_batch_sizes)
}

/// Pick the batch size to use for a run. Takes the sidecar's reported sizes
/// plus an optional hint from the caller (e.g. legacy `BATCH_SIZE = 16`).
///
/// Rules:
/// - If the hint is `0` (or absent), return the supported max.
/// - Otherwise return `min(hint, supported.max)`. Floor of 1 because every
///   sidecar supports batch=1.
pub fn pick_batch_size(supported: &[u32], hint: usize) -> usize {
    let max_supported = supported.iter().copied().max().unwrap_or(1) as usize;
    if hint == 0 {
        return max_supported.max(1);
    }
    hint.min(max_supported).max(1)
}

/// Decide the batch cap for a batch whose **longest** segment is `max_chars`
/// characters. Returns a value `≤ ceiling` so the per-run cap from
/// `pick_batch_size` still wins.
///
/// Heuristic (Phase 6.B):
/// - `≤ 800` chars → use ceiling (short/medium segments still scale well)
/// - `801..=1600` → ceiling / 2 (longer paragraphs)
/// - `1601..=2500` → ceiling / 3 (long, but batch=4 beat batch=3 on M4)
/// - `> 2500` → 1 (sequential; Phase 6.C may pre-split these)
///
/// These thresholds are bench-informed from M4 mini + WebRWKV on 2026-05-21.
/// They remain heuristics, but avoid the old over-conservative 301-char cutoff
/// that left useful batch capacity idle for common paragraph-sized segments.
pub fn pick_batch_for_length(ceiling: usize, max_chars: usize) -> usize {
    let limit = match max_chars {
        0..=800 => ceiling,
        801..=1600 => ceiling.div_ceil(2),
        1601..=2500 => ceiling.div_ceil(3),
        _ => 1,
    };
    limit.max(1).min(ceiling.max(1))
}

/// Greedy length-bucket batch planner.
///
/// Walks `targets` in original order (preserving the user's segment ordering
/// in the output document) and groups them into batches such that:
///
/// 1. No batch is larger than `pick_batch_for_length(ceiling, batch_max_chars)`.
/// 2. When adding a segment would push the batch's longest segment into a
///    smaller bucket — i.e., the new `max_chars` allows fewer slots than the
///    batch already holds — the current batch is flushed first so the new
///    (longer) segment starts a fresh, smaller batch.
///
/// This keeps short segments throughputs at the sidecar's max while long
/// segments naturally fall into smaller batches without explicit sorting.
///
/// The closure `text_len` lets callers extract char-count from any item type;
/// tests use `String` directly, the run loop uses `Segment::source_text`.
pub fn plan_batches<'a, T, F>(targets: &'a [T], ceiling: usize, text_len: F) -> Vec<Vec<&'a T>>
where
    F: Fn(&T) -> usize,
{
    let mut batches: Vec<Vec<&'a T>> = Vec::new();
    let mut current: Vec<&'a T> = Vec::new();
    let mut current_max_chars: usize = 0;

    for target in targets {
        let target_len = text_len(target);
        let next_max_chars = current_max_chars.max(target_len);
        let cap = pick_batch_for_length(ceiling, next_max_chars);

        // Adding this segment would either exceed the cap implied by the new
        // max length, OR shrink an already-formed batch past its current
        // length — flush before adding.
        if !current.is_empty() && current.len() + 1 > cap {
            batches.push(std::mem::take(&mut current));
            current_max_chars = 0;
        }

        current.push(target);
        current_max_chars = current_max_chars.max(target_len);
    }

    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

/// Set `/v1/chat/roles` once per run. Extracted from `translate_batch` so the
/// orchestrator can hit the server one time per direction instead of once per
/// batch — `roles` is global server state, repeated POSTs are wasted RTTs.
pub async fn set_chat_roles_for_pair(
    config: &MobileBatchChatConfig,
    source_lang: &str,
    target_lang: &str,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<(), String> {
    let user_role = role_label_for_lang(source_lang);
    let assistant_role = role_label_for_lang(target_lang);
    set_chat_roles(config, user_role, assistant_role, cancel).await
}

async fn set_chat_roles(
    config: &MobileBatchChatConfig,
    user_role: &str,
    assistant_role: &str,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<(), String> {
    let client = loopback_client(config.timeout_ms)
        .map_err(|error| format!("无法创建 RWKV HTTP client: {error}"))?;

    if is_cancelled(cancel.as_ref()) {
        return Err("RWKV 翻译请求已取消。".to_string());
    }

    let url = join_url(&config.base_url, SET_ROLES_PATH);
    let body = SetRolesRequest {
        user_role,
        assistant_role,
    };
    let resp = send_request_with_cancel(client.post(&url).json(&body), cancel).await?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("{SET_ROLES_PATH} 返回 HTTP {status}: {body}"));
    }
    // Drain to release the connection back to the pool.
    let _ = resp.bytes().await;
    Ok(())
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

    // Assistant role label is needed for response parsing (strip prefix).
    let assistant_role = role_label_for_lang(batch.target_lang);

    let client = match loopback_client(batch.timeout_ms) {
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

    // NOTE: `/v1/chat/roles` is **not** posted here. The orchestrator is
    // expected to set roles once per run via `set_chat_roles_for_pair`. This
    // saves one HTTP round-trip per batch and keeps a single direction per run.
    // Callers that need an ad-hoc one-shot translation should call
    // `set_chat_roles_for_pair` first.

    let cleaned_texts = batch
        .source_texts
        .iter()
        .map(|text| crate::rwkv_text_cleaning::clean_text_for_rwkv(text))
        .collect::<Vec<_>>();
    let conversations: Vec<Conversation> = cleaned_texts
        .iter()
        .map(|text| Conversation {
            messages: vec![Message {
                role: "user",
                content: text.as_ref(),
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
        log_mobile_rwkv_io(
            batch.debug_context,
            &chat_url,
            batch.target_lang,
            &cleaned_texts,
            Some(chat_status),
            false,
            Some("HTTP error"),
            &[],
            Some(&response_text),
        );
        return error_result_with_status(
            chat_status,
            started_at,
            &response_text,
            format!("/v1/batch/chat 返回 HTTP {chat_status}。"),
        );
    }

    let parsed_translations =
        parse_translations(&response_text, batch.source_texts.len(), assistant_role);
    match &parsed_translations {
        Ok(translations) => log_mobile_rwkv_io(
            batch.debug_context,
            &chat_url,
            batch.target_lang,
            &cleaned_texts,
            Some(chat_status),
            true,
            None,
            translations,
            Some(&response_text),
        ),
        Err(error) => log_mobile_rwkv_io(
            batch.debug_context,
            &chat_url,
            batch.target_lang,
            &cleaned_texts,
            Some(chat_status),
            false,
            Some(error),
            &[],
            Some(&response_text),
        ),
    }

    match parsed_translations {
        Ok(translations) => ProviderTranslateResult {
            ok: true,
            status_code: Some(chat_status),
            translations,
            raw_response_preview: String::new(),
            message: format!(
                "RWKV /v1/batch/chat 已翻译 {} 条。",
                batch.source_texts.len()
            ),
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
    let started_at = Instant::now();
    // Probe is a one-shot operation, so it sets its own roles instead of
    // assuming an orchestrator already did. `translate_batch` itself no longer
    // touches `/v1/chat/roles`.
    if let Err(message) = set_chat_roles_for_pair(config, source_lang, target_lang, None).await {
        return error_result(None, started_at, message);
    }

    let texts: Vec<String> = PROBE_TEXTS.iter().map(|s| s.to_string()).collect();
    let mut result = translate_batch(
        config,
        ProviderTranslateBatch {
            source_texts: &texts,
            target_lang,
            timeout_ms: config.timeout_ms,
            cancel: None,
            debug_context: Some("mobile-probe"),
        },
    )
    .await;

    if result.ok {
        result.message = "RWKV 本地 /v1/batch/chat 探测成功。".to_string();
    }
    result
}

fn log_mobile_rwkv_io(
    debug_context: Option<&str>,
    endpoint: &str,
    target_lang: &str,
    inputs: &[std::borrow::Cow<'_, str>],
    status_code: Option<u16>,
    ok: bool,
    error: Option<&str>,
    translations: &[String],
    raw_response: Option<&str>,
) {
    if !crate::rwkv_io_debug::enabled() {
        return;
    }
    crate::rwkv_io_debug::log_record(crate::rwkv_io_debug::RwkvIoDebugRecord {
        provider: "rwkv-mobile-batch-chat",
        context: debug_context,
        endpoint: Some(endpoint),
        source_lang: None,
        target_lang: Some(target_lang),
        status_code,
        ok,
        error,
        inputs: inputs.iter().map(|text| text.as_ref()).collect(),
        outputs: translations.iter().map(String::as_str).collect(),
        raw_response,
    });
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

/// Build a reqwest client tuned for **loopback** (`127.0.0.1`) traffic.
///
/// Critically calls `.no_proxy()`: reqwest reads `HTTPS_PROXY` / `HTTP_PROXY`
/// env vars by default, and users running Tauri behind Clash routinely have
/// those set so the install step can reach HuggingFace. Without `.no_proxy()`
/// every loopback request (set_chat_roles, batch_chat, supported_batch_sizes,
/// /health probe) would also be funnelled through Clash → connections refused
/// or hung. Loopback traffic should never be proxied.
fn loopback_client(timeout_ms: u64) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
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
            eprintln!(
                "[rwkv-cancel] send_request_with_cancel: cancel observed, aborting in-flight POST"
            );
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
            eprintln!(
                "[rwkv-cancel] read_text_with_cancel: cancel observed, aborting response body read"
            );
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
        let content = "He said Chinese: that's interesting.\n\nChinese: 他说中文：那很有意思。";
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
        let error = parse_translations("not json", 1, "Chinese").expect_err("non-json should fail");
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

    #[test]
    fn pick_batch_size_returns_supported_max_when_hint_is_zero() {
        // Hint=0 means "auto" — let Rust pick the sidecar's reported maximum.
        assert_eq!(pick_batch_size(&[1, 2, 4, 8, 12], 0), 12);
        assert_eq!(pick_batch_size(&[1], 0), 1);
    }

    #[test]
    fn pick_batch_size_clamps_hint_to_supported_max() {
        // Legacy frontend passes 16; sidecar advertises max=12 → clamp to 12.
        assert_eq!(pick_batch_size(&[1, 2, 4, 8, 12], 16), 12);
        assert_eq!(pick_batch_size(&[8], 99), 8);
    }

    #[test]
    fn pick_batch_size_keeps_hint_when_below_max() {
        // A conservative hint below sidecar max should be respected (Phase 6.B
        // length-bucket scheduler will use this to keep long segments small).
        assert_eq!(pick_batch_size(&[1, 2, 4, 8, 12], 4), 4);
        assert_eq!(pick_batch_size(&[1, 2, 4, 8, 12], 1), 1);
    }

    #[test]
    fn pick_batch_size_floors_at_one() {
        // Degenerate inputs shouldn't yield 0 — every sidecar supports 1.
        assert_eq!(pick_batch_size(&[], 0), 1);
        assert_eq!(pick_batch_size(&[5], 0), 5);
    }

    #[test]
    fn supported_batch_sizes_response_parses_phase_0_shape() {
        // Locks in the wire format observed on M4 mini + 1.5B G1c nf4.
        let body =
            r#"{"model":"rwkv-translate","supported_batch_sizes":[1,2,3,4,5,6,7,8,9,10,11,12]}"#;
        let parsed: SupportedBatchSizesResponse =
            serde_json::from_str(body).expect("phase 0 response shape should parse");
        assert_eq!(
            parsed.supported_batch_sizes,
            (1u32..=12).collect::<Vec<_>>()
        );
    }

    // ---- Phase 6.B: length bucket policy + greedy batch planner ----

    #[test]
    fn pick_batch_for_length_bucket_transitions_on_ceiling_12() {
        // Same ceiling the Phase 0 sidecar reports (max of [1..12]).
        assert_eq!(pick_batch_for_length(12, 0), 12);
        assert_eq!(pick_batch_for_length(12, 800), 12); // full-batch boundary
        assert_eq!(pick_batch_for_length(12, 801), 6); // medium
        assert_eq!(pick_batch_for_length(12, 1600), 6); // medium upper boundary
        assert_eq!(pick_batch_for_length(12, 1601), 4); // long
        assert_eq!(pick_batch_for_length(12, 2500), 4); // long upper boundary
        assert_eq!(pick_batch_for_length(12, 2501), 1); // huge — sequential
        assert_eq!(pick_batch_for_length(12, 10_000), 1);
    }

    #[test]
    fn pick_batch_for_length_floors_at_one_and_respects_ceiling() {
        // Tiny ceilings shouldn't accidentally yield 0 from div_ceil math.
        assert_eq!(pick_batch_for_length(1, 50), 1);
        assert_eq!(pick_batch_for_length(1, 5000), 1);
        // Full-batch short/medium segments should respect small ceilings, and
        // div_ceil math should never round long buckets to zero.
        assert_eq!(pick_batch_for_length(3, 500), 3);
        assert_eq!(pick_batch_for_length(3, 1500), 2);
        assert_eq!(pick_batch_for_length(3, 2000), 1);
    }

    #[test]
    fn plan_batches_uses_full_ceiling_for_short_segments() {
        let targets: Vec<String> = (0..15).map(|_| "short text".to_string()).collect();
        let batches = plan_batches(&targets, 12, |s| s.chars().count());
        // 12 short + 3 leftover.
        assert_eq!(
            batches.iter().map(|b| b.len()).collect::<Vec<_>>(),
            vec![12, 3]
        );
    }

    #[test]
    fn plan_batches_shrinks_when_a_long_segment_appears() {
        // 8 short, then 1 long (1500 chars → bucket 6), then 8 short.
        let mut targets: Vec<String> = (0..8).map(|i| format!("seg-{i}")).collect();
        targets.push("x".repeat(1500));
        targets.extend((9..17).map(|i| format!("seg-{i}")));
        let batches = plan_batches(&targets, 12, |s| s.chars().count());
        // First flush at boundary: when the 1500-char segment arrives, the
        // cap drops to 6 → batch of 8 short flushes, then the long segment
        // starts a fresh batch capped at 6, picking up the next 5 short ones,
        // then the remaining short segments form batches of 12.
        let sizes: Vec<usize> = batches.iter().map(|b| b.len()).collect();
        assert_eq!(sizes, vec![8, 6, 3]);
        // Total preserved and order preserved.
        let total: usize = sizes.iter().sum();
        assert_eq!(total, targets.len());
        let flat: Vec<&String> = batches.into_iter().flatten().collect();
        for (idx, item) in flat.iter().enumerate() {
            assert_eq!(*item, &targets[idx]);
        }
    }

    #[test]
    fn plan_batches_isolates_huge_segments_to_their_own_batch() {
        // > 2500 chars forces batch=1.
        let targets: Vec<String> = vec!["short".to_string(), "x".repeat(3000), "short".to_string()];
        let batches = plan_batches(&targets, 12, |s| s.chars().count());
        // First batch: short (would have allowed 12). When the 3000-char seg
        // arrives, cap drops to 1 → flush. Huge alone. Then short alone (it
        // formed after the huge flush; current is empty so it joins).
        let sizes: Vec<usize> = batches.iter().map(|b| b.len()).collect();
        assert_eq!(sizes, vec![1, 1, 1]);
    }

    #[test]
    fn plan_batches_empty_input_yields_no_batches() {
        let targets: Vec<String> = Vec::new();
        let batches = plan_batches(&targets, 12, |s| s.chars().count());
        assert!(batches.is_empty());
    }

    #[test]
    fn plan_batches_with_ceiling_one_collapses_to_singletons() {
        // Sidecar that only reports batch=1 (very low-end). Every segment is
        // its own batch regardless of length.
        let targets: Vec<String> = (0..5).map(|i| format!("seg-{i}")).collect();
        let batches = plan_batches(&targets, 1, |s| s.chars().count());
        assert_eq!(batches.len(), 5);
        for b in &batches {
            assert_eq!(b.len(), 1);
        }
    }
}
