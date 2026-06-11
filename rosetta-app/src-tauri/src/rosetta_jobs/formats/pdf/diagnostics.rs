//! Per-run PDF translation diagnostics.
//!
//! Each `translate_rosetta_pdf_pages` run writes one JSON profile under
//! `<job_dir>/diagnostics/`. The profile separates RWKV model time from
//! pdf2zh process time (startup, parse/layout, render) so performance work
//! can target the real bottleneck. Counts, durations and page numbers only —
//! never source or translated text.

use std::path::Path;

use serde::Serialize;

use crate::managed_pdf2zh::openai_shim::ShimRwkvMetricsSnapshot;
use crate::rosetta_jobs::model::SCHEMA_VERSION;

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
    /// Aggregated RWKV request stats across all invocations. `None` when the
    /// run was cancelled/failed before any invocation finished.
    pub rwkv: Option<RwkvAggregate>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfTranslationDurations {
    /// Wall time of the whole command.
    pub total: u64,
    /// Sum of per-invocation warmup (status resolution, shim spawn, role
    /// setup, process spawn).
    pub pdf2zh_warmup: u64,
    /// Sum of per-invocation pdf2zh process wall time (parse + layout +
    /// translate + render). RWKV time happens inside this window; subtract
    /// `rwkv.totalRequestMs` for a lower bound on pure PDF processing.
    pub pdf2zh_process: u64,
    /// Splitting batch output into per-page PDFs under `pdf-pages/`.
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
    pub total_input_chars: u64,
    pub total_output_chars: u64,
}

impl RwkvAggregate {
    pub(crate) fn add(&mut self, snapshot: &ShimRwkvMetricsSnapshot) {
        self.request_count += snapshot.request_count;
        self.failed_request_count += snapshot.failed_request_count;
        self.total_request_ms += snapshot.total_request_ms;
        self.max_request_ms = self.max_request_ms.max(snapshot.max_request_ms);
        self.total_input_chars += snapshot.total_input_chars;
        self.total_output_chars += snapshot.total_output_chars;
        self.average_request_ms = if self.request_count > 0 {
            self.total_request_ms / self.request_count
        } else {
            0
        };
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rwkv_aggregate_sums_and_averages() {
        let mut agg = RwkvAggregate::default();
        agg.add(&ShimRwkvMetricsSnapshot {
            request_count: 4,
            failed_request_count: 1,
            total_request_ms: 4000,
            average_request_ms: 1000,
            max_request_ms: 2100,
            total_input_chars: 800,
            total_output_chars: 700,
        });
        agg.add(&ShimRwkvMetricsSnapshot {
            request_count: 6,
            failed_request_count: 0,
            total_request_ms: 3000,
            average_request_ms: 500,
            max_request_ms: 900,
            total_input_chars: 1200,
            total_output_chars: 1100,
        });
        assert_eq!(agg.request_count, 10);
        assert_eq!(agg.failed_request_count, 1);
        assert_eq!(agg.total_request_ms, 7000);
        assert_eq!(agg.average_request_ms, 700);
        assert_eq!(agg.max_request_ms, 2100);
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
}
