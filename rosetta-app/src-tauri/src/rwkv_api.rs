use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::rosetta_jobs::store::{read_translation_files, write_translation_files};
use crate::rosetta_jobs::{
    model::{Segment, TranslationSegment},
    path::{checked_job_dir, jobs_root},
    store::read_json,
    translation_files::{
        build_translation_file, read_translation_segments, write_translation_segments,
    },
};
use crate::rwkv_providers::{
    llama_cpp_chat::{self, LlamaCppChatConfig},
    mobile_batch_chat::{self, MobileBatchChatConfig},
    ProviderTranslateBatch, ProviderTranslateResult,
};

const PROBE_TEXTS: [&str; 2] = [
    "After a blissful two weeks, Jane encounters Rochester in the gardens.",
    "That night, a bolt of lightning splits the same chestnut tree.",
];
const RAW_RESPONSE_PREVIEW_CHARS: usize = 2_000;
const RUN_POLL_INTERVAL_MS: u64 = 50;

#[derive(Default)]
pub struct RwkvTranslationRunRegistry {
    runs: Mutex<HashMap<String, RwkvTranslationRunRecord>>,
}

struct RwkvTranslationRunRecord {
    cancel: Arc<AtomicBool>,
    status: RwkvTranslationRunStatus,
}

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
    source_lang: Option<String>,
    target_lang: Option<String>,
    source_texts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvTranslationRunStartRequest {
    run_id: String,
    job_id: String,
    translation_file_id: String,
    source_segment_ids: Vec<String>,
    base_url: String,
    endpoint: String,
    internal_token: String,
    body_password: String,
    timeout_ms: u64,
    source_lang: Option<String>,
    target_lang: String,
    batch_size: usize,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum RwkvTranslationRunState {
    Running,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RwkvTranslationRunStatus {
    run_id: String,
    job_id: String,
    translation_file_id: String,
    state: RwkvTranslationRunState,
    completed_segment_ids: Vec<String>,
    failed_segment_ids: Vec<String>,
    message: String,
    translation_file: Option<crate::rosetta_jobs::model::RosettaTranslationFile>,
    segments: Option<Vec<TranslationSegment>>,
}

#[derive(Debug, Serialize)]
struct RwkvChatCompletionsRequest {
    contents: Vec<String>,
    max_tokens: u32,
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
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct RwkvStreamingChunk {
    choices: Vec<RwkvStreamingChoice>,
}

#[derive(Debug, Deserialize)]
struct RwkvStreamingChoice {
    index: usize,
    #[serde(default)]
    delta: Option<RwkvChatCompletionMessage>,
    #[serde(default)]
    message: Option<RwkvChatCompletionMessage>,
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

#[tauri::command]
pub async fn start_rwkv_translation_run(
    app: AppHandle,
    registry: State<'_, RwkvTranslationRunRegistry>,
    request: RwkvTranslationRunStartRequest,
) -> Result<RwkvTranslationRunStatus, String> {
    start_translation_run(app, registry.inner(), request).await
}

#[tauri::command]
pub fn cancel_rwkv_translation_run(
    registry: State<'_, RwkvTranslationRunRegistry>,
    run_id: String,
) -> Result<RwkvTranslationRunStatus, String> {
    let mut runs = registry
        .runs
        .lock()
        .map_err(|_| "翻译运行状态锁不可用。".to_string())?;
    let Some(record) = runs.get_mut(&run_id) else {
        eprintln!(
            "[rwkv-cancel] cancel_rwkv_translation_run: run_id={run_id} NOT FOUND in registry"
        );
        return Err("翻译运行不存在。".to_string());
    };

    eprintln!(
        "[rwkv-cancel] cancel_rwkv_translation_run: setting flag for run_id={run_id} (was state={:?})",
        record.status.state
    );
    record.cancel.store(true, Ordering::SeqCst);
    if matches!(record.status.state, RwkvTranslationRunState::Running) {
        record.status.state = RwkvTranslationRunState::Cancelling;
        record.status.message = "正在停止翻译。".to_string();
    }
    Ok(record.status.clone())
}

#[tauri::command]
pub fn get_rwkv_translation_run_status(
    registry: State<'_, RwkvTranslationRunRegistry>,
    run_id: String,
) -> Result<RwkvTranslationRunStatus, String> {
    let runs = registry
        .runs
        .lock()
        .map_err(|_| "翻译运行状态锁不可用。".to_string())?;
    runs.get(&run_id)
        .map(|record| record.status.clone())
        .ok_or_else(|| "翻译运行不存在。".to_string())
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
            "RWKV API 批量翻译探测成功。".to_string()
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

    request_translations_for_language_pair(
        &request.base_url,
        &request.endpoint,
        &request.internal_token,
        &request.body_password,
        request.timeout_ms,
        request.source_lang.as_deref().unwrap_or("en"),
        request.target_lang.as_deref().unwrap_or("zh-CN"),
        &request.source_texts,
        Some("lightning-adhoc"),
    )
    .await
}

async fn start_translation_run(
    app: AppHandle,
    registry: &RwkvTranslationRunRegistry,
    request: RwkvTranslationRunStartRequest,
) -> Result<RwkvTranslationRunStatus, String> {
    if request.run_id.trim().is_empty() {
        return Err("翻译运行 id 不能为空。".to_string());
    }
    if request.batch_size == 0 {
        return Err("翻译批次大小必须大于 0。".to_string());
    }

    let root = jobs_root(&app)?;
    let dir = checked_job_dir(&root, &request.job_id)?;
    let source_segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut translation_segments = read_translation_segments(&dir, &request.translation_file_id)?;
    let source_segment_ids = request
        .source_segment_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let targets = source_segments
        .iter()
        .filter(|segment| source_segment_ids.contains(&segment.id))
        .filter(|segment| segment.status != "skipped" && !segment.source_text.trim().is_empty())
        .filter(|segment| {
            translation_segments
                .iter()
                .find(|translation| translation.source_segment_id == segment.id)
                .is_some_and(|translation| translation.status != "skipped")
        })
        .cloned()
        .collect::<Vec<_>>();

    let cancel = Arc::new(AtomicBool::new(false));
    let initial_bundle = save_run_segments(
        &dir,
        &request.translation_file_id,
        &request.target_lang,
        translation_segments.clone(),
    )?;
    let initial_status = RwkvTranslationRunStatus {
        run_id: request.run_id.clone(),
        job_id: request.job_id.clone(),
        translation_file_id: request.translation_file_id.clone(),
        state: RwkvTranslationRunState::Running,
        completed_segment_ids: Vec::new(),
        failed_segment_ids: Vec::new(),
        message: if targets.is_empty() {
            "没有需要翻译的文本。".to_string()
        } else {
            "翻译运行已开始。".to_string()
        },
        translation_file: Some(initial_bundle.0),
        segments: Some(initial_bundle.1),
    };
    {
        let mut runs = registry
            .runs
            .lock()
            .map_err(|_| "翻译运行状态锁不可用。".to_string())?;
        runs.insert(
            request.run_id.clone(),
            RwkvTranslationRunRecord {
                cancel: cancel.clone(),
                status: initial_status.clone(),
            },
        );
    }

    if targets.is_empty() {
        let status = update_run_status(registry, &request.run_id, |status| {
            status.state = RwkvTranslationRunState::Completed;
            status.message = "没有需要翻译的文本。".to_string();
        })?;
        return Ok(status);
    }

    let mut completed_segment_ids = Vec::new();
    let mut failed_segment_ids = Vec::new();

    for batch in targets.chunks(request.batch_size) {
        if cancel.load(Ordering::SeqCst) {
            let status = cancel_current_run(
                registry,
                &dir,
                &request.run_id,
                &request.translation_file_id,
                &request.target_lang,
                &mut translation_segments,
                &[],
            )?;
            return Ok(status);
        }

        let batch_ids = batch
            .iter()
            .map(|segment| segment.id.clone())
            .collect::<Vec<_>>();
        mark_translation_segments_translating(&mut translation_segments, &batch_ids);
        let bundle = save_run_segments(
            &dir,
            &request.translation_file_id,
            &request.target_lang,
            translation_segments.clone(),
        )?;
        update_run_status(registry, &request.run_id, |status| {
            status.state = RwkvTranslationRunState::Running;
            status.message = "正在翻译当前批次。".to_string();
            status.translation_file = Some(bundle.0.clone());
            status.segments = Some(bundle.1.clone());
        })?;

        let source_texts = batch
            .iter()
            .map(|segment| segment.source_text.clone())
            .collect::<Vec<_>>();
        let debug_context = format!(
            "lightning-run:{}:{}:{}",
            request.run_id, request.job_id, request.translation_file_id
        );
        let result = request_translations_for_language_pair_with_cancel(
            &request.base_url,
            &request.endpoint,
            &request.internal_token,
            &request.body_password,
            request.timeout_ms,
            request.source_lang.as_deref().unwrap_or("en"),
            &request.target_lang,
            &source_texts,
            Some(cancel.clone()),
            Some(&debug_context),
        )
        .await;

        if cancel.load(Ordering::SeqCst) {
            let status = cancel_current_run(
                registry,
                &dir,
                &request.run_id,
                &request.translation_file_id,
                &request.target_lang,
                &mut translation_segments,
                &batch_ids,
            )?;
            return Ok(status);
        }

        if !result.ok || result.translations.len() != batch.len() {
            let message = if result.ok {
                format!(
                    "RWKV API 返回 {} 条译文，但本批有 {} 条文本。",
                    result.translations.len(),
                    batch.len()
                )
            } else {
                result.message
            };
            mark_translation_segments_failed(&mut translation_segments, &batch_ids, &message);
            failed_segment_ids.extend(batch_ids.clone());
            let bundle = save_run_segments(
                &dir,
                &request.translation_file_id,
                &request.target_lang,
                translation_segments.clone(),
            )?;
            let status = update_run_status(registry, &request.run_id, |status| {
                status.state = RwkvTranslationRunState::Failed;
                status.failed_segment_ids = failed_segment_ids.clone();
                status.message = message.clone();
                status.translation_file = Some(bundle.0.clone());
                status.segments = Some(bundle.1.clone());
            })?;
            return Ok(status);
        }

        mark_translation_segments_done(&mut translation_segments, &batch_ids, &result.translations);
        completed_segment_ids.extend(batch_ids);
        let bundle = save_run_segments(
            &dir,
            &request.translation_file_id,
            &request.target_lang,
            translation_segments.clone(),
        )?;
        update_run_status(registry, &request.run_id, |status| {
            status.completed_segment_ids = completed_segment_ids.clone();
            status.message = format!("已完成 {} 段。", status.completed_segment_ids.len());
            status.translation_file = Some(bundle.0.clone());
            status.segments = Some(bundle.1.clone());
        })?;
    }

    update_run_status(registry, &request.run_id, |status| {
        status.state = RwkvTranslationRunState::Completed;
        status.completed_segment_ids = completed_segment_ids;
        status.failed_segment_ids = failed_segment_ids;
        status.message = "翻译运行完成。".to_string();
    })
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
    let prepare_started = Instant::now();
    let body = build_chat_completions_request(source_texts, body_password, "en", "zh-CN");
    let prepare_request_ms = prepare_started.elapsed().as_millis() as u64;
    let source_lang = "en";
    let target_lang = "zh-CN";
    let debug_context = Some("lightning-probe");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            let message = format!("unable to create reqwest client: {error}");
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                &[],
                None,
                false,
                Some(&message),
                prepare_request_ms,
                0,
                0,
                0,
                started_at.elapsed().as_millis() as u64,
            );
            return translation_error(
                None,
                "",
                internal_token,
                body_password,
                true,
                format!("无法创建 RWKV API client: {error}"),
                started_at,
            );
        }
    };

    let http_started = Instant::now();
    let response = client
        .post(&url)
        .header("X-Internal-Token", internal_token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "*/*")
        .json(&body)
        .send()
        .await;
    let http_send_ms = http_started.elapsed().as_millis() as u64;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let message = format!("request failed: {error}");
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                &[],
                None,
                false,
                Some(&message),
                prepare_request_ms,
                http_send_ms,
                0,
                0,
                started_at.elapsed().as_millis() as u64,
            );
            return translation_error(
                None,
                "",
                internal_token,
                body_password,
                true,
                format!("RWKV API 请求失败: {error}"),
                started_at,
            );
        }
    };

    let status_code = response.status().as_u16();
    let response_read_started = Instant::now();
    let response_text = match response.text().await {
        Ok(response_text) => response_text,
        Err(error) => {
            let message = format!("unable to read response: {error}");
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                &[],
                Some(status_code),
                false,
                Some(&message),
                prepare_request_ms,
                http_send_ms,
                response_read_started.elapsed().as_millis() as u64,
                0,
                started_at.elapsed().as_millis() as u64,
            );
            return translation_error(
                Some(status_code),
                "",
                internal_token,
                body_password,
                true,
                format!("无法读取 RWKV API 响应: {error}"),
                started_at,
            );
        }
    };
    let response_read_ms = response_read_started.elapsed().as_millis() as u64;

    if !(200..300).contains(&status_code) {
        log_lightning_rwkv_io(
            debug_context,
            &url,
            source_lang,
            target_lang,
            &body.contents,
            Some(status_code),
            false,
            Some("HTTP error"),
            &[],
            Some(&response_text),
        );
        log_lightning_perf(
            debug_context,
            &url,
            source_lang,
            target_lang,
            source_texts,
            &[],
            Some(status_code),
            false,
            Some("HTTP error"),
            prepare_request_ms,
            http_send_ms,
            response_read_ms,
            0,
            started_at.elapsed().as_millis() as u64,
        );
        return translation_error(
            Some(status_code),
            &response_text,
            internal_token,
            body_password,
            true,
            format!("RWKV API 返回 HTTP {status_code}。"),
            started_at,
        );
    }

    let parse_started = Instant::now();
    let parsed_translations = parse_translations(&response_text, source_texts.len());
    let response_parse_ms = parse_started.elapsed().as_millis() as u64;
    match &parsed_translations {
        Ok(translations) => {
            log_lightning_rwkv_io(
                debug_context,
                &url,
                source_lang,
                target_lang,
                &body.contents,
                Some(status_code),
                true,
                None,
                translations,
                Some(&response_text),
            );
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                translations,
                Some(status_code),
                true,
                None,
                prepare_request_ms,
                http_send_ms,
                response_read_ms,
                response_parse_ms,
                started_at.elapsed().as_millis() as u64,
            );
        }
        Err(error) => {
            log_lightning_rwkv_io(
                debug_context,
                &url,
                source_lang,
                target_lang,
                &body.contents,
                Some(status_code),
                false,
                Some(error),
                &[],
                Some(&response_text),
            );
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                &[],
                Some(status_code),
                false,
                Some(error),
                prepare_request_ms,
                http_send_ms,
                response_read_ms,
                response_parse_ms,
                started_at.elapsed().as_millis() as u64,
            );
        }
    }

    match parsed_translations {
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
            true,
            format!("RWKV API 响应格式不可用: {error}"),
            started_at,
        ),
    }
}

