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
const DEFAULT_TEMPERATURE: f64 = 0.25;
const DEFAULT_TOP_K: u32 = 20;
const DEFAULT_TOP_P: f64 = 0.9;
const DEFAULT_MIN_P: f64 = 0.05;
const DEFAULT_REPEAT_PENALTY: f64 = 1.18;
const DEFAULT_REPEAT_LAST_N: u32 = 192;
const DEFAULT_PENALIZE_NL: bool = false;

pub const DEFAULT_PARALLEL_REQUESTS: usize = 16;
/// Total llama-server context for all slots. llama.cpp divides this across
/// `--parallel` slots, so 16384 / 16 gives each concurrent PDF request about
/// 1024 tokens. That is the current strict-correct PDF baseline for the
/// Windows llama.cpp Vulkan runtime.
pub const DEFAULT_SERVER_CTX_SIZE: usize = 16384;
pub const MANAGED_SERVER_CTX_SIZE_ENV: &str = "ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE";
pub const MANAGED_PARALLEL_REQUESTS_ENV: &str = "ROSETTA_MANAGED_LLAMA_CPP_PARALLEL";
pub const GENERATION_TEMPERATURE_ENV: &str = "ROSETTA_LLAMA_CPP_TEMPERATURE";
pub const GENERATION_TOP_K_ENV: &str = "ROSETTA_LLAMA_CPP_TOP_K";
pub const GENERATION_TOP_P_ENV: &str = "ROSETTA_LLAMA_CPP_TOP_P";
pub const GENERATION_MIN_P_ENV: &str = "ROSETTA_LLAMA_CPP_MIN_P";
pub const GENERATION_REPEAT_PENALTY_ENV: &str = "ROSETTA_LLAMA_CPP_REPEAT_PENALTY";
pub const GENERATION_REPEAT_LAST_N_ENV: &str = "ROSETTA_LLAMA_CPP_REPEAT_LAST_N";
pub const GENERATION_N_PREDICT_ENV: &str = "ROSETTA_LLAMA_CPP_N_PREDICT";
pub const PROBE_TEXTS: [&str; 2] = [
    "After a blissful two weeks, Jane encounters Rochester in the gardens.",
    "That night, a bolt of lightning splits the same chestnut tree.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedLlamaCppRuntimeSettings {
    pub server_ctx_size: usize,
    pub parallel_requests: usize,
}

impl Default for ManagedLlamaCppRuntimeSettings {
    fn default() -> Self {
        Self {
            server_ctx_size: DEFAULT_SERVER_CTX_SIZE,
            parallel_requests: DEFAULT_PARALLEL_REQUESTS,
        }
    }
}

pub fn managed_runtime_settings_from_env() -> ManagedLlamaCppRuntimeSettings {
    let defaults = ManagedLlamaCppRuntimeSettings::default();
    ManagedLlamaCppRuntimeSettings {
        server_ctx_size: managed_runtime_usize_env(
            MANAGED_SERVER_CTX_SIZE_ENV,
            defaults.server_ctx_size,
        ),
        parallel_requests: managed_runtime_usize_env(
            MANAGED_PARALLEL_REQUESTS_ENV,
            defaults.parallel_requests,
        ),
    }
}

fn managed_runtime_usize_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .as_deref()
        .and_then(parse_positive_usize_override)
        .unwrap_or(default)
}

fn parse_positive_usize_override(raw: &str) -> Option<usize> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<usize>().ok().filter(|value| *value > 0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LlamaCppGenerationSettings {
    n_predict: u32,
    temperature: f64,
    top_k: u32,
    top_p: f64,
    min_p: f64,
    repeat_penalty: f64,
    repeat_last_n: u32,
    penalize_nl: bool,
}

impl Default for LlamaCppGenerationSettings {
    fn default() -> Self {
        Self {
            n_predict: MAX_TOKENS_PER_SEGMENT,
            temperature: DEFAULT_TEMPERATURE,
            top_k: DEFAULT_TOP_K,
            top_p: DEFAULT_TOP_P,
            min_p: DEFAULT_MIN_P,
            repeat_penalty: DEFAULT_REPEAT_PENALTY,
            repeat_last_n: DEFAULT_REPEAT_LAST_N,
            penalize_nl: DEFAULT_PENALIZE_NL,
        }
    }
}

fn generation_settings_from_env() -> LlamaCppGenerationSettings {
    let defaults = LlamaCppGenerationSettings::default();
    LlamaCppGenerationSettings {
        n_predict: generation_u32_env(GENERATION_N_PREDICT_ENV, defaults.n_predict),
        temperature: generation_f64_env(
            GENERATION_TEMPERATURE_ENV,
            defaults.temperature,
            |value| (0.0..=2.0).contains(&value),
        ),
        top_k: generation_u32_env(GENERATION_TOP_K_ENV, defaults.top_k),
        top_p: generation_f64_env(GENERATION_TOP_P_ENV, defaults.top_p, |value| {
            (0.0..=1.0).contains(&value)
        }),
        min_p: generation_f64_env(GENERATION_MIN_P_ENV, defaults.min_p, |value| {
            (0.0..=1.0).contains(&value)
        }),
        repeat_penalty: generation_f64_env(
            GENERATION_REPEAT_PENALTY_ENV,
            defaults.repeat_penalty,
            |value| value > 0.0,
        ),
        repeat_last_n: generation_u32_env(GENERATION_REPEAT_LAST_N_ENV, defaults.repeat_last_n),
        penalize_nl: defaults.penalize_nl,
    }
}

fn generation_u32_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .as_deref()
        .and_then(parse_positive_u32_override)
        .unwrap_or(default)
}

