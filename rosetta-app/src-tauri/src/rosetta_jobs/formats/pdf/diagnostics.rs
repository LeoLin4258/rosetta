//! Per-run PDF translation diagnostics.
//!
//! Each `translate_rosetta_pdf_pages` run writes one JSON profile under
//! `<job_dir>/diagnostics/`. The profile separates RWKV model time from
//! pdf2zh process time (startup, parse/layout, render) so performance work
//! can target the real bottleneck. Counts, durations and page numbers only —
//! never source or translated text.

use std::{fs::OpenOptions, io::Write, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::rosetta_jobs::model::SCHEMA_VERSION;

use super::unit_translation::{PdfBatchSizeBucket, PdfUnitTranslationMetrics};

pub(crate) const PDF_TIMELINE_FILENAME: &str = "pdf-timeline.jsonl";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfTranslationProfile {
    pub schema_version: u32,
    pub run_id: String,
    pub job_id: String,
    /// `completed`, `cancelled`, or `failed`.
    pub status: String,
    pub source_lang: String,
    pub target_lang: String,
    pub page_selection: String,
    pub pages_requested: u32,
    pub pages_translated: u32,
    pub pages_failed: u32,
    pub started_at: String,
    pub ended_at: String,
    pub durations_ms: PdfTranslationDurations,
    /// Number of pdf2zh process invocations in this run.
    pub invocation_count: u32,
    /// Number of invocations that reused the worker's active prepared PDF.
    pub prepared_cache_hits: u32,
    /// Number of page-window render calls made to the worker.
    pub render_call_count: u32,
    /// Aggregated RWKV request stats across all invocations. `None` when the
    /// run was cancelled/failed before any invocation finished.
    pub rwkv: Option<RwkvAggregate>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfTranslationDurations {
    /// Wall time of the whole command.
    pub total: u64,
    /// Sum of per-invocation PDF prepare wall time, including worker protocol
    /// overhead and cache reset time.
    pub pdf2zh_warmup: u64,
    pub pdf_prepare_font_assets: u64,
    pub pdf_prepare_document: u64,
    pub pdf_layout: u64,
    pub pdf_unit_collection: u64,
    pub pdf_prepare_other: u64,
    pub pdf_prepare_cache_reset: u64,
    pub pdf_render: u64,
    /// Sum of per-invocation pdf2zh process wall time (parse + layout +
    /// translate + render). RWKV time happens inside this window; subtract
    /// `rwkv.totalRequestMs` for a lower bound on pure PDF processing.
    pub pdf2zh_process: u64,
    /// Splitting batch output into per-page PDFs under `translated-pages/`.
    pub page_artifact_assembly: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RwkvAggregate {
    pub request_count: u64,
    pub failed_request_count: u64,
    pub total_request_ms: u64,
    pub average_request_ms: u64,
    pub max_request_ms: u64,
    pub p95_request_ms: u64,
    pub total_batch_items: u64,
    pub average_batch_size: f64,
    pub batch_size_distribution: Vec<PdfBatchSizeBucket>,
    pub total_assembly_wait_ms: u64,
    pub average_assembly_wait_ms: u64,
    pub max_assembly_wait_ms: u64,
    pub total_input_chars: u64,
    pub total_output_chars: u64,
}

impl RwkvAggregate {
    pub(crate) fn add(&mut self, snapshot: &PdfUnitTranslationMetrics) {
        self.request_count += snapshot.request_count;
        self.failed_request_count += snapshot.failed_request_count;
        self.total_request_ms += snapshot.total_request_ms;
        self.max_request_ms = self.max_request_ms.max(snapshot.max_request_ms);
        self.p95_request_ms = self.p95_request_ms.max(snapshot.max_request_ms);
        self.total_batch_items += snapshot
            .batch_size_distribution
            .iter()
            .map(|bucket| bucket.batch_size * bucket.request_count)
            .sum::<u64>();
        merge_batch_size_distribution(
            &mut self.batch_size_distribution,
            &snapshot.batch_size_distribution,
        );
        self.total_input_chars += snapshot.total_input_chars;
        self.total_output_chars += snapshot.total_output_chars;
        self.average_request_ms = if self.request_count > 0 {
            self.total_request_ms / self.request_count
        } else {
            0
        };
        self.average_batch_size = if self.request_count > 0 {
            self.total_batch_items as f64 / self.request_count as f64
        } else {
            0.0
        };
        self.average_assembly_wait_ms = if self.request_count > 0 {
            self.total_assembly_wait_ms / self.request_count
        } else {
            0
        };
    }
}

fn merge_batch_size_distribution(
    aggregate: &mut Vec<PdfBatchSizeBucket>,
    snapshot: &[PdfBatchSizeBucket],
) {
    for bucket in snapshot {
        if let Some(existing) = aggregate
            .iter_mut()
            .find(|existing| existing.batch_size == bucket.batch_size)
        {
            existing.request_count += bucket.request_count;
        } else {
            aggregate.push(*bucket);
        }
    }
    aggregate.sort_by_key(|bucket| bucket.batch_size);
}

pub(crate) fn new_profile(
    run_id: &str,
    job_id: &str,
    source_lang: &str,
    target_lang: &str,
    page_selection: &str,
    pages_requested: u32,
    started_at: String,
) -> PdfTranslationProfile {
    PdfTranslationProfile {
        schema_version: SCHEMA_VERSION,
        run_id: run_id.to_string(),
        job_id: job_id.to_string(),
        status: "completed".to_string(),
        source_lang: source_lang.to_string(),
        target_lang: target_lang.to_string(),
        page_selection: page_selection.to_string(),
        pages_requested,
        pages_translated: 0,
        pages_failed: 0,
        started_at,
        ended_at: String::new(),
        durations_ms: PdfTranslationDurations::default(),
        invocation_count: 0,
        prepared_cache_hits: 0,
        render_call_count: 0,
        rwkv: None,
    }
}

/// Best-effort profile write; diagnostics must never fail a translation run.
pub(crate) fn write_profile(job_dir: &Path, profile: &PdfTranslationProfile) {
    let dir = job_dir.join("diagnostics");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("pdf-translation-profile-{}.json", profile.run_id));
    if let Ok(json) = serde_json::to_vec_pretty(profile) {
        let _ = std::fs::write(path, json);
    }
}

/// Append one PDF lifecycle event to `<job_dir>/diagnostics/pdf-timeline.jsonl`.
///
/// The timeline is diagnostic-only and must not become a source of truth for
/// job/page state. Keep details to counts, timings, IDs, and file sizes; never
/// write source text, translated text, prompts, or model responses here.
pub(crate) fn append_timeline_event(job_dir: &Path, event: PdfTimelineEvent) {
    let dir = job_dir.join("diagnostics");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(PDF_TIMELINE_FILENAME);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    if let Ok(mut line) = serde_json::to_string(&event) {
        line.push('\n');
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfTimelineEvent {
    pub schema_version: u32,
    pub timestamp_ms: String,
    pub job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub phase: String,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl PdfTimelineEvent {
    pub(crate) fn new(job_id: &str, phase: &str, event: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            timestamp_ms: crate::rosetta_jobs::path::timestamp_ms_string(),
            job_id: job_id.to_string(),
            run_id: None,
            phase: phase.to_string(),
            event: event.to_string(),
            target_lang: None,
            page_number: None,
            duration_ms: None,
            details: Value::Null,
        }
    }

    pub(crate) fn run_id(mut self, run_id: &str) -> Self {
        self.run_id = Some(run_id.to_string());
        self
    }

    pub(crate) fn target_lang(mut self, target_lang: &str) -> Self {
        self.target_lang = Some(target_lang.to_string());
        self
    }

    pub(crate) fn page_number(mut self, page_number: u32) -> Self {
        self.page_number = Some(page_number);
        self
    }

    pub(crate) fn duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    pub(crate) fn details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

pub(crate) fn rwkv_snapshot_details(snapshot: &PdfUnitTranslationMetrics) -> Value {
    json!({
        "requestCount": snapshot.request_count,
        "failedRequestCount": snapshot.failed_request_count,
        "totalRequestMs": snapshot.total_request_ms,
        "averageRequestMs": if snapshot.request_count > 0 {
            snapshot.total_request_ms / snapshot.request_count
        } else {
            0
        },
        "maxRequestMs": snapshot.max_request_ms,
        "p95RequestMs": snapshot.max_request_ms,
        "totalBatchItems": snapshot.batch_size_distribution.iter().map(|bucket| bucket.batch_size * bucket.request_count).sum::<u64>(),
        "averageBatchSize": if snapshot.request_count > 0 {
            snapshot.batch_size_distribution.iter().map(|bucket| bucket.batch_size * bucket.request_count).sum::<u64>() as f64 / snapshot.request_count as f64
        } else {
            0.0
        },
        "batchSizeDistribution": snapshot.batch_size_distribution,
        "totalAssemblyWaitMs": 0,
        "averageAssemblyWaitMs": 0,
        "maxAssemblyWaitMs": 0,
        "totalInputChars": snapshot.total_input_chars,
        "totalOutputChars": snapshot.total_output_chars,
        "truncatedCount": snapshot.truncated_count,
        "emptyOutputCount": snapshot.empty_output_count,
    })
}

pub(crate) fn rwkv_aggregate_details(aggregate: &RwkvAggregate) -> Value {
    json!({
        "requestCount": aggregate.request_count,
        "failedRequestCount": aggregate.failed_request_count,
        "totalRequestMs": aggregate.total_request_ms,
        "averageRequestMs": aggregate.average_request_ms,
        "maxRequestMs": aggregate.max_request_ms,
        "p95RequestMs": aggregate.p95_request_ms,
        "totalBatchItems": aggregate.total_batch_items,
        "averageBatchSize": aggregate.average_batch_size,
        "batchSizeDistribution": aggregate.batch_size_distribution,
        "totalAssemblyWaitMs": aggregate.total_assembly_wait_ms,
        "averageAssemblyWaitMs": aggregate.average_assembly_wait_ms,
        "maxAssemblyWaitMs": aggregate.max_assembly_wait_ms,
        "totalInputChars": aggregate.total_input_chars,
        "totalOutputChars": aggregate.total_output_chars,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rwkv_aggregate_sums_and_averages() {
        let mut agg = RwkvAggregate::default();
        agg.add(&PdfUnitTranslationMetrics {
            request_count: 4,
            failed_request_count: 1,
            total_request_ms: 4000,
            max_request_ms: 2100,
            batch_size_distribution: vec![PdfBatchSizeBucket {
                batch_size: 4,
                request_count: 4,
            }],
            total_input_chars: 800,
            total_output_chars: 700,
            ..PdfUnitTranslationMetrics::default()
        });
        agg.add(&PdfUnitTranslationMetrics {
            request_count: 6,
            failed_request_count: 0,
            total_request_ms: 3000,
            max_request_ms: 900,
            batch_size_distribution: vec![
                PdfBatchSizeBucket {
                    batch_size: 4,
                    request_count: 2,
                },
                PdfBatchSizeBucket {
                    batch_size: 8,
                    request_count: 4,
                },
            ],
            total_input_chars: 1200,
            total_output_chars: 1100,
            ..PdfUnitTranslationMetrics::default()
        });
        assert_eq!(agg.request_count, 10);
        assert_eq!(agg.failed_request_count, 1);
        assert_eq!(agg.total_request_ms, 7000);
        assert_eq!(agg.average_request_ms, 700);
        assert_eq!(agg.max_request_ms, 2100);
        assert_eq!(agg.p95_request_ms, 2100);
        assert_eq!(agg.total_batch_items, 56);
        assert_eq!(agg.average_batch_size, 5.6);
        assert_eq!(
            agg.batch_size_distribution
                .iter()
                .map(|bucket| (bucket.batch_size, bucket.request_count))
                .collect::<Vec<_>>(),
            vec![(4, 6), (8, 4)]
        );
        assert_eq!(agg.total_assembly_wait_ms, 0);
        assert_eq!(agg.average_assembly_wait_ms, 0);
        assert_eq!(agg.max_assembly_wait_ms, 0);
        assert_eq!(agg.total_input_chars, 2000);
        assert_eq!(agg.total_output_chars, 1800);
    }

    #[test]
    fn profile_json_contains_no_text_fields() {
        let profile = new_profile("run-x", "job-y", "en", "zh-CN", "1-3", 3, "0".to_string());
        let json = serde_json::to_string(&profile).expect("serialize profile");
        for forbidden in ["sourceText", "translatedText", "content", "messages"] {
            assert!(
                !json.contains(forbidden),
                "profile JSON must not contain `{forbidden}`"
            );
        }
    }

    #[test]
    fn timeline_event_json_contains_no_text_fields() {
        let event = PdfTimelineEvent::new("job-y", "translation", "run.completed")
            .run_id("run-x")
            .target_lang("zh-CN")
            .duration_ms(1200)
            .details(json!({
                "pagesRequested": 3,
                "pagesTranslated": 3,
                "rwkv": rwkv_aggregate_details(&RwkvAggregate {
                    request_count: 2,
                    failed_request_count: 0,
                    total_request_ms: 900,
                    average_request_ms: 450,
                    max_request_ms: 500,
                    p95_request_ms: 500,
                    total_batch_items: 8,
                    average_batch_size: 4.0,
                    batch_size_distribution: vec![PdfBatchSizeBucket {
                        batch_size: 4,
                        request_count: 2,
                    }],
                    total_assembly_wait_ms: 160,
                    average_assembly_wait_ms: 80,
                    max_assembly_wait_ms: 80,
                    total_input_chars: 100,
                    total_output_chars: 80,
                })
            }));
        let text = serde_json::to_string(&event).expect("serialize event");
        assert!(text.contains("run.completed"));
        for forbidden in [
            "sourceText",
            "translatedText",
            "prompt",
            "rawResponse",
            "messages",
        ] {
            assert!(
                !text.contains(forbidden),
                "timeline JSON must not contain `{forbidden}`"
            );
        }
    }
}