async fn request_translations_for_language_pair(
    base_url: &str,
    endpoint: &str,
    internal_token: &str,
    body_password: &str,
    timeout_ms: u64,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    debug_context: Option<&str>,
) -> RwkvTranslationApiTranslateResult {
    request_translations_for_language_pair_with_cancel(
        base_url,
        endpoint,
        internal_token,
        body_password,
        timeout_ms,
        source_lang,
        target_lang,
        source_texts,
        None,
        debug_context,
    )
    .await
}

async fn request_translations_for_language_pair_with_cancel(
    base_url: &str,
    endpoint: &str,
    internal_token: &str,
    body_password: &str,
    timeout_ms: u64,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    cancel: Option<Arc<AtomicBool>>,
    debug_context: Option<&str>,
) -> RwkvTranslationApiTranslateResult {
    let started_at = Instant::now();
    let url = api_url(base_url, endpoint);
    let prepare_started = Instant::now();
    let body =
        build_chat_completions_request(source_texts, body_password, source_lang, target_lang);
    let prepare_request_ms = prepare_started.elapsed().as_millis() as u64;
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            let message = format!("unable to create reqwest client: {error}");
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                &[],
                None,
                false,
                Some(&message),
                prepare_request_ms,
                0,
                0,
                0,
                started_at.elapsed().as_millis() as u64,
            );
            return translation_error(
                None,
                "",
                internal_token,
                body_password,
                false,
                format!("无法创建 RWKV API client: {error}"),
                started_at,
            );
        }
    };

    if is_cancelled(cancel.as_ref()) {
        log_lightning_perf(
            debug_context,
            &url,
            source_lang,
            target_lang,
            source_texts,
            &[],
            None,
            false,
            Some("cancelled before request"),
            prepare_request_ms,
            0,
            0,
            0,
            started_at.elapsed().as_millis() as u64,
        );
        return translation_error(
            None,
            "",
            internal_token,
            body_password,
            false,
            "RWKV API 请求已取消。".to_string(),
            started_at,
        );
    }

    let http_started = Instant::now();
    let response_future = client
        .post(&url)
        .header("X-Internal-Token", internal_token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "*/*")
        .json(&body)
        .send();
    let response: Result<reqwest::Response, reqwest::Error> = if let Some(cancel) = cancel.as_ref()
    {
        let handle = tokio::spawn(response_future);
        loop {
            if cancel.load(Ordering::SeqCst) {
                handle.abort();
                log_lightning_perf(
                    debug_context,
                    &url,
                    source_lang,
                    target_lang,
                    source_texts,
                    &[],
                    None,
                    false,
                    Some("cancelled during request"),
                    prepare_request_ms,
                    http_started.elapsed().as_millis() as u64,
                    0,
                    0,
                    started_at.elapsed().as_millis() as u64,
                );
                return translation_error(
                    None,
                    "",
                    internal_token,
                    body_password,
                    false,
                    "RWKV API 请求已取消。".to_string(),
                    started_at,
                );
            }
            if handle.is_finished() {
                break match handle.await {
                    Ok(response) => response,
                    Err(error) => {
                        let message = format!("request task failed: {error}");
                        log_lightning_perf(
                            debug_context,
                            &url,
                            source_lang,
                            target_lang,
                            source_texts,
                            &[],
                            None,
                            false,
                            Some(&message),
                            prepare_request_ms,
                            http_started.elapsed().as_millis() as u64,
                            0,
                            0,
                            started_at.elapsed().as_millis() as u64,
                        );
                        return translation_error(
                            None,
                            "",
                            internal_token,
                            body_password,
                            false,
                            format!("RWKV API 请求任务失败: {error}"),
                            started_at,
                        );
                    }
                };
            }
            tokio::time::sleep(Duration::from_millis(RUN_POLL_INTERVAL_MS)).await;
        }
    } else {
        response_future.await
    };
    let http_send_ms = http_started.elapsed().as_millis() as u64;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let message = format!("request failed: {error}");
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                &[],
                None,
                false,
                Some(&message),
                prepare_request_ms,
                http_send_ms,
                0,
                0,
                started_at.elapsed().as_millis() as u64,
            );
            return translation_error(
                None,
                "",
                internal_token,
                body_password,
                false,
                format!("RWKV API 请求失败: {error}"),
                started_at,
            );
        }
    };

    let status_code = response.status().as_u16();
    let response_read_started = Instant::now();
    let response_text = match response_text_with_cancel(
        response,
        cancel.clone(),
        internal_token,
        body_password,
        status_code,
        started_at,
    )
    .await
    {
        Ok(response_text) => response_text,
        Err(error_result) => {
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                &[],
                Some(status_code),
                false,
                Some(error_result.message.as_str()),
                prepare_request_ms,
                http_send_ms,
                response_read_started.elapsed().as_millis() as u64,
                0,
                started_at.elapsed().as_millis() as u64,
            );
            return error_result;
        }
    };
    let response_read_ms = response_read_started.elapsed().as_millis() as u64;

    if !(200..300).contains(&status_code) {
        log_lightning_rwkv_io(
            debug_context,
            &url,
            source_lang,
            target_lang,
            &body.contents,
            Some(status_code),
            false,
            Some("HTTP error"),
            &[],
            Some(&response_text),
        );
        log_lightning_perf(
            debug_context,
            &url,
            source_lang,
            target_lang,
            source_texts,
            &[],
            Some(status_code),
            false,
            Some("HTTP error"),
            prepare_request_ms,
            http_send_ms,
            response_read_ms,
            0,
            started_at.elapsed().as_millis() as u64,
        );
        return translation_error(
            Some(status_code),
            &response_text,
            internal_token,
            body_password,
            false,
            format!("RWKV API 返回 HTTP {status_code}。"),
            started_at,
        );
    }

    let parse_started = Instant::now();
    let parsed_translations = parse_translations(&response_text, source_texts.len());
    let response_parse_ms = parse_started.elapsed().as_millis() as u64;
    match &parsed_translations {
        Ok(translations) => {
            log_lightning_rwkv_io(
                debug_context,
                &url,
                source_lang,
                target_lang,
                &body.contents,
                Some(status_code),
                true,
                None,
                translations,
                Some(&response_text),
            );
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                translations,
                Some(status_code),
                true,
                None,
                prepare_request_ms,
                http_send_ms,
                response_read_ms,
                response_parse_ms,
                started_at.elapsed().as_millis() as u64,
            );
        }
        Err(error) => {
            log_lightning_rwkv_io(
                debug_context,
                &url,
                source_lang,
                target_lang,
                &body.contents,
                Some(status_code),
                false,
                Some(error),
                &[],
                Some(&response_text),
            );
            log_lightning_perf(
                debug_context,
                &url,
                source_lang,
                target_lang,
                source_texts,
                &[],
                Some(status_code),
                false,
                Some(error),
                prepare_request_ms,
                http_send_ms,
                response_read_ms,
                response_parse_ms,
                started_at.elapsed().as_millis() as u64,
            );
        }
    }

    match parsed_translations {
        Ok(translations) => RwkvTranslationApiTranslateResult {
            ok: true,
            status_code: Some(status_code),
            translations,
            raw_response_preview: String::new(),
            message: format!("RWKV API 已翻译 {} 条文本。", source_texts.len()),
            latency_ms: started_at.elapsed().as_millis(),
        },
        Err(error) => translation_error(
            Some(status_code),
            &response_text,
            internal_token,
            body_password,
            false,
            format!("RWKV API 响应格式不可用: {error}"),
            started_at,
        ),
    }
}

