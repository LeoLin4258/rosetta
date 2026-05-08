use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const PROBE_TEXTS: [&str; 2] = [
    "After a blissful two weeks, Jane encounters Rochester in the gardens.",
    "That night, a bolt of lightning splits the same chestnut tree.",
];
const RAW_RESPONSE_PREVIEW_CHARS: usize = 2_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvTranslationApiProbeRequest {
    base_url: String,
    endpoint: String,
    internal_token: String,
    body_password: String,
    timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvTranslationApiTranslateRequest {
    base_url: String,
    endpoint: String,
    internal_token: String,
    body_password: String,
    timeout_ms: u64,
    source_texts: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvTranslationApiProbeResult {
    ok: bool,
    status_code: Option<u16>,
    translations: Vec<String>,
    raw_response_preview: String,
    message: String,
    latency_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvTranslationApiTranslateResult {
    ok: bool,
    status_code: Option<u16>,
    translations: Vec<String>,
    raw_response_preview: String,
    message: String,
    latency_ms: u128,
}

#[derive(Debug, Serialize)]
struct RwkvChatCompletionsRequest {
    contents: Vec<String>,
    max_tokens: u32,
    stop_tokens: Vec<u32>,
    temperature: f64,
    top_k: u32,
    top_p: f64,
    alpha_presence: f64,
    alpha_frequency: f64,
    alpha_decay: f64,
    stream: bool,
    password: String,
}

#[derive(Debug, Deserialize)]
struct RwkvChatCompletionsResponse {
    choices: Vec<RwkvChatCompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct RwkvChatCompletionChoice {
    index: usize,
    message: RwkvChatCompletionMessage,
}

#[derive(Debug, Deserialize)]
struct RwkvChatCompletionMessage {
    content: String,
}

#[tauri::command]
pub async fn probe_rwkv_translation_api(
    request: RwkvTranslationApiProbeRequest,
) -> Result<RwkvTranslationApiProbeResult, String> {
    Ok(probe_translation_api(request).await)
}

#[tauri::command]
pub async fn translate_rwkv_texts_with_api(
    request: RwkvTranslationApiTranslateRequest,
) -> Result<RwkvTranslationApiTranslateResult, String> {
    Ok(translate_texts_with_api(request).await)
}

async fn probe_translation_api(
    request: RwkvTranslationApiProbeRequest,
) -> RwkvTranslationApiProbeResult {
    let result = request_translations(
        &request.base_url,
        &request.endpoint,
        &request.internal_token,
        &request.body_password,
        request.timeout_ms,
        &PROBE_TEXTS
            .iter()
            .map(|text| text.to_string())
            .collect::<Vec<_>>(),
    )
    .await;

    RwkvTranslationApiProbeResult {
        ok: result.ok,
        status_code: result.status_code,
        translations: result.translations,
        raw_response_preview: result.raw_response_preview,
        message: if result.ok {
            "RWKV API 非流式批量翻译探测成功。".to_string()
        } else {
            result.message
        },
        latency_ms: result.latency_ms,
    }
}

async fn translate_texts_with_api(
    request: RwkvTranslationApiTranslateRequest,
) -> RwkvTranslationApiTranslateResult {
    if request.source_texts.is_empty() {
        return RwkvTranslationApiTranslateResult {
            ok: true,
            status_code: None,
            translations: Vec::new(),
            raw_response_preview: String::new(),
            message: "没有需要翻译的文本。".to_string(),
            latency_ms: 0,
        };
    }

    request_translations(
        &request.base_url,
        &request.endpoint,
        &request.internal_token,
        &request.body_password,
        request.timeout_ms,
        &request.source_texts,
    )
    .await
}

async fn request_translations(
    base_url: &str,
    endpoint: &str,
    internal_token: &str,
    body_password: &str,
    timeout_ms: u64,
    source_texts: &[String],
) -> RwkvTranslationApiTranslateResult {
    let started_at = Instant::now();
    let url = api_url(base_url, endpoint);
    let body = build_chat_completions_request(source_texts, body_password);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return translation_error(
                None,
                "",
                internal_token,
                body_password,
                format!("无法创建 RWKV API client: {error}"),
                started_at,
            );
        }
    };

    let response = client
        .post(url)
        .header("X-Internal-Token", internal_token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "*/*")
        .json(&body)
        .send()
        .await;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return translation_error(
                None,
                "",
                internal_token,
                body_password,
                format!("RWKV API 请求失败: {error}"),
                started_at,
            );
        }
    };

    let status_code = response.status().as_u16();
    let response_text = match response.text().await {
        Ok(response_text) => response_text,
        Err(error) => {
            return translation_error(
                Some(status_code),
                "",
                internal_token,
                body_password,
                format!("无法读取 RWKV API 响应: {error}"),
                started_at,
            );
        }
    };

    if !(200..300).contains(&status_code) {
        return translation_error(
            Some(status_code),
            &response_text,
            internal_token,
            body_password,
            format!("RWKV API 返回 HTTP {status_code}。"),
            started_at,
        );
    }

    match parse_translations(&response_text, source_texts.len()) {
        Ok(translations) => RwkvTranslationApiTranslateResult {
            ok: true,
            status_code: Some(status_code),
            translations,
            raw_response_preview: preview_text_with_redactions(
                &response_text,
                internal_token,
                body_password,
            ),
            message: format!("RWKV API 已翻译 {} 条文本。", source_texts.len()),
            latency_ms: started_at.elapsed().as_millis(),
        },
        Err(error) => translation_error(
            Some(status_code),
            &response_text,
            internal_token,
            body_password,
            format!("RWKV API 响应格式不可用: {error}"),
            started_at,
        ),
    }
}

