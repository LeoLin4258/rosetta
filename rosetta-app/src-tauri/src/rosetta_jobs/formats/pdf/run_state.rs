use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::rosetta_jobs::{
    model::SCHEMA_VERSION,
    path::timestamp_ms_string,
    store::{read_json, write_json},
};

pub(crate) const PDF_RUN_CHUNK_SIZE: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfTranslationRun {
    pub schema_version: u32,
    pub run_id: String,
    pub job_id: String,
    pub target_lang: String,
    pub state: String,
    pub mode: String,
    #[serde(default)]
    pub requested_pages: Vec<u32>,
    #[serde(default)]
    pub completed_pages: Vec<u32>,
    #[serde(default)]
    pub failed_pages: Vec<u32>,
    #[serde(default)]
    pub current_chunk: Vec<u32>,
    pub owner_session_id: String,
    pub lease_updated_at: String,
    #[serde(default)]
    pub cancel_requested: bool,
    pub started_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl PdfTranslationRun {
    pub(crate) fn new(
        run_id: String,
        job_id: String,
        target_lang: String,
        mode: String,
        requested_pages: Vec<u32>,
        owner_session_id: String,
    ) -> Self {
        let now = timestamp_ms_string();
        Self {
            schema_version: SCHEMA_VERSION,
            run_id,
            job_id,
            target_lang,
            state: "running".to_string(),
            mode,
            requested_pages,
            completed_pages: Vec::new(),
            failed_pages: Vec::new(),
            current_chunk: Vec::new(),
            owner_session_id,
            lease_updated_at: now.clone(),
            cancel_requested: false,
            started_at: now.clone(),
            updated_at: now,
            last_error: None,
        }
    }

    pub(crate) fn touch_lease(&mut self) {
        let now = timestamp_ms_string();
        self.lease_updated_at = now.clone();
        self.updated_at = now;
    }

    pub(crate) fn set_state(&mut self, state: &str) {
        self.state = state.to_string();
        self.updated_at = timestamp_ms_string();
    }

    pub(crate) fn is_live_state(&self) -> bool {
        self.state == "running" || self.state == "pausing"
    }
}

pub(crate) fn pdf_run_state_filename(target_lang: &str) -> String {
    format!(
        "pdf_run.{}.json",
        super::page_state::pdf_page_language_dir(target_lang)
    )
}

pub(crate) fn read_pdf_run_state(
    job_dir: &Path,
    target_lang: &str,
) -> Result<Option<PdfTranslationRun>, String> {
    let path = job_dir.join(pdf_run_state_filename(target_lang));
    if !path.is_file() {
        return Ok(None);
    }
    read_json(&path).map(Some)
}

pub(crate) fn write_pdf_run_state(job_dir: &Path, run: &PdfTranslationRun) -> Result<(), String> {
    write_json(&job_dir.join(pdf_run_state_filename(&run.target_lang)), run)
}

pub(crate) fn recover_stale_run(
    job_dir: &Path,
    target_lang: &str,
    current_session_id: &str,
) -> Result<Option<PdfTranslationRun>, String> {
    let Some(mut run) = read_pdf_run_state(job_dir, target_lang)? else {
        return Ok(None);
    };
    if run.is_live_state() && run.owner_session_id != current_session_id {
        run.state = "paused".to_string();
        run.cancel_requested = false;
        run.current_chunk.clear();
        run.last_error = Some("上次翻译在应用退出时中断，已恢复为可继续状态。".to_string());
        run.updated_at = timestamp_ms_string();
        write_pdf_run_state(job_dir, &run)?;
    }
    Ok(Some(run))
}

pub(crate) fn append_unique_page(pages: &mut Vec<u32>, page_number: u32) {
    if !pages.contains(&page_number) {
        pages.push(page_number);
        pages.sort_unstable();
    }
}