fn build_chat_completions_request(
    source_texts: &[String],
    password: &str,
    source_lang: &str,
    target_lang: &str,
) -> RwkvChatCompletionsRequest {
    RwkvChatCompletionsRequest {
        contents: source_texts
            .iter()
            .map(|text| translation_prompt(text, source_lang, target_lang))
            .collect(),
        max_tokens: 1024,
        temperature: 1.0,
        top_k: 1,
        top_p: 0.0,
        alpha_presence: 0.0,
        alpha_frequency: 0.0,
        alpha_decay: 0.99,
        stream: false,
        password: password.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn log_lightning_rwkv_io(
    debug_context: Option<&str>,
    endpoint: &str,
    source_lang: &str,
    target_lang: &str,
    inputs: &[String],
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
        provider: "rwkv-lightning-contents",
        context: debug_context,
        endpoint: Some(endpoint),
        source_lang: Some(source_lang),
        target_lang: Some(target_lang),
        status_code,
        ok,
        error,
        inputs: inputs.iter().map(String::as_str).collect(),
        outputs: translations.iter().map(String::as_str).collect(),
        raw_response,
    });
}

#[allow(clippy::too_many_arguments)]
fn log_lightning_perf(
    debug_context: Option<&str>,
    endpoint: &str,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    translations: &[String],
    status_code: Option<u16>,
    ok: bool,
    error: Option<&str>,
    prepare_request_ms: u64,
    http_send_ms: u64,
    response_read_ms: u64,
    response_parse_ms: u64,
    latency_ms: u64,
) {
    if !crate::rwkv_perf_debug::enabled() {
        return;
    }
    crate::rwkv_perf_debug::log_record(crate::rwkv_perf_debug::RwkvPerfRecord {
        provider: "rwkv-lightning-contents",
        context: debug_context,
        endpoint: Some(endpoint),
        source_lang: Some(source_lang),
        target_lang: Some(target_lang),
        batch_size: source_texts.len(),
        input_chars: source_texts
            .iter()
            .map(|text| text.chars().count() as u64)
            .sum(),
        output_chars: translations
            .iter()
            .map(|text| text.chars().count() as u64)
            .sum(),
        status_code,
        ok,
        error,
        prepare_request_ms,
        http_send_ms,
        response_read_ms,
        response_parse_ms,
        latency_ms,
    });
}

