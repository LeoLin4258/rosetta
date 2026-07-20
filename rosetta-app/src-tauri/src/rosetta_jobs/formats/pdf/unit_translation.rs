use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use serde::Serialize;

use crate::{
    managed_pdf2zh::worker::PdfTranslationUnit,
    pdf_v3::{
        paragraph_translation_plan::{
            VisualParagraphPagePlan, VISUAL_PARAGRAPH_PLAN_SCHEMA_VERSION,
        },
        translation_plan::{
            TranslationPagePlan, TranslationUnitResult, TRANSLATION_PLAN_SCHEMA_VERSION,
        },
    },
    rwkv_providers::{
        llama_cpp_chat,
        mobile_batch_chat::{self, MobileBatchChatConfig},
        ProviderTranslateBatch, ProviderTranslateResult,
    },
};

#[cfg(test)]
use crate::pdf_v3::translation_plan::TranslationUnitPlan;

const LIGHTNING_MAX_BATCH_SIZE: usize = 256;
const LIGHTNING_TARGET_PROMPT_TOKENS: usize = 160;
const LIGHTNING_HARD_PROMPT_TOKENS: usize = 220;
const REFERENCE_ENTRY_MIN_CHARS: usize = 40;
const NON_LIGHTNING_DEFAULT_BATCH_SIZE: usize = 8;
const NON_LIGHTNING_TARGET_PROMPT_TOKENS: usize = 72;
const NON_LIGHTNING_HARD_PROMPT_TOKENS: usize = 88;
const NON_LIGHTNING_RETRY_TARGET_PROMPT_TOKENS: usize = 36;
const NON_LIGHTNING_RETRY_HARD_PROMPT_TOKENS: usize = 44;
const NON_LIGHTNING_FINAL_RETRY_TARGET_PROMPT_TOKENS: usize = 24;
const NON_LIGHTNING_FINAL_RETRY_HARD_PROMPT_TOKENS: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct LightningPdfApiConfig {
    pub base_url: String,
    pub endpoint: String,
    pub internal_token: String,
    pub body_password: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct LlamaCppPdfApiConfig {
    pub base_url: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) enum PdfUnitProviderConfig {
    MobileBatch(MobileBatchChatConfig),
    Lightning(LightningPdfApiConfig),
    LlamaCpp(LlamaCppPdfApiConfig),
    #[cfg(test)]
    Scripted {
        results: Arc<std::sync::Mutex<std::collections::VecDeque<ProviderTranslateResult>>>,
        max_batch_size: usize,
    },
}

impl PdfUnitProviderConfig {
    pub(crate) fn provider_id(&self) -> &'static str {
        match self {
            Self::MobileBatch(_) => "rwkv-mobile-batch-chat",
            Self::Lightning(_) => "rwkv-lightning-contents",
            Self::LlamaCpp(_) => "llama-cpp-chat-completions",
            #[cfg(test)]
            Self::Scripted { .. } => "scripted-test-provider",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderTranslationUnit {
    unit_id: String,
    source_text: String,
    requires_translation: bool,
    chunking: ProviderUnitChunking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderUnitChunking {
    Semantic,
    VisualParagraph,
}

impl From<&PdfTranslationUnit> for ProviderTranslationUnit {
    fn from(unit: &PdfTranslationUnit) -> Self {
        Self {
            unit_id: unit.unit_id.clone(),
            source_text: unit.source_text.clone(),
            requires_translation: unit.requires_translation,
            chunking: ProviderUnitChunking::Semantic,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PdfV3ProviderFailureKind {
    NoTranslatableUnits,
    InvalidPlan,
    Cancelled,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct PdfV3ProviderFailure {
    pub kind: PdfV3ProviderFailureKind,
    pub retryable: bool,
    pub message: String,
}

impl std::fmt::Display for PdfV3ProviderFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PdfV3ProviderFailure {}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct PdfV3ProviderTranslationBatchResult {
    pub results: Vec<TranslationUnitResult>,
    pub metrics: PdfUnitTranslationMetrics,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfUnitTranslation {
    pub unit_id: String,
    pub text: String,
    pub output_chars: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfUnitTranslationBatchResult {
    pub translations: Vec<PdfUnitTranslation>,
    pub metrics: PdfUnitTranslationMetrics,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfUnitTranslationMetrics {
    pub request_count: u64,
    pub batch_size_distribution: Vec<PdfBatchSizeBucket>,
    pub total_input_chars: u64,
    pub total_output_chars: u64,
    pub failed_request_count: u64,
    pub truncated_count: u64,
    pub empty_output_count: u64,
    pub total_request_ms: u64,
    pub max_request_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfBatchSizeBucket {
    pub batch_size: u64,
    pub request_count: u64,
}

impl PdfUnitTranslationMetrics {
    fn record_request(
        &mut self,
        batch_size: usize,
        elapsed_ms: u64,
        ok: bool,
        input_chars: u64,
        output_chars: u64,
    ) {
        self.request_count += 1;
        if !ok {
            self.failed_request_count += 1;
        }
        self.total_request_ms += elapsed_ms;
        self.max_request_ms = self.max_request_ms.max(elapsed_ms);
        self.total_input_chars += input_chars;
        self.total_output_chars += output_chars;
        add_batch_bucket(&mut self.batch_size_distribution, batch_size as u64);
    }
}

#[allow(dead_code)]
pub(crate) async fn translate_pdf_units(
    provider: &PdfUnitProviderConfig,
    source_lang: &str,
    target_lang: &str,
    units: &[PdfTranslationUnit],
    cancel: Option<Arc<AtomicBool>>,
) -> Result<PdfUnitTranslationBatchResult, String> {
    let mut noop = |_translation: PdfUnitTranslation| {};
    translate_pdf_units_with_events(provider, source_lang, target_lang, units, cancel, &mut noop)
        .await
}

#[allow(dead_code)]
pub(crate) async fn translate_pdf_v3_page_plan(
    provider: &PdfUnitProviderConfig,
    source_lang: &str,
    target_lang: &str,
    plan: &TranslationPagePlan,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<PdfV3ProviderTranslationBatchResult, PdfV3ProviderFailure> {
    let units = provider_units_from_v3_plan(plan)?;
    let mut noop = |_translation: PdfUnitTranslation| {};
    let translated = translate_provider_units_with_events(
        provider,
        source_lang,
        target_lang,
        &units,
        cancel.clone(),
        &mut noop,
    )
    .await
    .map_err(|message| provider_failure(message, cancel.as_ref()))?;
    let results = translated
        .translations
        .into_iter()
        .map(|translation| TranslationUnitResult {
            unit_id: translation.unit_id,
            translated_text: translation.text,
        })
        .collect();
    Ok(PdfV3ProviderTranslationBatchResult {
        results,
        metrics: translated.metrics,
    })
}

#[allow(dead_code)]
pub(crate) async fn translate_pdf_v3_visual_paragraph_plan(
    provider: &PdfUnitProviderConfig,
    source_lang: &str,
    target_lang: &str,
    plan: &VisualParagraphPagePlan,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<PdfV3ProviderTranslationBatchResult, PdfV3ProviderFailure> {
    let units = provider_units_from_visual_paragraph_plan(plan)?;
    let mut noop = |_translation: PdfUnitTranslation| {};
    let translated = translate_provider_units_with_events(
        provider,
        source_lang,
        target_lang,
        &units,
        cancel.clone(),
        &mut noop,
    )
    .await
    .map_err(|message| provider_failure(message, cancel.as_ref()))?;
    let results = translated
        .translations
        .into_iter()
        .map(|translation| TranslationUnitResult {
            unit_id: translation.unit_id,
            translated_text: translation.text,
        })
        .collect();
    Ok(PdfV3ProviderTranslationBatchResult {
        results,
        metrics: translated.metrics,
    })
}

pub(crate) async fn translate_pdf_units_with_events(
    provider: &PdfUnitProviderConfig,
    source_lang: &str,
    target_lang: &str,
    units: &[PdfTranslationUnit],
    cancel: Option<Arc<AtomicBool>>,
    on_unit_translation: &mut (dyn FnMut(PdfUnitTranslation) + Send),
) -> Result<PdfUnitTranslationBatchResult, String> {
    let provider_units = units
        .iter()
        .map(ProviderTranslationUnit::from)
        .collect::<Vec<_>>();
    translate_provider_units_with_events(
        provider,
        source_lang,
        target_lang,
        &provider_units,
        cancel,
        on_unit_translation,
    )
    .await
}

async fn translate_provider_units_with_events(
    provider: &PdfUnitProviderConfig,
    source_lang: &str,
    target_lang: &str,
    units: &[ProviderTranslationUnit],
    cancel: Option<Arc<AtomicBool>>,
    on_unit_translation: &mut (dyn FnMut(PdfUnitTranslation) + Send),
) -> Result<PdfUnitTranslationBatchResult, String> {
    let translatable = units
        .iter()
        .filter(|unit| unit.requires_translation && !unit.source_text.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    let passthrough = units
        .iter()
        .filter(|unit| !unit.requires_translation)
        .map(|unit| PdfUnitTranslation {
            unit_id: unit.unit_id.clone(),
            text: String::new(),
            output_chars: 0,
        })
        .collect::<Vec<_>>();

    let mut metrics = PdfUnitTranslationMetrics::default();

    if translatable.is_empty() {
        for translation in passthrough.iter().cloned() {
            on_unit_translation(translation);
        }
        return Ok(PdfUnitTranslationBatchResult {
            translations: passthrough,
            metrics,
        });
    }

    for translation in passthrough.iter().cloned() {
        on_unit_translation(translation);
    }

    let mut translations = match provider {
        PdfUnitProviderConfig::Lightning(config) => {
            translate_units_lightning(
                config,
                source_lang,
                target_lang,
                &translatable,
                cancel,
                &mut metrics,
                on_unit_translation,
            )
            .await?
        }
        PdfUnitProviderConfig::MobileBatch(config) => {
            mobile_batch_chat::set_chat_roles_for_pair(
                config,
                source_lang,
                target_lang,
                cancel.clone(),
            )
            .await?;
            let max_batch_size = mobile_batch_chat::query_supported_batch_sizes(config)
                .await
                .map(|sizes| mobile_batch_chat::pick_batch_size(&sizes, 0))
                .unwrap_or(NON_LIGHTNING_DEFAULT_BATCH_SIZE)
                .max(1);
            translate_units_provider_batches(
                ProviderKind::Mobile(config.clone()),
                source_lang,
                target_lang,
                &translatable,
                max_batch_size,
                cancel,
                &mut metrics,
                on_unit_translation,
            )
            .await?
        }
        PdfUnitProviderConfig::LlamaCpp(config) => {
            let max_batch_size = llama_cpp_chat::managed_runtime_settings_from_env()
                .parallel_requests
                .max(1);
            translate_units_provider_batches(
                ProviderKind::Llama(config.clone()),
                source_lang,
                target_lang,
                &translatable,
                max_batch_size,
                cancel,
                &mut metrics,
                on_unit_translation,
            )
            .await?
        }
        #[cfg(test)]
        PdfUnitProviderConfig::Scripted {
            results,
            max_batch_size,
        } => {
            translate_units_provider_batches(
                ProviderKind::Scripted(Arc::clone(results)),
                source_lang,
                target_lang,
                &translatable,
                (*max_batch_size).max(1),
                cancel,
                &mut metrics,
                on_unit_translation,
            )
            .await?
        }
    };
    translations.extend(passthrough);

    metrics.empty_output_count = translations
        .iter()
        .filter(|translation| {
            translatable
                .iter()
                .any(|unit| unit.unit_id == translation.unit_id)
                && translation.text.trim().is_empty()
        })
        .count() as u64;
    if metrics.empty_output_count > 0 {
        return Err(format!(
            "PDF unit translation produced {} empty output(s).",
            metrics.empty_output_count
        ));
    }
    Ok(PdfUnitTranslationBatchResult {
        translations,
        metrics,
    })
}

#[allow(dead_code)]
fn provider_units_from_v3_plan(
    plan: &TranslationPagePlan,
) -> Result<Vec<ProviderTranslationUnit>, PdfV3ProviderFailure> {
    if plan.schema_version != TRANSLATION_PLAN_SCHEMA_VERSION
        || plan.page_number == 0
        || plan.source_page_hash.is_empty()
    {
        return Err(PdfV3ProviderFailure {
            kind: PdfV3ProviderFailureKind::InvalidPlan,
            retryable: false,
            message: "PDF v3 translation plan identity is invalid.".to_string(),
        });
    }
    if plan.units.is_empty() {
        return Err(PdfV3ProviderFailure {
            kind: PdfV3ProviderFailureKind::NoTranslatableUnits,
            retryable: false,
            message: "PDF v3 translation plan has no safe units.".to_string(),
        });
    }
    let mut unit_ids = BTreeSet::new();
    let mut units = Vec::with_capacity(plan.units.len());
    for unit in &plan.units {
        if unit.unit_id.is_empty()
            || unit.provider_text.trim().is_empty()
            || !unit_ids.insert(unit.unit_id.as_str())
        {
            return Err(PdfV3ProviderFailure {
                kind: PdfV3ProviderFailureKind::InvalidPlan,
                retryable: false,
                message: "PDF v3 translation plan unit identity is invalid.".to_string(),
            });
        }
        units.push(ProviderTranslationUnit {
            unit_id: unit.unit_id.clone(),
            source_text: unit.provider_text.clone(),
            requires_translation: true,
            chunking: ProviderUnitChunking::Semantic,
        });
    }
    Ok(units)
}

#[allow(dead_code)]
fn provider_units_from_visual_paragraph_plan(
    plan: &VisualParagraphPagePlan,
) -> Result<Vec<ProviderTranslationUnit>, PdfV3ProviderFailure> {
    if plan.schema_version != VISUAL_PARAGRAPH_PLAN_SCHEMA_VERSION
        || plan.page_number == 0
        || plan.source_page_hash.is_empty()
    {
        return Err(PdfV3ProviderFailure {
            kind: PdfV3ProviderFailureKind::InvalidPlan,
            retryable: false,
            message: "PDF v3 visual paragraph plan identity is invalid.".to_string(),
        });
    }
    if plan.units.is_empty() {
        return Err(PdfV3ProviderFailure {
            kind: PdfV3ProviderFailureKind::NoTranslatableUnits,
            retryable: false,
            message: "PDF v3 visual paragraph plan has no safe units.".to_string(),
        });
    }
    let mut unit_ids = BTreeSet::new();
    let mut units = Vec::with_capacity(plan.units.len());
    for unit in &plan.units {
        if unit.unit_id.is_empty()
            || unit.paragraph_group_id.is_empty()
            || unit.flow_container_group_id.is_empty()
            || unit.provider_text.trim().is_empty()
            || !unit_ids.insert(unit.unit_id.as_str())
        {
            return Err(PdfV3ProviderFailure {
                kind: PdfV3ProviderFailureKind::InvalidPlan,
                retryable: false,
                message: "PDF v3 visual paragraph unit identity is invalid.".to_string(),
            });
        }
        units.push(ProviderTranslationUnit {
            unit_id: unit.unit_id.clone(),
            source_text: unit.provider_text.clone(),
            requires_translation: true,
            chunking: ProviderUnitChunking::VisualParagraph,
        });
    }
    Ok(units)
}

#[allow(dead_code)]
fn provider_failure(_message: String, cancel: Option<&Arc<AtomicBool>>) -> PdfV3ProviderFailure {
    if cancel.is_some_and(|cancel| cancel.load(Ordering::SeqCst)) {
        PdfV3ProviderFailure {
            kind: PdfV3ProviderFailureKind::Cancelled,
            retryable: true,
            message: "PDF v3 translation provider request was cancelled.".to_string(),
        }
    } else {
        PdfV3ProviderFailure {
            kind: PdfV3ProviderFailureKind::Provider,
            retryable: true,
            message: "PDF v3 translation provider request failed.".to_string(),
        }
    }
}

async fn translate_units_lightning(
    config: &LightningPdfApiConfig,
    source_lang: &str,
    target_lang: &str,
    units: &[ProviderTranslationUnit],
    cancel: Option<Arc<AtomicBool>>,
    metrics: &mut PdfUnitTranslationMetrics,
    on_unit_translation: &mut (dyn FnMut(PdfUnitTranslation) + Send),
) -> Result<Vec<PdfUnitTranslation>, String> {
    let prepared = prepare_lightning_chunks(units);
    let mut chunk_outputs = vec![String::new(); prepared.chunks.len()];
    let mut chunk_ready = vec![false; prepared.chunks.len()];
    let mut emitted_unit_ids = BTreeSet::new();
    emit_ready_unit_outputs(
        units,
        &prepared,
        &chunk_outputs,
        &chunk_ready,
        target_lang,
        &mut emitted_unit_ids,
        on_unit_translation,
    )?;
    for batch in prepared.chunks.chunks(LIGHTNING_MAX_BATCH_SIZE) {
        ensure_not_cancelled(cancel.as_ref())?;
        let source_texts = batch
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let started = Instant::now();
        let result = crate::rwkv_api::translate_batch_via_lightning(
            &config.base_url,
            &config.endpoint,
            &config.internal_token,
            &config.body_password,
            config.timeout_ms,
            source_lang,
            target_lang,
            &source_texts,
            Some("pdf-unit-lightning"),
        )
        .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(translations) if translations.len() == batch.len() => {
                metrics.record_request(
                    batch.len(),
                    elapsed_ms,
                    true,
                    source_texts
                        .iter()
                        .map(|text| text.chars().count() as u64)
                        .sum(),
                    translations
                        .iter()
                        .map(|text| text.chars().count() as u64)
                        .sum(),
                );
                for (chunk, translation) in batch.iter().zip(translations) {
                    chunk_outputs[chunk.chunk_index] = translation;
                    chunk_ready[chunk.chunk_index] = true;
                }
                emit_ready_unit_outputs(
                    units,
                    &prepared,
                    &chunk_outputs,
                    &chunk_ready,
                    target_lang,
                    &mut emitted_unit_ids,
                    on_unit_translation,
                )?;
            }
            Ok(translations) => {
                metrics.record_request(
                    batch.len(),
                    elapsed_ms,
                    false,
                    source_texts
                        .iter()
                        .map(|text| text.chars().count() as u64)
                        .sum(),
                    translations
                        .iter()
                        .map(|text| text.chars().count() as u64)
                        .sum(),
                );
                return Err(format!(
                    "PDF unit translation count mismatch (expected {}, got {}).",
                    batch.len(),
                    translations.len()
                ));
            }
            Err(error) => {
                metrics.record_request(
                    batch.len(),
                    elapsed_ms,
                    false,
                    source_texts
                        .iter()
                        .map(|text| text.chars().count() as u64)
                        .sum(),
                    0,
                );
                return Err(error);
            }
        }
    }
    build_unit_outputs(units, &prepared, &chunk_outputs, target_lang)
}

#[derive(Clone)]
enum ProviderKind {
    Mobile(MobileBatchChatConfig),
    Llama(LlamaCppPdfApiConfig),
    #[cfg(test)]
    Scripted(std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<ProviderTranslateResult>>>),
}

async fn translate_units_provider_batches(
    provider: ProviderKind,
    source_lang: &str,
    target_lang: &str,
    units: &[ProviderTranslationUnit],
    max_batch_size: usize,
    cancel: Option<Arc<AtomicBool>>,
    metrics: &mut PdfUnitTranslationMetrics,
    on_unit_translation: &mut (dyn FnMut(PdfUnitTranslation) + Send),
) -> Result<Vec<PdfUnitTranslation>, String> {
    let prepared = prepare_non_lightning_chunks(units);
    let mut chunk_outputs = vec![String::new(); prepared.chunks.len()];
    let mut chunk_ready = vec![false; prepared.chunks.len()];
    let mut emitted_unit_ids = BTreeSet::new();
    emit_ready_unit_outputs(
        units,
        &prepared,
        &chunk_outputs,
        &chunk_ready,
        target_lang,
        &mut emitted_unit_ids,
        on_unit_translation,
    )?;
    for batch in prepared.chunks.chunks(max_batch_size.max(1)) {
        ensure_not_cancelled(cancel.as_ref())?;
        let source_texts = batch
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let started = Instant::now();
        let result = translate_provider_batch(
            &provider,
            source_lang,
            target_lang,
            &source_texts,
            cancel.clone(),
            provider_debug_context(&provider),
        )
        .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        metrics.record_request(
            batch.len(),
            elapsed_ms,
            result.ok,
            source_texts
                .iter()
                .map(|text| text.chars().count() as u64)
                .sum(),
            result
                .translations
                .iter()
                .map(|text| text.chars().count() as u64)
                .sum(),
        );
        if !result.ok {
            let limit_failure = is_llama_limit_failure(&result.message);
            if limit_failure {
                metrics.truncated_count += 1;
            }
            if provider_supports_llama_split_retry(&provider) && limit_failure {
                let translations = translate_llama_batch_with_split_retry(
                    &provider,
                    source_lang,
                    target_lang,
                    batch,
                    cancel.clone(),
                    metrics,
                )
                .await?;
                for (chunk, translation) in batch.iter().zip(translations) {
                    chunk_outputs[chunk.chunk_index] = translation;
                    chunk_ready[chunk.chunk_index] = true;
                }
                emit_ready_unit_outputs(
                    units,
                    &prepared,
                    &chunk_outputs,
                    &chunk_ready,
                    target_lang,
                    &mut emitted_unit_ids,
                    on_unit_translation,
                )?;
                continue;
            } else {
                return Err(result.message);
            }
        }
        if result.translations.len() != batch.len() {
            metrics.failed_request_count += 1;
            return Err(format!(
                "PDF unit translation count mismatch (expected {}, got {}).",
                batch.len(),
                result.translations.len()
            ));
        }
        for (chunk, translation) in batch.iter().zip(result.translations) {
            chunk_outputs[chunk.chunk_index] = translation;
            chunk_ready[chunk.chunk_index] = true;
        }
        emit_ready_unit_outputs(
            units,
            &prepared,
            &chunk_outputs,
            &chunk_ready,
            target_lang,
            &mut emitted_unit_ids,
            on_unit_translation,
        )?;
    }

    build_unit_outputs(units, &prepared, &chunk_outputs, target_lang)
}

async fn translate_llama_batch_with_split_retry(
    provider: &ProviderKind,
    source_lang: &str,
    target_lang: &str,
    batch: &[PreparedChunk],
    cancel: Option<Arc<AtomicBool>>,
    metrics: &mut PdfUnitTranslationMetrics,
) -> Result<Vec<String>, String> {
    let mut recovered = Vec::with_capacity(batch.len());
    for chunk in batch {
        ensure_not_cancelled(cancel.as_ref())?;
        let translation = translate_llama_chunk_with_split_retry(
            provider,
            source_lang,
            target_lang,
            &chunk.text,
            cancel.clone(),
            metrics,
        )
        .await?;
        recovered.push(translation);
    }
    Ok(recovered)
}

async fn translate_llama_chunk_with_split_retry(
    provider: &ProviderKind,
    source_lang: &str,
    target_lang: &str,
    text: &str,
    cancel: Option<Arc<AtomicBool>>,
    metrics: &mut PdfUnitTranslationMetrics,
) -> Result<String, String> {
    let retry_budgets = [
        (
            NON_LIGHTNING_RETRY_TARGET_PROMPT_TOKENS,
            NON_LIGHTNING_RETRY_HARD_PROMPT_TOKENS,
            "pdf-unit-llama-split-retry",
        ),
        (
            NON_LIGHTNING_FINAL_RETRY_TARGET_PROMPT_TOKENS,
            NON_LIGHTNING_FINAL_RETRY_HARD_PROMPT_TOKENS,
            "pdf-unit-llama-final-split-retry",
        ),
    ];
    let mut last_error = None;
    for (target_tokens, hard_tokens, debug_context) in retry_budgets {
        ensure_not_cancelled(cancel.as_ref())?;
        let parts = split_pdf_text_with_budget(text, target_tokens, hard_tokens);
        let started = Instant::now();
        let result = translate_provider_batch(
            provider,
            source_lang,
            target_lang,
            &parts,
            cancel.clone(),
            debug_context,
        )
        .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        metrics.record_request(
            parts.len(),
            elapsed_ms,
            result.ok,
            parts.iter().map(|part| part.chars().count() as u64).sum(),
            result
                .translations
                .iter()
                .map(|text| text.chars().count() as u64)
                .sum(),
        );
        if result.ok && result.translations.len() == parts.len() {
            return Ok(join_translated_chunks(result.translations, target_lang));
        }
        let message = if result.ok {
            format!(
                "PDF unit split retry count mismatch (expected {}, got {}).",
                parts.len(),
                result.translations.len()
            )
        } else {
            result.message
        };
        if is_llama_limit_failure(&message) {
            metrics.truncated_count += 1;
            last_error = Some(message);
            continue;
        }
        return Err(message);
    }
    Err(last_error.unwrap_or_else(|| "PDF unit split retry failed.".to_string()))
}

async fn translate_provider_batch(
    provider: &ProviderKind,
    source_lang: &str,
    target_lang: &str,
    source_texts: &[String],
    cancel: Option<Arc<AtomicBool>>,
    debug_context: &str,
) -> ProviderTranslateResult {
    match provider {
        ProviderKind::Mobile(config) => {
            mobile_batch_chat::translate_batch(
                config,
                ProviderTranslateBatch {
                    source_texts,
                    source_lang,
                    target_lang,
                    timeout_ms: config.timeout_ms,
                    cancel,
                    debug_context: Some(debug_context),
                },
            )
            .await
        }
        ProviderKind::Llama(config) => {
            llama_cpp_chat::translate_batch(
                &llama_cpp_chat::LlamaCppChatConfig {
                    base_url: config.base_url.clone(),
                    timeout_ms: config.timeout_ms,
                },
                ProviderTranslateBatch {
                    source_texts,
                    source_lang,
                    target_lang,
                    timeout_ms: config.timeout_ms,
                    cancel,
                    debug_context: Some(debug_context),
                },
            )
            .await
        }
        #[cfg(test)]
        ProviderKind::Scripted(results) => results
            .lock()
            .ok()
            .and_then(|mut results| results.pop_front())
            .unwrap_or_else(|| ProviderTranslateResult {
                ok: false,
                status_code: None,
                translations: Vec::new(),
                raw_response_preview: String::new(),
                message: "scripted provider result queue is empty".to_string(),
                latency_ms: 0,
            }),
    }
}

fn provider_debug_context(provider: &ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Mobile(_) => "pdf-unit-mobile",
        ProviderKind::Llama(_) => "pdf-unit-llama",
        #[cfg(test)]
        ProviderKind::Scripted(_) => "pdf-unit-llama",
    }
}

fn provider_supports_llama_split_retry(provider: &ProviderKind) -> bool {
    match provider {
        ProviderKind::Llama(_) => true,
        ProviderKind::Mobile(_) => false,
        #[cfg(test)]
        ProviderKind::Scripted(_) => true,
    }
}

fn is_llama_limit_failure(message: &str) -> bool {
    message.contains("truncated=true") || message.contains("stop_type=limit")
}

struct PreparedChunks {
    chunks: Vec<PreparedChunk>,
    unit_plans: BTreeMap<String, UnitChunkPlan>,
}

#[derive(Clone)]
struct PreparedChunk {
    chunk_index: usize,
    text: String,
}

#[derive(Clone)]
struct UnitChunkPlan {
    parts: Vec<UnitPlanPart>,
}

#[derive(Clone)]
enum UnitPlanPart {
    Text(Vec<usize>),
    Placeholder(String),
    Literal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PdfTextPart {
    Text(String),
    Placeholder(String),
}

fn prepare_lightning_chunks(units: &[ProviderTranslationUnit]) -> PreparedChunks {
    prepare_provider_chunks(units, PdfChunkPolicy::Lightning)
}

fn prepare_non_lightning_chunks(units: &[ProviderTranslationUnit]) -> PreparedChunks {
    prepare_provider_chunks(units, PdfChunkPolicy::NonLightning)
}

#[derive(Clone, Copy)]
enum PdfChunkPolicy {
    Lightning,
    NonLightning,
}

fn prepare_provider_chunks(
    units: &[ProviderTranslationUnit],
    policy: PdfChunkPolicy,
) -> PreparedChunks {
    let mut chunks = Vec::new();
    let mut unit_plans = BTreeMap::new();
    for unit in units {
        if unit.chunking == ProviderUnitChunking::VisualParagraph {
            let normalized = normalize_pdf_text(&unit.source_text);
            let trimmed = normalized.trim();
            let parts = if trimmed.is_empty() {
                Vec::new()
            } else if should_passthrough_pdf_fragment(trimmed) {
                vec![UnitPlanPart::Literal(trimmed.to_string())]
            } else {
                let chunk_index = chunks.len();
                chunks.push(PreparedChunk {
                    chunk_index,
                    text: trimmed.to_string(),
                });
                vec![UnitPlanPart::Text(vec![chunk_index])]
            };
            unit_plans.insert(unit.unit_id.clone(), UnitChunkPlan { parts });
            continue;
        }
        let mut parts = Vec::new();
        for part in split_pdf_placeholder_parts(&unit.source_text) {
            match part {
                PdfTextPart::Placeholder(placeholder) => {
                    parts.push(UnitPlanPart::Placeholder(placeholder));
                }
                PdfTextPart::Text(text) => {
                    let normalized = normalize_pdf_text(&text);
                    let trimmed = normalized.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if should_passthrough_pdf_fragment(trimmed) {
                        parts.push(UnitPlanPart::Literal(trimmed.to_string()));
                        continue;
                    }
                    let text_chunks = match policy {
                        PdfChunkPolicy::Lightning => split_pdf_text_for_lightning(trimmed),
                        PdfChunkPolicy::NonLightning => split_pdf_text(trimmed),
                    }
                    .into_iter()
                    .filter(|chunk| !chunk.trim().is_empty())
                    .collect::<Vec<_>>();
                    let mut indices = Vec::with_capacity(text_chunks.len());
                    for text in text_chunks {
                        let chunk_index = chunks.len();
                        chunks.push(PreparedChunk { chunk_index, text });
                        indices.push(chunk_index);
                    }
                    if !indices.is_empty() {
                        parts.push(UnitPlanPart::Text(indices));
                    }
                }
            }
        }
        unit_plans.insert(unit.unit_id.clone(), UnitChunkPlan { parts });
    }
    PreparedChunks { chunks, unit_plans }
}

fn build_unit_outputs(
    units: &[ProviderTranslationUnit],
    prepared: &PreparedChunks,
    chunk_outputs: &[String],
    target_lang: &str,
) -> Result<Vec<PdfUnitTranslation>, String> {
    let mut outputs = Vec::with_capacity(units.len());
    for unit in units {
        let plan = prepared
            .unit_plans
            .get(&unit.unit_id)
            .ok_or_else(|| format!("missing prepared chunks for unit {}", unit.unit_id))?;
        let text = render_unit_plan(plan, chunk_outputs, target_lang)?;
        outputs.push(PdfUnitTranslation {
            unit_id: unit.unit_id.clone(),
            output_chars: text.chars().count() as u64,
            text,
        });
    }
    Ok(outputs)
}

fn emit_ready_unit_outputs(
    units: &[ProviderTranslationUnit],
    prepared: &PreparedChunks,
    chunk_outputs: &[String],
    chunk_ready: &[bool],
    target_lang: &str,
    emitted_unit_ids: &mut BTreeSet<String>,
    on_unit_translation: &mut (dyn FnMut(PdfUnitTranslation) + Send),
) -> Result<(), String> {
    for unit in units {
        if emitted_unit_ids.contains(&unit.unit_id) {
            continue;
        }
        let plan = prepared
            .unit_plans
            .get(&unit.unit_id)
            .ok_or_else(|| format!("missing prepared chunks for unit {}", unit.unit_id))?;
        if !unit_plan_ready(plan, chunk_ready) {
            continue;
        }
        let text = render_unit_plan(plan, chunk_outputs, target_lang)?;
        let translation = PdfUnitTranslation {
            unit_id: unit.unit_id.clone(),
            output_chars: text.chars().count() as u64,
            text,
        };
        emitted_unit_ids.insert(unit.unit_id.clone());
        on_unit_translation(translation);
    }
    Ok(())
}

fn unit_plan_ready(plan: &UnitChunkPlan, chunk_ready: &[bool]) -> bool {
    plan.parts.iter().all(|part| match part {
        UnitPlanPart::Placeholder(_) | UnitPlanPart::Literal(_) => true,
        UnitPlanPart::Text(indices) => indices
            .iter()
            .all(|index| chunk_ready.get(*index).copied().unwrap_or(false)),
    })
}

fn render_unit_plan(
    plan: &UnitChunkPlan,
    chunk_outputs: &[String],
    target_lang: &str,
) -> Result<String, String> {
    let compact = is_compact_target_lang(target_lang);
    let mut output = String::new();
    for part in &plan.parts {
        match part {
            UnitPlanPart::Placeholder(placeholder) => {
                append_pdf_placeholder(&mut output, placeholder, compact);
            }
            UnitPlanPart::Literal(literal) => {
                append_pdf_literal(&mut output, literal, compact);
            }
            UnitPlanPart::Text(indices) => {
                let translated_chunks = indices
                    .iter()
                    .map(|index| {
                        chunk_outputs
                            .get(*index)
                            .cloned()
                            .ok_or_else(|| format!("missing translated chunk {index}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let text = join_translated_chunks(translated_chunks, target_lang);
                append_translated_text(&mut output, &text, compact);
            }
        }
    }
    if compact {
        Ok(output.trim().to_string())
    } else {
        Ok(collapse_whitespace(&output))
    }
}

fn append_pdf_placeholder(output: &mut String, placeholder: &str, compact: bool) {
    if compact {
        output.push_str(placeholder);
        return;
    }
    if !output.is_empty() && !output.chars().next_back().is_some_and(char::is_whitespace) {
        output.push(' ');
    }
    output.push_str(placeholder);
    output.push(' ');
}

fn append_translated_text(output: &mut String, text: &str, compact: bool) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if !compact
        && !output.is_empty()
        && !output.chars().next_back().is_some_and(char::is_whitespace)
    {
        output.push(' ');
    }
    output.push_str(trimmed);
}

fn append_pdf_literal(output: &mut String, text: &str, compact: bool) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if !compact
        && !output.is_empty()
        && !output.chars().next_back().is_some_and(char::is_whitespace)
        && !trimmed
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_punctuation())
    {
        output.push(' ');
    }
    output.push_str(trimmed);
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_pdf_placeholder_parts(text: &str) -> Vec<PdfTextPart> {
    let mut parts = Vec::new();
    let mut cursor = 0usize;
    while let Some((start, end)) = find_pdf_placeholder(text, cursor) {
        if start > cursor {
            parts.push(PdfTextPart::Text(text[cursor..start].to_string()));
        }
        parts.push(PdfTextPart::Placeholder(text[start..end].to_string()));
        cursor = end;
    }
    if cursor < text.len() {
        parts.push(PdfTextPart::Text(text[cursor..].to_string()));
    }
    if parts.is_empty() {
        parts.push(PdfTextPart::Text(text.to_string()));
    }
    parts
}

fn find_pdf_placeholder(text: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut index = from;
    while index + 3 < bytes.len() {
        if bytes[index] == b'{' && bytes[index + 1] == b'v' {
            let mut end = index + 2;
            let first_digit = end;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > first_digit && end < bytes.len() && bytes[end] == b'}' {
                return Some((index, end + 1));
            }
        }
        index += 1;
    }
    None
}

fn should_passthrough_pdf_fragment(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let alphabetic_count = trimmed.chars().filter(|ch| ch.is_alphabetic()).count();
    alphabetic_count == 0 || (alphabetic_count <= 2 && trimmed.chars().all(|ch| ch.is_ascii()))
}

fn split_pdf_text_for_lightning(text: &str) -> Vec<String> {
    split_reference_entries(text)
        .into_iter()
        .flat_map(|entry| {
            split_pdf_text_with_budget(
                &entry,
                LIGHTNING_TARGET_PROMPT_TOKENS,
                LIGHTNING_HARD_PROMPT_TOKENS,
            )
        })
        .filter(|chunk| !chunk.trim().is_empty())
        .collect()
}

fn split_reference_entries(text: &str) -> Vec<String> {
    let normalized = normalize_pdf_text(text);
    let trimmed = normalized.trim();
    let markers = numeric_reference_markers(trimmed);
    if !markers.windows(2).any(|window| {
        window[0].1.checked_add(1) == Some(window[1].1)
            && window[1].0.saturating_sub(window[0].0) >= REFERENCE_ENTRY_MIN_CHARS
    }) {
        return vec![trimmed.to_string()];
    }

    let mut entries = Vec::with_capacity(markers.len() + 1);
    if markers[0].0 > 0 {
        let prefix = trimmed[..markers[0].0].trim();
        if !prefix.is_empty() {
            entries.push(prefix.to_string());
        }
    }
    for (index, (start, _)) in markers.iter().enumerate() {
        let end = markers
            .get(index + 1)
            .map(|marker| marker.0)
            .unwrap_or(trimmed.len());
        let entry = trimmed[*start..end].trim();
        if !entry.is_empty() {
            entries.push(entry.to_string());
        }
    }
    entries
}

fn numeric_reference_markers(text: &str) -> Vec<(usize, u32)> {
    let bytes = text.as_bytes();
    let mut markers = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        let digits_start = end;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end > digits_start && end < bytes.len() && bytes[end] == b']' {
            if let Ok(number) = text[digits_start..end].parse::<u32>() {
                markers.push((index, number));
            }
            index = end + 1;
        } else {
            index += 1;
        }
    }
    markers
}

fn split_pdf_text(text: &str) -> Vec<String> {
    split_pdf_text_with_budget(
        text,
        NON_LIGHTNING_TARGET_PROMPT_TOKENS,
        NON_LIGHTNING_HARD_PROMPT_TOKENS,
    )
}

fn split_pdf_text_with_budget(text: &str, target_tokens: usize, hard_tokens: usize) -> Vec<String> {
    let normalized = normalize_pdf_text(text);
    let trimmed = normalized.trim();
    if trimmed.is_empty() || estimate_prompt_tokens(trimmed) <= hard_tokens {
        return vec![trimmed.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for sentence in split_sentences(trimmed) {
        let candidate = if current.is_empty() {
            sentence.clone()
        } else {
            format!("{} {}", current, sentence)
        };
        if estimate_prompt_tokens(&candidate) <= target_tokens {
            current = candidate;
        } else {
            if !current.trim().is_empty() {
                chunks.push(current.trim().to_string());
            }
            current = sentence;
        }
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
        .into_iter()
        .flat_map(|chunk| split_oversized_chunk_with_budget(chunk, target_tokens, hard_tokens))
        .filter(|chunk| !chunk.trim().is_empty())
        .collect()
}

fn split_oversized_chunk_with_budget(
    chunk: String,
    target_tokens: usize,
    hard_tokens: usize,
) -> Vec<String> {
    if estimate_prompt_tokens(&chunk) <= hard_tokens {
        return vec![chunk];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in chunk.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if estimate_prompt_tokens(&candidate) <= target_tokens {
            current = candidate;
        } else {
            if !current.trim().is_empty() {
                chunks.push(current);
            }
            current = word.to_string();
        }
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut start = 0usize;
    for (index, ch) in text.char_indices() {
        if is_sentence_boundary(text, index, ch) {
            let end = index + ch.len_utf8();
            let unit = text[start..end].trim();
            if !unit.is_empty() {
                units.push(unit.to_string());
            }
            start = end;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        units.push(tail.to_string());
    }
    if units.is_empty() {
        vec![text.to_string()]
    } else {
        units
    }
}

fn normalize_pdf_text(text: &str) -> String {
    text.replace("- ", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
    next.is_none_or(char::is_whitespace)
}

fn estimate_prompt_tokens(text: &str) -> usize {
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

fn ensure_not_cancelled(cancel: Option<&Arc<AtomicBool>>) -> Result<(), String> {
    if cancel.is_some_and(|cancel| cancel.load(Ordering::SeqCst)) {
        Err("PDF unit translation cancelled.".to_string())
    } else {
        Ok(())
    }
}

fn add_batch_bucket(distribution: &mut Vec<PdfBatchSizeBucket>, batch_size: u64) {
    if let Some(bucket) = distribution
        .iter_mut()
        .find(|bucket| bucket.batch_size == batch_size)
    {
        bucket.request_count += 1;
    } else {
        distribution.push(PdfBatchSizeBucket {
            batch_size,
            request_count: 1,
        });
    }
    distribution.sort_by_key(|bucket| bucket.batch_size);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(unit_id: &str, text: &str) -> ProviderTranslationUnit {
        ProviderTranslationUnit {
            unit_id: unit_id.to_string(),
            source_text: text.to_string(),
            requires_translation: true,
            chunking: ProviderUnitChunking::Semantic,
        }
    }

    #[test]
    fn visual_paragraph_uses_one_provider_chunk_until_a_real_provider_limit_retry() {
        let source = "A complete visual paragraph with several sentences. ".repeat(20);
        let unit = ProviderTranslationUnit {
            unit_id: "visual-paragraph".to_string(),
            source_text: source.clone(),
            requires_translation: true,
            chunking: ProviderUnitChunking::VisualParagraph,
        };
        let prepared = prepare_non_lightning_chunks(&[unit]);
        assert_eq!(prepared.chunks.len(), 1);
        assert_eq!(prepared.chunks[0].text, source.trim());
    }

    fn visual_paragraph_unit(
        unit_id: &str,
        text: &str,
    ) -> crate::pdf_v3::paragraph_translation_plan::VisualParagraphUnitPlan {
        crate::pdf_v3::paragraph_translation_plan::VisualParagraphUnitPlan {
            unit_id: unit_id.to_string(),
            paragraph_group_id: format!("{unit_id}-group"),
            flow_container_group_id: "container-1".to_string(),
            order_on_page: 1,
            atom_ids: vec![format!("{unit_id}-atom")],
            source_text: text.to_string(),
            provider_text: text.to_string(),
            protected_spans: Vec::new(),
        }
    }

    fn non_translation_unit(unit_id: &str, text: &str) -> PdfTranslationUnit {
        PdfTranslationUnit {
            unit_id: unit_id.to_string(),
            page_number: 1,
            order_on_page: 1,
            source_text: text.to_string(),
            source_chars: text.chars().count() as u64,
            kind: "body".to_string(),
            requires_translation: false,
        }
    }

    #[test]
    fn split_pdf_text_breaks_oversized_text_for_non_lightning() {
        let text = "This is a sentence about PDF translation stability. ".repeat(30);
        let chunks = split_pdf_text(&text);

        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| estimate_prompt_tokens(chunk) <= NON_LIGHTNING_HARD_PROMPT_TOKENS));
    }

    #[test]
    fn lightning_splits_long_text_within_its_prompt_budget() {
        let text = "This is a sentence about PDF translation stability. ".repeat(80);
        let chunks = split_pdf_text_for_lightning(&text);

        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| estimate_prompt_tokens(chunk) <= LIGHTNING_HARD_PROMPT_TOKENS));
    }

    #[test]
    fn lightning_splits_consecutive_reference_entries_before_translation() {
        let source = concat!(
            ": Squeeze-enhanced axial transformer for mobile visual recognition. ",
            "International Journal of Computer Vision, pages 1-22, 2025. 6 ",
            "[44] Haoyang Wang et al. Wcg-vmamba. 2 ",
            "[45] Qianning Wang et al. Insectmamba. 3 ",
            "[46] Xiaoping Wu et al. Ip102. 6, 7 ",
            "[47] Chengjun Xie et al. Automatic classification. 6 ",
            "[48] Fei Xie et al. Quadmamba. 4 ",
            "[49] Saining Xie, Ross Girshick, Piotr Doll"
        );
        let chunks = split_pdf_text_for_lightning(source);

        assert_eq!(chunks.len(), 7);
        assert!(chunks[0].starts_with(": Squeeze-enhanced"));
        for (offset, chunk) in chunks.iter().skip(1).enumerate() {
            assert!(chunk.starts_with(&format!("[{}]", 44 + offset)));
        }
    }

    #[test]
    fn prose_citations_do_not_trigger_reference_list_splitting() {
        let source = "Prior work [2] introduced the task, while [8] and [19] improved it.";

        assert_eq!(split_reference_entries(source), vec![source.to_string()]);
    }

    #[test]
    fn adjacent_inline_citations_do_not_trigger_reference_list_splitting() {
        let source = "The baselines [1] and [2] use different visual encoders.";

        assert_eq!(split_reference_entries(source), vec![source.to_string()]);
    }

    #[test]
    fn lightning_splits_two_long_consecutive_reference_entries() {
        let source = concat!(
            "A continuation from the previous reference. ",
            "[50] Zhiwen Yang, Jiayin Li, Hui Zhang, Dan Zhao, Bingzheng Wei, and Yan Xu. ",
            "Restore-rwkv: Efficient and effective medical image restoration with rwkv. 2, 3, 5 ",
            "[51] Haobo Yuan, Xiangtai Li, Lu Qi, Tao Zhang, Ming-Hsuan Yang, Shuicheng Yan, ",
            "and Chen Change Loy. Mamba or rwkv: Exploring efficient segmentation models."
        );
        let chunks = split_reference_entries(source);

        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].starts_with("A continuation"));
        assert!(chunks[1].starts_with("[50]"));
        assert!(chunks[2].starts_with("[51]"));
    }

    #[test]
    fn trivial_pdf_fragments_bypass_the_translation_provider() {
        let units = [
            unit("number", "2."),
            unit("letters", "el"),
            unit("colon", ":"),
            unit("period", "."),
        ];
        let prepared = prepare_lightning_chunks(&units);

        assert!(prepared.chunks.is_empty());
        let output = build_unit_outputs(&units, &prepared, &[], "zh-CN").expect("build output");
        assert_eq!(
            output
                .iter()
                .map(|translation| translation.text.as_str())
                .collect::<Vec<_>>(),
            vec!["2.", "el", ":", "."]
        );
    }

    #[test]
    fn split_retry_budget_breaks_medium_chunks_more_aggressively() {
        let text = &"Alpha beta gamma delta epsilon zeta eta theta. ".repeat(4);
        assert_eq!(split_pdf_text(text).len(), 1);

        let retry_chunks = split_pdf_text_with_budget(
            text,
            NON_LIGHTNING_RETRY_TARGET_PROMPT_TOKENS,
            NON_LIGHTNING_RETRY_HARD_PROMPT_TOKENS,
        );

        assert!(retry_chunks.len() > 1);
        assert!(retry_chunks
            .iter()
            .all(|chunk| estimate_prompt_tokens(chunk) <= NON_LIGHTNING_RETRY_HARD_PROMPT_TOKENS));
    }

    #[tokio::test]
    async fn llama_limit_failure_retries_with_smaller_pdf_chunks() {
        let text = &"Alpha beta gamma delta epsilon zeta eta theta. ".repeat(4);
        let retry_chunks = split_pdf_text_with_budget(
            text,
            NON_LIGHTNING_RETRY_TARGET_PROMPT_TOKENS,
            NON_LIGHTNING_RETRY_HARD_PROMPT_TOKENS,
        );
        let scripted = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::VecDeque::from(vec![
                provider_error(
                    "llama.cpp completion was truncated (truncated=true, stop_type=limit)",
                ),
                provider_success(vec!["甲".to_string(); retry_chunks.len()]),
            ]),
        ));
        let provider = ProviderKind::Scripted(scripted);
        let mut metrics = PdfUnitTranslationMetrics::default();
        let mut emitted = Vec::new();

        let translations = translate_units_provider_batches(
            provider,
            "en",
            "zh-CN",
            &[unit("a", text)],
            1,
            None,
            &mut metrics,
            &mut |translation| emitted.push(translation),
        )
        .await
        .expect("split retry should recover");

        assert_eq!(translations.len(), 1);
        assert_eq!(translations[0].text, "甲".repeat(retry_chunks.len()));
        assert_eq!(emitted.len(), 1);
        assert_eq!(metrics.request_count, 2);
        assert_eq!(metrics.failed_request_count, 1);
        assert_eq!(metrics.truncated_count, 1);
    }

    #[tokio::test]
    async fn non_required_pdf_units_emit_empty_passthrough_translations() {
        let provider = PdfUnitProviderConfig::LlamaCpp(LlamaCppPdfApiConfig {
            base_url: "http://127.0.0.1:1".to_string(),
            timeout_ms: 1,
        });
        let mut emitted = Vec::new();

        let result = translate_pdf_units_with_events(
            &provider,
            "en",
            "zh-CN",
            &[non_translation_unit("dup", "Duplicate text layer")],
            None,
            &mut |translation| emitted.push(translation),
        )
        .await
        .expect("non-required unit should not call provider");

        assert_eq!(result.translations.len(), 1);
        assert_eq!(result.translations[0].unit_id, "dup");
        assert_eq!(result.translations[0].text, "");
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].unit_id, "dup");
        assert_eq!(emitted[0].text, "");
        assert_eq!(result.metrics.request_count, 0);
    }

    #[tokio::test]
    async fn pdf_v3_plan_uses_provider_text_and_returns_identity_keyed_results() {
        let scripted = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
            vec![provider_success(vec!["甲".to_string(), "乙".to_string()])],
        )));
        let provider = PdfUnitProviderConfig::Scripted {
            results: Arc::clone(&scripted),
            max_batch_size: 8,
        };
        let plan = pdf_v3_plan(vec![("unit-a", "Alpha {v0} beta."), ("unit-b", "2.")]);

        let translated = translate_pdf_v3_page_plan(&provider, "en", "zh-CN", &plan, None)
            .await
            .expect("translate PDF v3 plan");

        assert_eq!(translated.results.len(), 2);
        assert_eq!(translated.results[0].unit_id, "unit-a");
        assert_eq!(translated.results[0].translated_text, "甲{v0}乙");
        assert_eq!(translated.results[1].unit_id, "unit-b");
        assert_eq!(translated.results[1].translated_text, "2.");
        assert_eq!(translated.metrics.request_count, 1);
        assert_eq!(translated.metrics.total_input_chars, 10);
        assert_eq!(scripted.lock().expect("scripted queue").len(), 0);
    }

    #[tokio::test]
    async fn visual_paragraph_plan_batches_clean_paragraphs_instead_of_source_objects() {
        let scripted = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
            vec![provider_success(vec![
                "完整段落甲".to_string(),
                "完整段落乙".to_string(),
            ])],
        )));
        let provider = PdfUnitProviderConfig::Scripted {
            results: Arc::clone(&scripted),
            max_batch_size: 16,
        };
        let plan = VisualParagraphPagePlan {
            schema_version: VISUAL_PARAGRAPH_PLAN_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:visual-paragraph-provider".to_string(),
            units: vec![
                visual_paragraph_unit("paragraph-a", "Clean paragraph one."),
                visual_paragraph_unit("paragraph-b", "Clean paragraph two."),
            ],
            preserved_containers: Vec::new(),
        };

        let translated =
            translate_pdf_v3_visual_paragraph_plan(&provider, "en", "zh-CN", &plan, None)
                .await
                .expect("translate visual paragraph plan");

        assert_eq!(translated.results.len(), 2);
        assert_eq!(translated.results[0].translated_text, "完整段落甲");
        assert_eq!(translated.results[1].translated_text, "完整段落乙");
        assert_eq!(translated.metrics.request_count, 1);
        assert_eq!(translated.metrics.total_input_chars, 40);
        assert_eq!(scripted.lock().expect("scripted queue").len(), 0);
    }

    #[tokio::test]
    async fn pdf_v3_plan_cancellation_stops_before_provider_request() {
        let scripted = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
            vec![provider_success(vec!["甲".to_string()])],
        )));
        let provider = PdfUnitProviderConfig::Scripted {
            results: Arc::clone(&scripted),
            max_batch_size: 1,
        };
        let cancel = Arc::new(AtomicBool::new(true));
        let plan = pdf_v3_plan(vec![("unit-a", "Translate me")]);

        let failure = translate_pdf_v3_page_plan(&provider, "en", "zh-CN", &plan, Some(cancel))
            .await
            .expect_err("cancelled provider request");

        assert_eq!(failure.kind, PdfV3ProviderFailureKind::Cancelled);
        assert!(failure.retryable);
        assert_eq!(scripted.lock().expect("scripted queue").len(), 1);
    }

    #[tokio::test]
    async fn pdf_v3_empty_or_duplicate_unit_plan_is_rejected_without_provider_io() {
        let scripted = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
            vec![provider_success(Vec::new())],
        )));
        let provider = PdfUnitProviderConfig::Scripted {
            results: Arc::clone(&scripted),
            max_batch_size: 1,
        };
        let empty = pdf_v3_plan(Vec::new());

        let empty_failure = translate_pdf_v3_page_plan(&provider, "en", "zh-CN", &empty, None)
            .await
            .expect_err("empty plan");
        assert_eq!(
            empty_failure.kind,
            PdfV3ProviderFailureKind::NoTranslatableUnits
        );
        assert!(!empty_failure.retryable);

        let duplicate = pdf_v3_plan(vec![("unit-a", "First"), ("unit-a", "Second")]);
        let duplicate_failure =
            translate_pdf_v3_page_plan(&provider, "en", "zh-CN", &duplicate, None)
                .await
                .expect_err("duplicate unit identity");
        assert_eq!(
            duplicate_failure.kind,
            PdfV3ProviderFailureKind::InvalidPlan
        );
        assert!(!duplicate_failure.retryable);
        assert_eq!(scripted.lock().expect("scripted queue").len(), 1);
    }

    #[test]
    fn duplicate_source_text_keeps_distinct_unit_ids() {
        let prepared = prepare_non_lightning_chunks(&[unit("a", "Repeat."), unit("b", "Repeat.")]);

        assert_ne!(
            text_part_indices(prepared.unit_plans.get("a").unwrap()),
            text_part_indices(prepared.unit_plans.get("b").unwrap())
        );
    }

    #[test]
    fn chinese_join_has_no_spaces() {
        assert_eq!(
            join_translated_chunks(
                vec!["第一句。".to_string(), "第二句。".to_string()],
                "zh-CN"
            ),
            "第一句。第二句。"
        );
    }

    #[test]
    fn pdf_placeholders_are_split_from_model_input() {
        let parts = split_pdf_placeholder_parts("Alpha {v0} beta {v12}.");

        assert_eq!(
            parts,
            vec![
                PdfTextPart::Text("Alpha ".to_string()),
                PdfTextPart::Placeholder("{v0}".to_string()),
                PdfTextPart::Text(" beta ".to_string()),
                PdfTextPart::Placeholder("{v12}".to_string()),
                PdfTextPart::Text(".".to_string()),
            ]
        );
    }

    #[test]
    fn placeholder_units_preserve_tokens_without_sending_them_to_model() {
        let prepared = prepare_non_lightning_chunks(&[unit("a", "Alpha {v0} beta {v12}.")]);
        let chunk_texts = prepared
            .chunks
            .iter()
            .map(|chunk| chunk.text.as_str())
            .collect::<Vec<_>>();

        assert_eq!(chunk_texts, vec!["Alpha", "beta"]);
        assert!(chunk_texts.iter().all(|chunk| !chunk.contains("{v")));

        let output = build_unit_outputs(
            &[unit("a", "Alpha {v0} beta {v12}.")],
            &prepared,
            &["甲".to_string(), "乙".to_string()],
            "zh-CN",
        )
        .expect("build output");

        assert_eq!(output[0].text, "甲{v0}乙{v12}.");
    }

    #[test]
    fn structural_line_break_placeholders_split_and_reassemble_list_items() {
        let marker = "{v900000000}";
        let source = format!("Title Page{marker}Gateway Introduction{marker}Preface");
        let prepared = prepare_non_lightning_chunks(&[unit("a", &source)]);
        let chunk_texts = prepared
            .chunks
            .iter()
            .map(|chunk| chunk.text.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            chunk_texts,
            vec!["Title Page", "Gateway Introduction", "Preface"]
        );
        let output = build_unit_outputs(
            &[unit("a", &source)],
            &prepared,
            &[
                "标题页".to_string(),
                "门户介绍".to_string(),
                "前言".to_string(),
            ],
            "zh-CN",
        )
        .expect("build structural line break output");

        assert_eq!(
            output[0].text,
            format!("标题页{marker}门户介绍{marker}前言")
        );
    }

    #[test]
    fn placeholder_only_unit_is_reconstructed_without_provider_chunks() {
        let prepared = prepare_non_lightning_chunks(&[unit("a", "{v0} {v1}")]);

        assert!(prepared.chunks.is_empty());
        let output = build_unit_outputs(&[unit("a", "{v0} {v1}")], &prepared, &[], "zh-CN")
            .expect("build output");

        assert_eq!(output[0].text, "{v0}{v1}");
    }

    #[test]
    fn ready_unit_event_waits_until_all_chunks_are_available() {
        let units = [unit(
            "a",
            &"This is a sentence about progressive PDF page rendering. ".repeat(30),
        )];
        let prepared = prepare_non_lightning_chunks(&units);
        let plan = prepared.unit_plans.get("a").unwrap();
        let chunk_indices = text_part_indices(plan);
        assert!(chunk_indices.len() > 1);

        let mut chunk_outputs = vec![String::new(); prepared.chunks.len()];
        let mut chunk_ready = vec![false; prepared.chunks.len()];
        let mut emitted_unit_ids = BTreeSet::new();
        let mut emitted = Vec::new();
        chunk_outputs[chunk_indices[0]] = "第一段".to_string();
        chunk_ready[chunk_indices[0]] = true;

        emit_ready_unit_outputs(
            &units,
            &prepared,
            &chunk_outputs,
            &chunk_ready,
            "zh-CN",
            &mut emitted_unit_ids,
            &mut |translation| emitted.push(translation),
        )
        .expect("emit partial");

        assert!(emitted.is_empty());

        for index in chunk_indices.iter().skip(1) {
            chunk_outputs[*index] = "后续段".to_string();
            chunk_ready[*index] = true;
        }

        emit_ready_unit_outputs(
            &units,
            &prepared,
            &chunk_outputs,
            &chunk_ready,
            "zh-CN",
            &mut emitted_unit_ids,
            &mut |translation| emitted.push(translation),
        )
        .expect("emit complete");

        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].unit_id, "a");
    }

    #[test]
    fn placeholder_only_unit_event_is_ready_without_provider_chunks() {
        let units = [unit("a", "{v0} {v1}")];
        let prepared = prepare_non_lightning_chunks(&units);
        let mut emitted_unit_ids = BTreeSet::new();
        let mut emitted = Vec::new();

        emit_ready_unit_outputs(
            &units,
            &prepared,
            &[],
            &[],
            "zh-CN",
            &mut emitted_unit_ids,
            &mut |translation| emitted.push(translation),
        )
        .expect("emit placeholder-only");

        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].unit_id, "a");
        assert_eq!(emitted[0].text, "{v0}{v1}");
    }

    fn text_part_indices(plan: &UnitChunkPlan) -> Vec<usize> {
        plan.parts
            .iter()
            .filter_map(|part| match part {
                UnitPlanPart::Text(indices) => Some(indices.clone()),
                UnitPlanPart::Placeholder(_) | UnitPlanPart::Literal(_) => None,
            })
            .flatten()
            .collect()
    }

    fn pdf_v3_plan(units: Vec<(&str, &str)>) -> TranslationPagePlan {
        TranslationPagePlan {
            schema_version: TRANSLATION_PLAN_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:test-page".to_string(),
            units: units
                .into_iter()
                .enumerate()
                .map(|(order, (unit_id, provider_text))| TranslationUnitPlan {
                    unit_id: unit_id.to_string(),
                    order_on_page: u32::try_from(order).expect("test order"),
                    atom_ids: vec![format!("atom-{order}")],
                    source_text: provider_text.to_string(),
                    provider_text: provider_text.to_string(),
                    protected_spans: Vec::new(),
                })
                .collect(),
            preserved_regions: Vec::new(),
        }
    }

    fn provider_success(translations: Vec<String>) -> ProviderTranslateResult {
        ProviderTranslateResult {
            ok: true,
            status_code: Some(200),
            translations,
            raw_response_preview: String::new(),
            message: "ok".to_string(),
            latency_ms: 0,
        }
    }

    fn provider_error(message: &str) -> ProviderTranslateResult {
        ProviderTranslateResult {
            ok: false,
            status_code: Some(200),
            translations: Vec::new(),
            raw_response_preview: String::new(),
            message: message.to_string(),
            latency_ms: 0,
        }
    }
}