fn build_chat_completions_request(
    source_texts: &[String],
    password: &str,
) -> RwkvChatCompletionsRequest {
    RwkvChatCompletionsRequest {
        contents: source_texts
            .iter()
            .map(|text| translation_prompt(text))
            .collect(),
        max_tokens: 1024,
        stop_tokens: vec![0, 261, 24281],
        temperature: 0.8,
        top_k: 50,
        top_p: 0.6,
        alpha_presence: 1.0,
        alpha_frequency: 0.1,
        alpha_decay: 0.99,
        stream: false,
        password: password.to_string(),
    }
}

fn translation_prompt(source_text: &str) -> String {
    format!("English: {source_text}\n\nChinese:")
}

fn parse_translations(response_text: &str, expected_count: usize) -> Result<Vec<String>, String> {
    let response: RwkvChatCompletionsResponse = serde_json::from_str(response_text)
        .map_err(|error| format!("JSON parse failed: {error}"))?;
    let mut translations: Vec<Option<String>> = vec![None; expected_count];

    for choice in response.choices {
        if choice.index >= expected_count {
            continue;
        }

        let content = choice.message.content.trim().to_string();
        if content.is_empty() {
            return Err(format!("choice {} returned empty content", choice.index));
        }

        translations[choice.index] = Some(content);
    }

    translations
        .into_iter()
        .enumerate()
        .map(|(index, translation)| {
            translation.ok_or_else(|| format!("missing translation for choice index {index}"))
        })
        .collect()
}

fn api_url(base_url: &str, endpoint: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        endpoint.trim_start_matches('/')
    )
}

fn translation_error(
    status_code: Option<u16>,
    response_text: &str,
    internal_token: &str,
    body_password: &str,
    message: String,
    started_at: Instant,
) -> RwkvTranslationApiTranslateResult {
    RwkvTranslationApiTranslateResult {
        ok: false,
        status_code,
        translations: Vec::new(),
        raw_response_preview: preview_text_with_redactions(
            response_text,
            internal_token,
            body_password,
        ),
        message,
        latency_ms: started_at.elapsed().as_millis(),
    }
}

