use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::{ProviderTranslateBatch, ProviderTranslateResult};

const COMPLETION_PATH: &str = "/completion";
const RESPONSE_PREVIEW_CHARS: usize = 2_000;
const POLL_INTERVAL_MS: u64 = 50;
const MAX_TOKENS_PER_SEGMENT: u32 = 1024;

pub const DEFAULT_PARALLEL_REQUESTS: usize = 16;
pub const PROBE_TEXTS: [&str; 2] = [
    "After a blissful two weeks, Jane encounters Rochester in the gardens.",
    "That night, a bolt of lightning splits the same chestnut tree.",
];

#[derive(Debug, Clone)]
pub struct LlamaCppChatConfig {
    pub base_url: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Serialize)]
struct CompletionRequest {
    prompt: String,
    n_predict: u32,
    temperature: f64,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    #[serde(default)]
    content: String,
}

pub async fn translate_batch(
    config: &LlamaCppChatConfig,
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

    let client = match loopback_client(batch.timeout_ms) {
        Ok(client) => client,
        Err(error) => {
            return error_result(
                None,
                started_at,
                format!("无法创建 llama.cpp HTTP client: {error}"),
            );
        }
    };

    let url = join_url(&config.base_url, COMPLETION_PATH);
    let source_lang = batch.source_lang.to_string();
    let target_lang = batch.target_lang.to_string();
    let debug_context = batch.debug_context.map(str::to_string);
    let handles = batch
        .source_texts
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let client = client.clone();
            let url = url.clone();
            let source_lang = source_lang.clone();
            let target_lang = target_lang.clone();
            let debug_context = debug_context.clone();
            let cancel = batch.cancel.clone();
            let source = source.clone();
            tokio::spawn(async move {
                translate_one(
                    client,
                    url,
                    index,
                    source,
                    source_lang,
                    target_lang,
                    cancel,
                    debug_context,
                )
                .await
            })
        })
        .collect::<Vec<_>>();

    let mut translations = vec![String::new(); batch.source_texts.len()];
    let mut raw_preview = String::new();
    let mut status_code = None;
    let mut first_error: Option<ProviderTranslateResult> = None;

    for handle in handles {
        match handle.await {
            Ok(Ok(result)) => {
                status_code = Some(result.status_code);
                if raw_preview.is_empty() {
                    raw_preview = result.raw_response_preview;
                }
                translations[result.index] = result.translation;
            }
            Ok(Err(error)) => {
                if first_error.is_none() {
                    first_error = Some(error_result_with_status(
                        error.status_code,
                        started_at,
                        &error.raw_response,
                        error.message,
                    ));
                }
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error_result(
                        None,
                        started_at,
                        format!("llama.cpp 请求任务失败: {error}"),
                    ));
                }
            }
        }
    }

    if let Some(error) = first_error {
        return error;
    }

    ProviderTranslateResult {
        ok: true,
        status_code,
        translations,
        raw_response_preview: raw_preview,
        message: format!(
            "llama.cpp /completion 已翻译 {} 条。",
            batch.source_texts.len()
        ),
        latency_ms: started_at.elapsed().as_millis(),
    }
}

struct SingleTranslation {
    index: usize,
    status_code: u16,
    translation: String,
    raw_response_preview: String,
}

struct SingleTranslationError {
    status_code: u16,
    raw_response: String,
    message: String,
}

async fn translate_one(
    client: reqwest::Client,
    url: String,
    index: usize,
    source: String,
    source_lang: String,
    target_lang: String,
    cancel: Option<Arc<AtomicBool>>,
    debug_context: Option<String>,
) -> Result<SingleTranslation, SingleTranslationError> {
    if is_cancelled(cancel.as_ref()) {
        return Err(SingleTranslationError {
            status_code: 499,
            raw_response: String::new(),
            message: "RWKV 翻译请求已取消。".to_string(),
        });
    }

    let cleaned = crate::rwkv_text_cleaning::clean_text_for_rwkv(&source);
    let body = build_completion_request(cleaned.as_ref(), &source_lang, &target_lang);
    let response = send_request_with_cancel(client.post(&url).json(&body), cancel.clone())
        .await
        .map_err(|message| SingleTranslationError {
            status_code: 599,
            raw_response: String::new(),
            message,
        })?;
    let http_status = response.status().as_u16();
    let response_text = read_text_with_cancel(response, cancel)
        .await
        .map_err(|message| SingleTranslationError {
            status_code: http_status,
            raw_response: String::new(),
            message,
        })?;

    if !(200..300).contains(&http_status) {
        log_llama_rwkv_io(
            debug_context.as_deref(),
            &url,
            &target_lang,
            &[cleaned.as_ref()],
            Some(http_status),
            false,
            Some("HTTP error"),
            &[],
            Some(&response_text),
        );
        let detail = llama_cpp_error_message(&response_text)
            .unwrap_or_else(|| format!("llama.cpp /completion 返回 HTTP {http_status}。"));
        return Err(SingleTranslationError {
            status_code: http_status,
            raw_response: response_text,
            message: format!("llama.cpp /completion 返回 HTTP {http_status}: {detail}"),
        });
    }

    let translation = parse_translation(&response_text).map_err(|error| {
        log_llama_rwkv_io(
            debug_context.as_deref(),
            &url,
            &target_lang,
            &[cleaned.as_ref()],
            Some(http_status),
            false,
            Some(&error),
            &[],
            Some(&response_text),
        );
        SingleTranslationError {
            status_code: http_status,
            raw_response: response_text.clone(),
            message: format!("llama.cpp 响应格式不可用: {error}"),
        }
    })?;
    log_llama_rwkv_io(
        debug_context.as_deref(),
        &url,
        &target_lang,
        &[cleaned.as_ref()],
        Some(http_status),
        true,
        None,
        std::slice::from_ref(&translation),
        Some(&response_text),
    );
    Ok(SingleTranslation {
        index,
        status_code: http_status,
        translation,
        raw_response_preview: preview_text(&response_text),
    })
}