fn generation_f64_env(name: &str, default: f64, is_valid: impl Fn(f64) -> bool) -> f64 {
    std::env::var(name)
        .ok()
        .as_deref()
        .and_then(|raw| parse_f64_override(raw, &is_valid))
        .unwrap_or(default)
}

fn parse_positive_u32_override(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u32>().ok().filter(|value| *value > 0)
}

fn parse_f64_override(raw: &str, is_valid: impl Fn(f64) -> bool) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && is_valid(*value))
}

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
    top_k: u32,
    top_p: f64,
    min_p: f64,
    repeat_penalty: f64,
    repeat_last_n: u32,
    penalize_nl: bool,
    stop: Vec<String>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    #[serde(default)]
    content: String,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    stop_type: Option<String>,
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
    let generation = generation_settings_from_env();
    CompletionRequest {
        prompt: format!("{source_label}: {source_text}\n\n{target_label}:"),
        n_predict: generation.n_predict,
        temperature: generation.temperature,
        top_k: generation.top_k,
        top_p: generation.top_p,
        min_p: generation.min_p,
        repeat_penalty: generation.repeat_penalty,
        repeat_last_n: generation.repeat_last_n,
        penalize_nl: generation.penalize_nl,
        stop: translation_stop_sequences(source_label, target_label),
        stream: false,
    }
}

fn translation_stop_sequences(source_label: &str, target_label: &str) -> Vec<String> {
    [
        format!("\n\n{source_label}:"),
        format!("\n{source_label}:"),
        format!("\n\n{target_label}:"),
        format!("\n{target_label}:"),
    ]
    .into_iter()
    .collect()
}

fn parse_translation(response_text: &str) -> Result<String, String> {
    let response: CompletionResponse = serde_json::from_str(response_text)
        .map_err(|error| format!("JSON parse failed: {error}"))?;
    let content = response.content.trim().to_string();
    if response.truncated || response.stop_type.as_deref() == Some("limit") {
        return Err(format!(
            "llama.cpp completion was truncated (truncated={}, stop_type={})",
            response.truncated,
            response.stop_type.as_deref().unwrap_or("unknown")
        ));
    }
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
    fn managed_runtime_override_parser_accepts_positive_numbers() {
        assert_eq!(parse_positive_usize_override("16384"), Some(16384));
        assert_eq!(parse_positive_usize_override(" 8 "), Some(8));
    }

    #[test]
    fn managed_runtime_override_parser_rejects_empty_zero_and_invalid_values() {
        assert_eq!(parse_positive_usize_override(""), None);
        assert_eq!(parse_positive_usize_override("0"), None);
        assert_eq!(parse_positive_usize_override("-1"), None);
        assert_eq!(parse_positive_usize_override("eight"), None);
    }

    #[test]
    fn parse_translation_rejects_truncated_completion() {
        let response = json!({
            "content": "半截译文",
            "truncated": true,
            "stop_type": "limit"
        });

        let error = parse_translation(&response.to_string()).expect_err("should reject");

        assert!(error.contains("truncated=true"));
        assert!(error.contains("stop_type=limit"));
    }

    #[test]
    fn parse_translation_rejects_limit_stop_type() {
        let response = json!({
            "content": "半截译文",
            "truncated": false,
            "stop_type": "limit"
        });

        let error = parse_translation(&response.to_string()).expect_err("should reject");

        assert!(error.contains("stop_type=limit"));
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
        assert_eq!(request.temperature, DEFAULT_TEMPERATURE);
        assert_eq!(request.top_k, DEFAULT_TOP_K);
        assert_eq!(request.top_p, DEFAULT_TOP_P);
        assert_eq!(request.min_p, DEFAULT_MIN_P);
        assert_eq!(request.repeat_penalty, DEFAULT_REPEAT_PENALTY);
        assert_eq!(request.repeat_last_n, DEFAULT_REPEAT_LAST_N);
        assert_eq!(request.penalize_nl, DEFAULT_PENALIZE_NL);
        assert_eq!(
            request.stop,
            vec![
                "\n\nEnglish:".to_string(),
                "\nEnglish:".to_string(),
                "\n\nChinese:".to_string(),
                "\nChinese:".to_string(),
            ]
        );
        assert!(!request.stream);
    }

    #[test]
    fn completion_request_reverse_direction() {
        let request = build_completion_request("你好世界", "zh-CN", "en");
        assert_eq!(request.prompt, "Chinese: 你好世界\n\nEnglish:");
    }

    #[test]
    fn generation_override_parser_accepts_valid_values() {
        assert_eq!(parse_positive_u32_override("1024"), Some(1024));
        assert_eq!(parse_positive_u32_override(" 16 "), Some(16));
        assert_eq!(parse_f64_override("0.25", |value| value > 0.0), Some(0.25));
        assert_eq!(
            parse_f64_override(" 1.18 ", |value| value > 0.0),
            Some(1.18)
        );
    }

    #[test]
    fn generation_override_parser_rejects_empty_zero_invalid_and_out_of_range_values() {
        assert_eq!(parse_positive_u32_override(""), None);
        assert_eq!(parse_positive_u32_override("0"), None);
        assert_eq!(parse_positive_u32_override("-1"), None);
        assert_eq!(parse_positive_u32_override("many"), None);
        assert_eq!(parse_f64_override("", |value| value > 0.0), None);
        assert_eq!(parse_f64_override("0", |value| value > 0.0), None);
        assert_eq!(parse_f64_override("nope", |value| value > 0.0), None);
        assert_eq!(
            parse_f64_override("1.5", |value| (0.0..=1.0).contains(&value)),
            None
        );
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