fn translation_prompt(source_text: &str, source_lang: &str, target_lang: &str) -> String {
    let source_label = prompt_language_label(source_lang);
    let target_label = prompt_language_label(target_lang);
    let source_text = crate::rwkv_text_cleaning::clean_text_for_rwkv(source_text);
    format!("{source_label}: {source_text}\n\n{target_label}:")
}

fn prompt_language_label(language: &str) -> &str {
    match language {
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

fn parse_translations(response_text: &str, expected_count: usize) -> Result<Vec<String>, String> {
    if looks_like_event_stream(response_text) {
        return parse_streaming_translations(response_text, expected_count);
    }

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

fn looks_like_event_stream(response_text: &str) -> bool {
    response_text
        .lines()
        .any(|line| line.trim_start().starts_with("data:"))
}

fn parse_streaming_translations(
    response_text: &str,
    expected_count: usize,
) -> Result<Vec<String>, String> {
    let mut translations = vec![String::new(); expected_count];

    for line in response_text.lines() {
        let trimmed = line.trim();
        let Some(payload) = trimmed.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }

        let chunk: RwkvStreamingChunk = serde_json::from_str(payload)
            .map_err(|error| format!("stream JSON parse failed: {error}"))?;
        for choice in chunk.choices {
            if choice.index >= expected_count {
                continue;
            }
            let content = choice
                .delta
                .or(choice.message)
                .map(|message| message.content)
                .unwrap_or_default();
            translations[choice.index].push_str(&content);
        }
    }

    translations
        .into_iter()
        .enumerate()
        .map(|(index, translation)| {
            let translation = translation.trim().to_string();
            if translation.is_empty() {
                Err(format!(
                    "missing translation for stream choice index {index}"
                ))
            } else {
                Ok(translation)
            }
        })
        .collect()
}

fn api_url(base_url: &str, endpoint: &str) -> String {
    let base_url = base_url.trim();
    let base_url = if base_url.contains("://") {
        base_url.to_string()
    } else {
        format!("http://{base_url}")
    };

    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        endpoint.trim_start_matches('/')
    )
}

fn is_cancelled(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.is_some_and(|cancel| cancel.load(Ordering::SeqCst))
}

async fn response_text_with_cancel(
    response: reqwest::Response,
    cancel: Option<Arc<AtomicBool>>,
    internal_token: &str,
    body_password: &str,
    status_code: u16,
    started_at: Instant,
) -> Result<String, RwkvTranslationApiTranslateResult> {
    let Some(cancel) = cancel else {
        return response.text().await.map_err(|error| {
            translation_error(
                Some(status_code),
                "",
                internal_token,
                body_password,
                false,
                format!("无法读取 RWKV API 响应: {error}"),
                started_at,
            )
        });
    };

    let handle = tokio::spawn(response.text());
    loop {
        if cancel.load(Ordering::SeqCst) {
            handle.abort();
            return Err(translation_error(
                Some(status_code),
                "",
                internal_token,
                body_password,
                false,
                "RWKV API 请求已取消。".to_string(),
                started_at,
            ));
        }

        if handle.is_finished() {
            return match handle.await {
                Ok(Ok(response_text)) => Ok(response_text),
                Ok(Err(error)) => Err(translation_error(
                    Some(status_code),
                    "",
                    internal_token,
                    body_password,
                    false,
                    format!("无法读取 RWKV API 响应: {error}"),
                    started_at,
                )),
                Err(error) => Err(translation_error(
                    Some(status_code),
                    "",
                    internal_token,
                    body_password,
                    false,
                    format!("RWKV API 响应读取任务失败: {error}"),
                    started_at,
                )),
            };
        }

        tokio::time::sleep(Duration::from_millis(RUN_POLL_INTERVAL_MS)).await;
    }
}

fn update_run_status(
    registry: &RwkvTranslationRunRegistry,
    run_id: &str,
    update: impl FnOnce(&mut RwkvTranslationRunStatus),
) -> Result<RwkvTranslationRunStatus, String> {
    let mut runs = registry
        .runs
        .lock()
        .map_err(|_| "翻译运行状态锁不可用。".to_string())?;
    let Some(record) = runs.get_mut(run_id) else {
        return Err("翻译运行不存在。".to_string());
    };
    update(&mut record.status);
    Ok(record.status.clone())
}

fn cancel_current_run(
    registry: &RwkvTranslationRunRegistry,
    dir: &std::path::Path,
    run_id: &str,
    translation_file_id: &str,
    target_lang: &str,
    segments: &mut [TranslationSegment],
    current_batch_segment_ids: &[String],
) -> Result<RwkvTranslationRunStatus, String> {
    mark_translation_segments_pending(segments, current_batch_segment_ids);
    let bundle = save_run_segments(dir, translation_file_id, target_lang, segments.to_vec())?;
    update_run_status(registry, run_id, |status| {
        status.state = RwkvTranslationRunState::Cancelled;
        status.message = "翻译已停止，当前批次已恢复为待翻译。".to_string();
        status.translation_file = Some(bundle.0.clone());
        status.segments = Some(bundle.1.clone());
    })
}

fn save_run_segments(
    dir: &std::path::Path,
    translation_file_id: &str,
    target_lang: &str,
    segments: Vec<TranslationSegment>,
) -> Result<
    (
        crate::rosetta_jobs::model::RosettaTranslationFile,
        Vec<TranslationSegment>,
    ),
    String,
> {
    let mut translation_files = read_translation_files(dir)?;
    let Some(index) = translation_files
        .iter()
        .position(|file| file.id == translation_file_id)
    else {
        return Err("译文文件不存在，无法保存翻译运行状态。".to_string());
    };

    write_translation_segments(dir, translation_file_id, &segments)?;
    let source_file_id = translation_files[index].source_file_id.clone();
    let target_lang = if target_lang.trim().is_empty() {
        translation_files[index].target_lang.clone()
    } else {
        target_lang.to_string()
    };
    let translation_file = build_translation_file(&source_file_id, &target_lang, segments.clone());
    translation_files[index] = translation_file.clone();
    write_translation_files(dir, &translation_files)?;

    Ok((translation_file, segments))
}

fn mark_translation_segments_translating(
    segments: &mut [TranslationSegment],
    source_segment_ids: &[String],
) {
    let ids = source_segment_ids.iter().collect::<HashSet<_>>();
    for segment in segments {
        if ids.contains(&segment.source_segment_id) {
            segment.status = "translating".to_string();
            segment.translated_text = None;
            segment.error = None;
        }
    }
}

fn mark_translation_segments_pending(
    segments: &mut [TranslationSegment],
    source_segment_ids: &[String],
) {
    let ids = source_segment_ids.iter().collect::<HashSet<_>>();
    for segment in segments {
        if ids.contains(&segment.source_segment_id) {
            segment.status = "pending".to_string();
            segment.translated_text = None;
            segment.error = None;
        }
    }
}

fn mark_translation_segments_failed(
    segments: &mut [TranslationSegment],
    source_segment_ids: &[String],
    error: &str,
) {
    let ids = source_segment_ids.iter().collect::<HashSet<_>>();
    for segment in segments {
        if ids.contains(&segment.source_segment_id) {
            segment.status = "failed".to_string();
            segment.error = Some(error.to_string());
        }
    }
}

fn mark_translation_segments_done(
    segments: &mut [TranslationSegment],
    source_segment_ids: &[String],
    translations: &[String],
) {
    let translations = source_segment_ids
        .iter()
        .zip(translations.iter())
        .collect::<HashMap<_, _>>();
    for segment in segments {
        if let Some(translation) = translations.get(&segment.source_segment_id) {
            segment.status = "done".to_string();
            segment.translated_text = Some((*translation).clone());
            segment.error = None;
        }
    }
}

fn translation_error(
    status_code: Option<u16>,
    response_text: &str,
    internal_token: &str,
    body_password: &str,
    include_raw_response_preview: bool,
    message: String,
    started_at: Instant,
) -> RwkvTranslationApiTranslateResult {
    RwkvTranslationApiTranslateResult {
        ok: false,
        status_code,
        translations: Vec::new(),
        raw_response_preview: if include_raw_response_preview {
            preview_text_with_redactions(response_text, internal_token, body_password)
        } else {
            String::new()
        },
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

// =============================================================================
// rwkv-mobile batch-chat provider (Phase 1 — Provider Adapter Split)
// =============================================================================
//
// Below is the parallel command/run-orchestration path for the
// `rwkv-mobile-batch-chat` provider talking to a local sidecar that exposes
// `/v1/chat/roles` + `/v1/batch/chat` (see `rwkv_providers::mobile_batch_chat`).
//
// The HTTP shape, auth model (none, sidecar is loopback-only), and response
// parsing differ enough from the existing `rwkv-lightning-contents` flow that
// they get their own request types and their own run-start command. Cancel /
// status commands stay shared via `RwkvTranslationRunRegistry`.
//
// The run-orchestration body is intentionally a near-duplicate of
// `start_translation_run`. Deduplication into a generic loop is deferred to a
// future iteration that also migrates the lightning-contents path onto the
// `ProviderTranslateBatch` trait — see plan
// `docs/engineering/plans/2026-05-13-macos-rwkv-one-click-implementation.md`
// Phase 1.B.

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvMobileBatchChatProbeRequest {
    base_url: String,
    timeout_ms: u64,
    source_lang: Option<String>,
    target_lang: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvMobileBatchChatTranslateRequest {
    base_url: String,
    timeout_ms: u64,
    source_lang: Option<String>,
    target_lang: Option<String>,
    source_texts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvMobileBatchChatRunStartRequest {
    run_id: String,
    job_id: String,
    translation_file_id: String,
    source_segment_ids: Vec<String>,
    base_url: String,
    timeout_ms: u64,
    source_lang: Option<String>,
    target_lang: String,
    batch_size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvLlamaCppChatProbeRequest {
    base_url: String,
    timeout_ms: u64,
    source_lang: Option<String>,
    target_lang: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvLlamaCppChatTranslateRequest {
    base_url: String,
    timeout_ms: u64,
    source_lang: Option<String>,
    target_lang: Option<String>,
    source_texts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvLlamaCppChatRunStartRequest {
    run_id: String,
    job_id: String,
    translation_file_id: String,
    source_segment_ids: Vec<String>,
    base_url: String,
    timeout_ms: u64,
    source_lang: Option<String>,
    target_lang: String,
    batch_size: usize,
}

#[tauri::command]
pub async fn probe_rwkv_mobile_batch_chat(
    request: RwkvMobileBatchChatProbeRequest,
) -> Result<RwkvTranslationApiProbeResult, String> {
    let config = MobileBatchChatConfig {
        base_url: request.base_url,
        timeout_ms: request.timeout_ms,
    };
    let source_lang = request.source_lang.as_deref().unwrap_or("en");
    let target_lang = request.target_lang.as_deref().unwrap_or("zh-CN");
    let result = mobile_batch_chat::probe(&config, source_lang, target_lang).await;
    Ok(provider_result_into_probe(result))
}

#[tauri::command]
pub async fn translate_rwkv_mobile_batch_chat_texts(
    request: RwkvMobileBatchChatTranslateRequest,
) -> Result<RwkvTranslationApiTranslateResult, String> {
    if request.source_texts.is_empty() {
        return Ok(RwkvTranslationApiTranslateResult {
            ok: true,
            status_code: None,
            translations: Vec::new(),
            raw_response_preview: String::new(),
            message: "没有需要翻译的文本。".to_string(),
            latency_ms: 0,
        });
    }

    let config = MobileBatchChatConfig {
        base_url: request.base_url,
        timeout_ms: request.timeout_ms,
    };
    let source_lang = request.source_lang.as_deref().unwrap_or("en").to_string();
    let target_lang = request
        .target_lang
        .as_deref()
        .unwrap_or("zh-CN")
        .to_string();
    // One-shot ad-hoc translation: set roles ourselves (the orchestrated
    // run-mode path is the one that owns the once-per-run set_roles call).
    if let Err(message) =
        mobile_batch_chat::set_chat_roles_for_pair(&config, &source_lang, &target_lang, None).await
    {
        return Ok(RwkvTranslationApiTranslateResult {
            ok: false,
            status_code: None,
            translations: Vec::new(),
            raw_response_preview: String::new(),
            message,
            latency_ms: 0,
        });
    }
    let result = mobile_batch_chat::translate_batch(
        &config,
        ProviderTranslateBatch {
            source_texts: &request.source_texts,
            source_lang: &source_lang,
            target_lang: &target_lang,
            timeout_ms: config.timeout_ms,
            cancel: None,
            debug_context: Some("mobile-adhoc"),
        },
    )
    .await;
    Ok(provider_result_into_translate(result))
}

#[tauri::command]
pub async fn probe_rwkv_llama_cpp_chat(
    request: RwkvLlamaCppChatProbeRequest,
) -> Result<RwkvTranslationApiProbeResult, String> {
    let config = LlamaCppChatConfig {
        base_url: request.base_url,
        timeout_ms: request.timeout_ms,
    };
    let source_lang = request.source_lang.as_deref().unwrap_or("en");
    let target_lang = request.target_lang.as_deref().unwrap_or("zh-CN");
    let result = llama_cpp_chat::probe(&config, source_lang, target_lang).await;
    Ok(provider_result_into_probe(result))
}

#[tauri::command]
pub async fn translate_rwkv_llama_cpp_chat_texts(
    request: RwkvLlamaCppChatTranslateRequest,
) -> Result<RwkvTranslationApiTranslateResult, String> {
    if request.source_texts.is_empty() {
        return Ok(RwkvTranslationApiTranslateResult {
            ok: true,
            status_code: None,
            translations: Vec::new(),
            raw_response_preview: String::new(),
            message: "没有需要翻译的文本。".to_string(),
            latency_ms: 0,
        });
    }

    let config = LlamaCppChatConfig {
        base_url: request.base_url,
        timeout_ms: request.timeout_ms,
    };
    let source_lang = request.source_lang.as_deref().unwrap_or("en");
    let target_lang = request.target_lang.as_deref().unwrap_or("zh-CN");
    let result = llama_cpp_chat::translate_batch(
        &config,
        ProviderTranslateBatch {
            source_texts: &request.source_texts,
            source_lang,
            target_lang,
            timeout_ms: config.timeout_ms,
            cancel: None,
            debug_context: Some("llama-cpp-adhoc"),
        },
    )
    .await;
    Ok(provider_result_into_translate(result))
}

#[tauri::command]
pub async fn start_rwkv_mobile_batch_chat_run(
    app: AppHandle,
    registry: State<'_, RwkvTranslationRunRegistry>,
    request: RwkvMobileBatchChatRunStartRequest,
) -> Result<RwkvTranslationRunStatus, String> {
    start_mobile_batch_chat_run(app, registry.inner(), request).await
}

#[tauri::command]
pub async fn start_rwkv_llama_cpp_chat_run(
    app: AppHandle,
    registry: State<'_, RwkvTranslationRunRegistry>,
    request: RwkvLlamaCppChatRunStartRequest,
) -> Result<RwkvTranslationRunStatus, String> {
    start_llama_cpp_chat_run(app, registry.inner(), request).await
}

async fn start_mobile_batch_chat_run(
    app: AppHandle,
    registry: &RwkvTranslationRunRegistry,
    request: RwkvMobileBatchChatRunStartRequest,
) -> Result<RwkvTranslationRunStatus, String> {
    if request.run_id.trim().is_empty() {
        return Err("翻译运行 id 不能为空。".to_string());
    }
    // `request.batch_size` is treated as an upper-bound hint from the
    // frontend; 0 means "let Rust pick the sidecar maximum". The real value
    // is decided after we query `/v1/batch/supported_batch_sizes` below.

    let root = jobs_root(&app)?;
    let dir = checked_job_dir(&root, &request.job_id)?;
    let source_segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut translation_segments = read_translation_segments(&dir, &request.translation_file_id)?;
    let source_segment_ids = request
        .source_segment_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let targets = source_segments
        .iter()
        .filter(|segment| source_segment_ids.contains(&segment.id))
        .filter(|segment| segment.status != "skipped" && !segment.source_text.trim().is_empty())
        .filter(|segment| {
            translation_segments
                .iter()
                .find(|translation| translation.source_segment_id == segment.id)
                .is_some_and(|translation| translation.status != "skipped")
        })
        .cloned()
        .collect::<Vec<_>>();

    let cancel = Arc::new(AtomicBool::new(false));
    let initial_bundle = save_run_segments(
        &dir,
        &request.translation_file_id,
        &request.target_lang,
        translation_segments.clone(),
    )?;
    let initial_status = RwkvTranslationRunStatus {
        run_id: request.run_id.clone(),
        job_id: request.job_id.clone(),
        translation_file_id: request.translation_file_id.clone(),
        state: RwkvTranslationRunState::Running,
        completed_segment_ids: Vec::new(),
        failed_segment_ids: Vec::new(),
        message: if targets.is_empty() {
            "没有需要翻译的文本。".to_string()
        } else {
            "翻译运行已开始。".to_string()
        },
        translation_file: Some(initial_bundle.0),
        segments: Some(initial_bundle.1),
    };
    {
        let mut runs = registry
            .runs
            .lock()
            .map_err(|_| "翻译运行状态锁不可用。".to_string())?;
        runs.insert(
            request.run_id.clone(),
            RwkvTranslationRunRecord {
                cancel: cancel.clone(),
                status: initial_status.clone(),
            },
        );
    }

    if targets.is_empty() {
        let status = update_run_status(registry, &request.run_id, |status| {
            status.state = RwkvTranslationRunState::Completed;
            status.message = "没有需要翻译的文本。".to_string();
        })?;
        return Ok(status);
    }

    let provider_config = MobileBatchChatConfig {
        base_url: request.base_url,
        timeout_ms: request.timeout_ms,
    };
    let source_lang = request.source_lang.as_deref().unwrap_or("en").to_string();

    // If the user already clicked stop before we even reach the query, bail
    // out cleanly instead of running it. (Mostly defensive — the registry
    // insertion above is the earliest the cancel command can possibly fire.)
    if cancel.load(Ordering::SeqCst) {
        eprintln!("[rwkv-cancel] mobile_batch_chat: cancel set before query_supported_batch_sizes");
        let status = cancel_current_run(
            registry,
            &dir,
            &request.run_id,
            &request.translation_file_id,
            &request.target_lang,
            &mut translation_segments,
            &[],
        )?;
        return Ok(status);
    }

    // Phase 6 dynamic batch policy: ask the sidecar what batch sizes its
    // loaded model accepts, then clamp the frontend hint against the reported
    // maximum. Replaces the old `SIDECAR_MAX_BATCH_SIZE = 12` hardcode. A
    // failure here (sidecar unreachable, bad JSON) fails the run before any
    // segment is marked translating, so the user sees a clear error and can
    // retry without manual state recovery.
    let supported_sizes =
        match mobile_batch_chat::query_supported_batch_sizes(&provider_config).await {
            Ok(sizes) => sizes,
            Err(message) => {
                // If the user clicked stop while the GET was in flight, prefer
                // the Cancelled state — UI shows "已停止" rather than "失败".
                if cancel.load(Ordering::SeqCst) {
                    eprintln!(
                    "[rwkv-cancel] mobile_batch_chat: cancel during query_supported_batch_sizes"
                );
                    let status = cancel_current_run(
                        registry,
                        &dir,
                        &request.run_id,
                        &request.translation_file_id,
                        &request.target_lang,
                        &mut translation_segments,
                        &[],
                    )?;
                    return Ok(status);
                }
                let detail = format!("无法获取 /v1/batch/supported_batch_sizes: {message}");
                let status = update_run_status(registry, &request.run_id, |status| {
                    status.state = RwkvTranslationRunState::Failed;
                    status.message = detail.clone();
                })?;
                let _ = status;
                return Err(detail);
            }
        };
    let ceiling = mobile_batch_chat::pick_batch_size(&supported_sizes, request.batch_size);

    // Phase 6 single-direction-per-run: set roles **once** at the top of the
    // run instead of per batch. `/v1/chat/roles` is global sidecar state and
    // repeated POSTs are wasted RTTs (a 50-segment run with batch=8 used to
    // hit roles 7 times). Subsequent translate_batch calls inherit.
    if let Err(message) = mobile_batch_chat::set_chat_roles_for_pair(
        &provider_config,
        &source_lang,
        &request.target_lang,
        Some(cancel.clone()),
    )
    .await
    {
        // set_chat_roles_for_pair returns "RWKV 翻译请求已取消。" when the
        // cancel flag was tripped during its HTTP roundtrip. Treat that as
        // Cancelled, not Failed.
        if cancel.load(Ordering::SeqCst) {
            eprintln!("[rwkv-cancel] mobile_batch_chat: cancel during set_chat_roles_for_pair");
            let status = cancel_current_run(
                registry,
                &dir,
                &request.run_id,
                &request.translation_file_id,
                &request.target_lang,
                &mut translation_segments,
                &[],
            )?;
            return Ok(status);
        }
        let detail = format!("设置 /v1/chat/roles 失败: {message}");
        let status = update_run_status(registry, &request.run_id, |status| {
            status.state = RwkvTranslationRunState::Failed;
            status.message = detail.clone();
        })?;
        let _ = status;
        return Err(detail);
    }

    // Phase 6.B length-bucket batch planning: instead of fixed `chunks(N)`,
    // group consecutive segments greedily so that long segments fall into
    // smaller batches automatically (≤300 chars stays at ceiling, 301..1200
    // ceiling/2, 1201..2500 ceiling/4, >2500 batch=1). Order is preserved.
    let planned_batches = mobile_batch_chat::plan_batches(&targets, ceiling, |segment| {
        segment.source_text.chars().count()
    });

    let mut completed_segment_ids = Vec::new();
    let mut failed_segment_ids = Vec::new();

    for (batch_index, batch) in planned_batches.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            eprintln!(
                "[rwkv-cancel] mobile_batch_chat: cancel before batch #{batch_index} (of {})",
                planned_batches.len()
            );
            let status = cancel_current_run(
                registry,
                &dir,
                &request.run_id,
                &request.translation_file_id,
                &request.target_lang,
                &mut translation_segments,
                &[],
            )?;
            return Ok(status);
        }

        let batch_ids = batch
            .iter()
            .map(|segment| segment.id.clone())
            .collect::<Vec<_>>();
        mark_translation_segments_translating(&mut translation_segments, &batch_ids);
        let bundle = save_run_segments(
            &dir,
            &request.translation_file_id,
            &request.target_lang,
            translation_segments.clone(),
        )?;
        update_run_status(registry, &request.run_id, |status| {
            status.state = RwkvTranslationRunState::Running;
            status.message = "正在翻译当前批次。".to_string();
            status.translation_file = Some(bundle.0.clone());
            status.segments = Some(bundle.1.clone());
        })?;

        let source_texts = batch
            .iter()
            .map(|segment| segment.source_text.clone())
            .collect::<Vec<_>>();
        let debug_context = format!(
            "mobile-run:{}:{}:{}",
            request.run_id, request.job_id, request.translation_file_id
        );
        let result = mobile_batch_chat::translate_batch(
            &provider_config,
            ProviderTranslateBatch {
                source_texts: &source_texts,
                source_lang: &source_lang,
                target_lang: &request.target_lang,
                timeout_ms: provider_config.timeout_ms,
                cancel: Some(cancel.clone()),
                debug_context: Some(&debug_context),
            },
        )
        .await;

        if cancel.load(Ordering::SeqCst) {
            eprintln!(
                "[rwkv-cancel] mobile_batch_chat: cancel after batch #{batch_index} translate_batch returned ok={}",
                result.ok
            );
            let status = cancel_current_run(
                registry,
                &dir,
                &request.run_id,
                &request.translation_file_id,
                &request.target_lang,
                &mut translation_segments,
                &batch_ids,
            )?;
            return Ok(status);
        }

        if !result.ok || result.translations.len() != batch.len() {
            let message = if result.ok {
                format!(
                    "RWKV /v1/batch/chat 返回 {} 条译文，但本批有 {} 条文本。",
                    result.translations.len(),
                    batch.len()
                )
            } else {
                result.message
            };
            mark_translation_segments_failed(&mut translation_segments, &batch_ids, &message);
            failed_segment_ids.extend(batch_ids.clone());
            let bundle = save_run_segments(
                &dir,
                &request.translation_file_id,
                &request.target_lang,
                translation_segments.clone(),
            )?;
            let status = update_run_status(registry, &request.run_id, |status| {
                status.state = RwkvTranslationRunState::Failed;
                status.failed_segment_ids = failed_segment_ids.clone();
                status.message = message.clone();
                status.translation_file = Some(bundle.0.clone());
                status.segments = Some(bundle.1.clone());
            })?;
            return Ok(status);
        }

        mark_translation_segments_done(&mut translation_segments, &batch_ids, &result.translations);
        completed_segment_ids.extend(batch_ids);
        let bundle = save_run_segments(
            &dir,
            &request.translation_file_id,
            &request.target_lang,
            translation_segments.clone(),
        )?;
        update_run_status(registry, &request.run_id, |status| {
            status.completed_segment_ids = completed_segment_ids.clone();
            status.message = format!("已完成 {} 段。", status.completed_segment_ids.len());
            status.translation_file = Some(bundle.0.clone());
            status.segments = Some(bundle.1.clone());
        })?;
    }

    update_run_status(registry, &request.run_id, |status| {
        status.state = RwkvTranslationRunState::Completed;
        status.completed_segment_ids = completed_segment_ids;
        status.failed_segment_ids = failed_segment_ids;
        status.message = "翻译运行完成。".to_string();
    })
}

async fn start_llama_cpp_chat_run(
    app: AppHandle,
    registry: &RwkvTranslationRunRegistry,
    request: RwkvLlamaCppChatRunStartRequest,
) -> Result<RwkvTranslationRunStatus, String> {
    if request.run_id.trim().is_empty() {
        return Err("翻译运行 id 不能为空。".to_string());
    }

    let root = jobs_root(&app)?;
    let dir = checked_job_dir(&root, &request.job_id)?;
    let source_segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut translation_segments = read_translation_segments(&dir, &request.translation_file_id)?;
    let source_segment_ids = request
        .source_segment_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let targets = source_segments
        .iter()
        .filter(|segment| source_segment_ids.contains(&segment.id))
        .filter(|segment| segment.status != "skipped" && !segment.source_text.trim().is_empty())
        .filter(|segment| {
            translation_segments
                .iter()
                .find(|translation| translation.source_segment_id == segment.id)
                .is_some_and(|translation| translation.status != "skipped")
        })
        .cloned()
        .collect::<Vec<_>>();

    let cancel = Arc::new(AtomicBool::new(false));
    let initial_bundle = save_run_segments(
        &dir,
        &request.translation_file_id,
        &request.target_lang,
        translation_segments.clone(),
    )?;
    let initial_status = RwkvTranslationRunStatus {
        run_id: request.run_id.clone(),
        job_id: request.job_id.clone(),
        translation_file_id: request.translation_file_id.clone(),
        state: RwkvTranslationRunState::Running,
        completed_segment_ids: Vec::new(),
        failed_segment_ids: Vec::new(),
        message: if targets.is_empty() {
            "没有需要翻译的文本。".to_string()
        } else {
            "翻译运行已开始。".to_string()
        },
        translation_file: Some(initial_bundle.0),
        segments: Some(initial_bundle.1),
    };
    {
        let mut runs = registry
            .runs
            .lock()
            .map_err(|_| "翻译运行状态锁不可用。".to_string())?;
        runs.insert(
            request.run_id.clone(),
            RwkvTranslationRunRecord {
                cancel: cancel.clone(),
                status: initial_status.clone(),
            },
        );
    }

    if targets.is_empty() {
        let status = update_run_status(registry, &request.run_id, |status| {
            status.state = RwkvTranslationRunState::Completed;
            status.message = "没有需要翻译的文本。".to_string();
        })?;
        return Ok(status);
    }

    let provider_config = LlamaCppChatConfig {
        base_url: request.base_url,
        timeout_ms: request.timeout_ms,
    };
    let source_lang = request.source_lang.as_deref().unwrap_or("en").to_string();
    let llama_cpp_settings = llama_cpp_chat::managed_runtime_settings_from_env();
    let requested = if request.batch_size == 0 {
        llama_cpp_settings.parallel_requests
    } else {
        request.batch_size
    };
    let ceiling = requested.min(llama_cpp_settings.parallel_requests).max(1);
    let planned_batches = mobile_batch_chat::plan_batches(&targets, ceiling, |segment| {
        segment.source_text.chars().count()
    });

    let mut completed_segment_ids = Vec::new();
    let mut failed_segment_ids = Vec::new();

    for (batch_index, batch) in planned_batches.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            eprintln!(
                "[rwkv-cancel] llama_cpp_chat: cancel before batch #{batch_index} (of {})",
                planned_batches.len()
            );
            let status = cancel_current_run(
                registry,
                &dir,
                &request.run_id,
                &request.translation_file_id,
                &request.target_lang,
                &mut translation_segments,
                &[],
            )?;
            return Ok(status);
        }

        let batch_ids = batch
            .iter()
            .map(|segment| segment.id.clone())
            .collect::<Vec<_>>();
        mark_translation_segments_translating(&mut translation_segments, &batch_ids);
        let bundle = save_run_segments(
            &dir,
            &request.translation_file_id,
            &request.target_lang,
            translation_segments.clone(),
        )?;
        update_run_status(registry, &request.run_id, |status| {
            status.state = RwkvTranslationRunState::Running;
            status.message = "正在翻译当前批次。".to_string();
            status.translation_file = Some(bundle.0.clone());
            status.segments = Some(bundle.1.clone());
        })?;

        let source_texts = batch
            .iter()
            .map(|segment| segment.source_text.clone())
            .collect::<Vec<_>>();
        let debug_context = format!(
            "llama-cpp-run:{}:{}:{}:{}->{}",
            request.run_id,
            request.job_id,
            request.translation_file_id,
            source_lang,
            request.target_lang
        );
        let result = llama_cpp_chat::translate_batch(
            &provider_config,
            ProviderTranslateBatch {
                source_texts: &source_texts,
                source_lang: &source_lang,
                target_lang: &request.target_lang,
                timeout_ms: provider_config.timeout_ms,
                cancel: Some(cancel.clone()),
                debug_context: Some(&debug_context),
            },
        )
        .await;

        if cancel.load(Ordering::SeqCst) {
            eprintln!(
                "[rwkv-cancel] llama_cpp_chat: cancel after batch #{batch_index} translate_batch returned ok={}",
                result.ok
            );
            let status = cancel_current_run(
                registry,
                &dir,
                &request.run_id,
                &request.translation_file_id,
                &request.target_lang,
                &mut translation_segments,
                &batch_ids,
            )?;
            return Ok(status);
        }

        if !result.ok || result.translations.len() != batch.len() {
            let message = if result.ok {
                format!(
                    "llama.cpp 返回 {} 条译文，但本批有 {} 条文本。",
                    result.translations.len(),
                    batch.len()
                )
            } else {
                result.message
            };
            mark_translation_segments_failed(&mut translation_segments, &batch_ids, &message);
            failed_segment_ids.extend(batch_ids.clone());
            let bundle = save_run_segments(
                &dir,
                &request.translation_file_id,
                &request.target_lang,
                translation_segments.clone(),
            )?;
            let status = update_run_status(registry, &request.run_id, |status| {
                status.state = RwkvTranslationRunState::Failed;
                status.failed_segment_ids = failed_segment_ids.clone();
                status.message = message.clone();
                status.translation_file = Some(bundle.0.clone());
                status.segments = Some(bundle.1.clone());
            })?;
            return Ok(status);
        }

        mark_translation_segments_done(&mut translation_segments, &batch_ids, &result.translations);
        completed_segment_ids.extend(batch_ids);
        let bundle = save_run_segments(
            &dir,
            &request.translation_file_id,
            &request.target_lang,
            translation_segments.clone(),
        )?;
        update_run_status(registry, &request.run_id, |status| {
            status.completed_segment_ids = completed_segment_ids.clone();
            status.message = format!("已完成 {} 段。", status.completed_segment_ids.len());
            status.translation_file = Some(bundle.0.clone());
            status.segments = Some(bundle.1.clone());
        })?;
    }

    update_run_status(registry, &request.run_id, |status| {
        status.state = RwkvTranslationRunState::Completed;
        status.completed_segment_ids = completed_segment_ids;
        status.failed_segment_ids = failed_segment_ids;
        status.message = "翻译运行完成。".to_string();
    })
}

fn provider_result_into_probe(result: ProviderTranslateResult) -> RwkvTranslationApiProbeResult {
    RwkvTranslationApiProbeResult {
        ok: result.ok,
        status_code: result.status_code,
        translations: result.translations,
        raw_response_preview: result.raw_response_preview,
        message: result.message,
        latency_ms: result.latency_ms,
    }
}

fn provider_result_into_translate(
    result: ProviderTranslateResult,
) -> RwkvTranslationApiTranslateResult {
    RwkvTranslationApiTranslateResult {
        ok: result.ok,
        status_code: result.status_code,
        translations: result.translations,
        raw_response_preview: result.raw_response_preview,
        message: result.message,
        latency_ms: result.latency_ms,
    }
}

/// Translate a batch of texts via the Lightning (rwkv-lightning-contents) API.
/// Used by the PDF translation OpenAI shim when the remote API provider is active.
pub async fn translate_batch_via_lightning(
    base_url: &str,
    endpoint: &str,
    internal_token: &str,
    body_password: &str,
    timeout_ms: u64,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    debug_context: Option<&str>,
) -> Result<Vec<String>, String> {
    let result = request_translations_for_language_pair(
        base_url,
        endpoint,
        internal_token,
        body_password,
        timeout_ms,
        source_lang,
        target_lang,
        source_texts,
        debug_context,
    )
    .await;
    if result.ok {
        Ok(result.translations)
    } else {
        Err(result.message)
    }
}

/// Translate a batch of texts via the llama.cpp raw completion API.
/// Used by the PDF translation OpenAI shim when the Windows Vulkan provider is active.
pub async fn translate_batch_via_llama_cpp(
    base_url: &str,
    timeout_ms: u64,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    cancel: Option<Arc<AtomicBool>>,
    debug_context: Option<&str>,
) -> Result<Vec<String>, String> {
    let config = LlamaCppChatConfig {
        base_url: base_url.to_string(),
        timeout_ms,
    };
    let result = llama_cpp_chat::translate_batch(
        &config,
        ProviderTranslateBatch {
            source_texts,
            source_lang,
            target_lang,
            timeout_ms,
            cancel,
            debug_context,
        },
    )
    .await;
    if result.ok {
        Ok(result.translations)
    } else {
        Err(result.message)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn prompt_builder_wraps_english_text_for_chinese_translation() {
        let source_texts = vec!["Hello world.".to_string(), "Good morning.....".to_string()];
        let request = build_chat_completions_request(&source_texts, "secret", "en", "zh-CN");

        assert_eq!(
            request.contents,
            vec![
                "English: Hello world.\n\nChinese:".to_string(),
                "English: Good morning.\n\nChinese:".to_string()
            ]
        );
    }

    #[test]
    fn request_body_serializes_current_batch_shape() {
        let source_texts = vec!["Hello world.".to_string()];
        let request =
            build_chat_completions_request(&source_texts, "model-password", "en", "zh-CN");
        let value = serde_json::to_value(request).expect("request should serialize");

        assert_eq!(
            value["contents"],
            json!(["English: Hello world.\n\nChinese:"])
        );
        assert_eq!(value["max_tokens"], json!(1024));
        assert!(value.get("stop_tokens").is_none());
        assert_eq!(value["temperature"], json!(1.0));
        assert_eq!(value["top_k"], json!(1));
        assert_eq!(value["top_p"], json!(0.0));
        assert_eq!(value["alpha_presence"], json!(0.0));
        assert_eq!(value["alpha_frequency"], json!(0.0));
        assert_eq!(value["alpha_decay"], json!(0.99));
        assert_eq!(value["stream"], json!(false));
        assert_eq!(value["password"], json!("model-password"));
    }

    #[test]
    fn prompt_builder_uses_selected_language_labels() {
        let prompt = translation_prompt("Bonjour.", "fr", "ja");

        assert_eq!(prompt, "French: Bonjour.\n\nJapanese:");
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
    fn response_parser_accepts_streaming_data_chunks() {
        let response = [
            r#"data: {"choices":[{"index":0,"delta":{"content":"第"}}]}"#,
            r#"data: {"choices":[{"index":1,"delta":{"content":"二"}}]}"#,
            r#"data: {"choices":[{"index":0,"delta":{"content":"一段"}}]}"#,
            r#"data: {"choices":[{"index":1,"delta":{"content":"段"}}]}"#,
            r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"},{"index":1,"delta":{},"finish_reason":"stop"}]}"#,
            "data: [DONE]",
        ]
        .join("\n");

        let translations = parse_translations(&response, 2).expect("stream should parse");

        assert_eq!(translations, vec!["第一段".to_string(), "二段".to_string()]);
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
    fn api_url_defaults_bare_host_to_http() {
        assert_eq!(
            api_url("192.168.0.125:8002", "/v1/chat/completions"),
            "http://192.168.0.125:8002/v1/chat/completions"
        );
    }

    #[test]
    fn error_preview_does_not_include_request_token() {
        let result = translation_error(
            Some(500),
            r#"{"error":"sensitive-token sensitive-password"}"#,
            "sensitive-token",
            "sensitive-password",
            true,
            "RWKV API 返回 HTTP 500。".to_string(),
            Instant::now(),
        );

        assert!(!result.raw_response_preview.contains("sensitive-token"));
        assert!(!result.raw_response_preview.contains("sensitive-password"));
        assert!(result.raw_response_preview.contains("<redacted>"));
    }

    #[test]
    fn translation_error_can_omit_raw_preview() {
        let result = translation_error(
            Some(500),
            r#"{"error":"document text"}"#,
            "token",
            "password",
            false,
            "RWKV API 返回 HTTP 500。".to_string(),
            Instant::now(),
        );

        assert!(result.raw_response_preview.is_empty());
    }

    #[test]
    fn run_status_marks_current_batch_translating() {
        let mut segments = vec![
            test_translation_segment(
                "segment-1",
                "pending",
                Some("old translation"),
                Some("old error"),
            ),
            test_translation_segment("segment-2", "done", Some("kept translation"), None),
        ];

        mark_translation_segments_translating(&mut segments, &["segment-1".to_string()]);

        assert_eq!(segments[0].status, "translating");
        assert_eq!(segments[0].translated_text, None);
        assert_eq!(segments[0].error, None);
        assert_eq!(segments[1].status, "done");
        assert_eq!(
            segments[1].translated_text,
            Some("kept translation".to_string())
        );
    }

    #[test]
    fn run_status_cancel_restores_only_current_batch_to_pending() {
        let mut segments = vec![
            test_translation_segment("segment-1", "translating", None, None),
            test_translation_segment("segment-2", "done", Some("finished"), None),
        ];

        mark_translation_segments_pending(&mut segments, &["segment-1".to_string()]);

        assert_eq!(segments[0].status, "pending");
        assert_eq!(segments[0].translated_text, None);
        assert_eq!(segments[0].error, None);
        assert_eq!(segments[1].status, "done");
        assert_eq!(segments[1].translated_text, Some("finished".to_string()));
    }

    #[test]
    fn run_status_failed_batch_records_error_without_clearing_existing_text() {
        let mut segments = vec![
            test_translation_segment("segment-1", "translating", Some("old"), None),
            test_translation_segment("segment-2", "pending", None, None),
        ];

        mark_translation_segments_failed(
            &mut segments,
            &["segment-1".to_string()],
            "model timeout",
        );

        assert_eq!(segments[0].status, "failed");
        assert_eq!(segments[0].translated_text, Some("old".to_string()));
        assert_eq!(segments[0].error, Some("model timeout".to_string()));
        assert_eq!(segments[1].status, "pending");
    }

    #[test]
    fn run_status_done_maps_translations_by_source_segment_id() {
        let mut segments = vec![
            test_translation_segment("segment-2", "translating", None, Some("old error")),
            test_translation_segment("segment-1", "translating", None, None),
        ];

        mark_translation_segments_done(
            &mut segments,
            &["segment-1".to_string(), "segment-2".to_string()],
            &["first".to_string(), "second".to_string()],
        );

        assert_eq!(segments[0].status, "done");
        assert_eq!(segments[0].translated_text, Some("second".to_string()));
        assert_eq!(segments[0].error, None);
        assert_eq!(segments[1].status, "done");
        assert_eq!(segments[1].translated_text, Some("first".to_string()));
    }

    fn test_translation_segment(
        source_segment_id: &str,
        status: &str,
        translated_text: Option<&str>,
        error: Option<&str>,
    ) -> TranslationSegment {
        TranslationSegment {
            source_segment_id: source_segment_id.to_string(),
            translated_text: translated_text.map(str::to_string),
            target_lang: "zh-CN".to_string(),
            status: status.to_string(),
            error: error.map(str::to_string),
            translation_history: Vec::new(),
        }
    }
}