pub async fn probe(
    config: &LlamaCppChatConfig,
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
            debug_context: Some("llama-cpp-probe"),
        },
    )
    .await;
    if result.ok {
        result.message = "llama.cpp 本地 /completion 探测成功。".to_string();
    }
    result
}

fn build_completion_request(
    source_text: &str,
    source_lang: &str,
    target_lang: &str,
) -> CompletionRequest {
    let source_label = role_label_for_lang(source_lang);
    let target_label = role_label_for_lang(target_lang);
    CompletionRequest {
        prompt: format!("{source_label}: {source_text}\n\n{target_label}:"),
        n_predict: MAX_TOKENS_PER_SEGMENT,
        temperature: 1.0,
        stream: false,
    }
}

fn parse_translation(response_text: &str) -> Result<String, String> {
    let response: CompletionResponse = serde_json::from_str(response_text)
        .map_err(|error| format!("JSON parse failed: {error}"))?;
    let content = response.content.trim().to_string();
    if content.is_empty() {
        return Err("completion returned empty content".to_string());
    }
    Ok(content)
}

fn llama_cpp_error_message(response_text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(response_text).ok()?;
    let error = value.get("error")?;
    let message = error
        .get("message")
        .and_then(|message| message.as_str())
        .or_else(|| error.as_str())?;
    let message = message.trim();
    (!message.is_empty()).then(|| message.to_string())
}

fn log_llama_rwkv_io(
    debug_context: Option<&str>,
    endpoint: &str,
    target_lang: &str,
    inputs: &[&str],
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
        provider: "llama-cpp-chat-completions",
        context: debug_context,
        endpoint: Some(endpoint),
        source_lang: None,
        target_lang: Some(target_lang),
        status_code,
        ok,
        error,
        inputs: inputs.to_vec(),
        outputs: translations.iter().map(String::as_str).collect(),
        raw_response,
    });
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
        _ => "Chinese",
    }
}

fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn loopback_client(timeout_ms: u64) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
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
            .map_err(|error| format!("llama.cpp HTTP 请求失败: {error}"));
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
                Ok(Err(error)) => Err(format!("llama.cpp HTTP 请求失败: {error}")),
                Err(error) => Err(format!("llama.cpp HTTP 请求任务失败: {error}")),
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
            .map_err(|error| format!("无法读取 llama.cpp 响应: {error}"));
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
                Ok(Err(error)) => Err(format!("无法读取 llama.cpp 响应: {error}")),
                Err(error) => Err(format!("llama.cpp 响应读取任务失败: {error}")),
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
    fn parse_translation_reads_completion_response() {
        let response = json!({
            "content": "  你好，世界。  ",
            "stop": true
        });
        let parsed = parse_translation(&response.to_string()).expect("should parse");
        assert_eq!(parsed, "你好，世界。");
    }

    #[test]
    fn parse_translation_rejects_empty_content() {
        let response = json!({
            "content": "   ",
            "stop": true
        });
        let error = parse_translation(&response.to_string()).expect_err("should reject");
        assert!(error.contains("empty content"));
    }

    #[test]
    fn parse_translation_rejects_non_json() {
        let error = parse_translation("not json").expect_err("should reject");
        assert!(error.contains("JSON parse failed"));
    }

    #[test]
    fn completion_request_uses_role_based_prompt() {
        let request = build_completion_request("Hello.", "en", "zh-CN");
        assert_eq!(request.prompt, "English: Hello.\n\nChinese:");
        assert_eq!(request.n_predict, MAX_TOKENS_PER_SEGMENT);
        assert!(!request.stream);
    }

    #[test]
    fn completion_request_reverse_direction() {
        let request = build_completion_request("你好世界", "zh-CN", "en");
        assert_eq!(request.prompt, "Chinese: 你好世界\n\nEnglish:");
    }

    #[test]
    fn extracts_llama_cpp_error_message() {
        let response = json!({
            "error": {
                "message": "request (393 tokens) exceeds the available context size (256 tokens)"
            }
        });

        assert_eq!(
            llama_cpp_error_message(&response.to_string()).as_deref(),
            Some("request (393 tokens) exceeds the available context size (256 tokens)")
        );
    }

    #[test]
    fn join_url_does_not_double_slash() {
        assert_eq!(
            join_url("http://127.0.0.1:8765", "/completion"),
            "http://127.0.0.1:8765/completion"
        );
    }
}