fn preview_text_with_redactions(text: &str, internal_token: &str, body_password: &str) -> String {
    redact_sensitive_values(text, &[internal_token, body_password])
        .chars()
        .take(RAW_RESPONSE_PREVIEW_CHARS)
        .collect()
}

fn redact_sensitive_values(text: &str, sensitive_values: &[&str]) -> String {
    sensitive_values
        .iter()
        .fold(text.to_string(), |redacted, value| {
            if value.is_empty() {
                redacted
            } else {
                redacted.replace(value, "<redacted>")
            }
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn prompt_builder_wraps_english_text_for_chinese_translation() {
        let source_texts = vec!["Hello world.".to_string(), "Good morning.".to_string()];
        let request = build_chat_completions_request(&source_texts, "secret");

        assert_eq!(
            request.contents,
            vec![
                "English: Hello world.\n\nChinese:".to_string(),
                "English: Good morning.\n\nChinese:".to_string()
            ]
        );
    }

    #[test]
    fn request_body_serializes_non_streaming_batch_shape() {
        let source_texts = vec!["Hello world.".to_string()];
        let request = build_chat_completions_request(&source_texts, "model-password");
        let value = serde_json::to_value(request).expect("request should serialize");

        assert_eq!(
            value["contents"],
            json!(["English: Hello world.\n\nChinese:"])
        );
        assert_eq!(value["max_tokens"], json!(1024));
        assert_eq!(value["stop_tokens"], json!([0, 261, 24281]));
        assert_eq!(value["temperature"], json!(0.8));
        assert_eq!(value["top_k"], json!(50));
        assert_eq!(value["top_p"], json!(0.6));
        assert_eq!(value["alpha_presence"], json!(1.0));
        assert_eq!(value["alpha_frequency"], json!(0.1));
        assert_eq!(value["alpha_decay"], json!(0.99));
        assert_eq!(value["stream"], json!(false));
        assert_eq!(value["password"], json!("model-password"));
    }

    #[test]
    fn response_parser_restores_choice_index_order() {
        let response = json!({
            "choices": [
                {"index": 1, "message": {"content": " 第二段 "}},
                {"index": 0, "message": {"content": " 第一段 "}}
            ]
        });

        let translations =
            parse_translations(&response.to_string(), 2).expect("translations should parse");

        assert_eq!(
            translations,
            vec!["第一段".to_string(), "第二段".to_string()]
        );
    }

    #[test]
    fn response_parser_rejects_missing_choice() {
        let response = json!({
            "choices": [
                {"index": 0, "message": {"content": "第一段"}}
            ]
        });

        let error = parse_translations(&response.to_string(), 2)
            .expect_err("missing translation should fail");

        assert!(error.contains("missing translation for choice index 1"));
    }

    #[test]
    fn response_parser_rejects_empty_content() {
        let response = json!({
            "choices": [
                {"index": 0, "message": {"content": "   "}}
            ]
        });

        let error = parse_translations(&response.to_string(), 1)
            .expect_err("empty translation should fail");

        assert!(error.contains("empty content"));
    }

    #[test]
    fn response_parser_rejects_non_json() {
        let error = parse_translations("not json", 1).expect_err("non json should fail");

        assert!(error.contains("JSON parse failed"));
    }

    #[test]
    fn api_url_joins_base_and_endpoint() {
        assert_eq!(
            api_url("https://example.com/", "/v1/chat/completions"),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn error_preview_does_not_include_request_token() {
        let result = translation_error(
            Some(500),
            r#"{"error":"sensitive-token sensitive-password"}"#,
            "sensitive-token",
            "sensitive-password",
            "RWKV API 返回 HTTP 500。".to_string(),
            Instant::now(),
        );

        assert!(!result.raw_response_preview.contains("sensitive-token"));
        assert!(!result.raw_response_preview.contains("sensitive-password"));
        assert!(result.raw_response_preview.contains("<redacted>"));
    }
}
