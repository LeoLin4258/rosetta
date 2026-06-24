use std::{collections::BTreeSet, path::Path};

use serde::{Deserialize, Serialize};

use crate::rosetta_jobs::{
    model::SCHEMA_VERSION,
    path::timestamp_ms_string,
    store::{read_json, write_json},
};

pub(crate) const PDF_PAGE_TRANSLATIONS_FILENAME: &str = "pdf_page_translations.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfPageTranslationState {
    pub schema_version: u32,
    pub source_page_count: u32,
    pub target_lang: String,
    pub pages: Vec<PdfPageTranslation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfPageTranslation {
    pub page_number: u32,
    pub status: String,
    pub translated_pdf_path: Option<String>,
    pub error: Option<String>,
    pub updated_at: String,
}

pub(crate) fn parse_pdf_page_selection(
    input: &str,
    source_page_count: u32,
) -> Result<Vec<u32>, String> {
    if source_page_count == 0 {
        return Err("PDF 没有可选择的页面。".to_string());
    }

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("请输入要翻译的页码。".to_string());
    }

    let mut pages = BTreeSet::new();
    for raw_part in trimmed.split(',') {
        let part = raw_part.trim();
        if part.is_empty() {
            return Err("页码范围里有空项。".to_string());
        }

        if let Some((start, end)) = part.split_once('-') {
            let start = parse_page_number(start.trim(), source_page_count)?;
            let end = parse_page_number(end.trim(), source_page_count)?;
            if start > end {
                return Err(format!("页码范围 {start}-{end} 的起始页不能大于结束页。"));
            }
            for page in start..=end {
                pages.insert(page);
            }
        } else {
            pages.insert(parse_page_number(part, source_page_count)?);
        }
    }

    Ok(pages.into_iter().collect())
}

pub(crate) fn read_pdf_page_translation_state(
    job_dir: &Path,
    source_page_count: u32,
    target_lang: &str,
) -> Result<PdfPageTranslationState, String> {
    let preferred_path = job_dir.join(pdf_page_translation_state_filename(target_lang));
    let legacy_path = job_dir.join(PDF_PAGE_TRANSLATIONS_FILENAME);
    let path = if preferred_path.is_file() {
        preferred_path
    } else {
        legacy_path
    };
    if !path.is_file() {
        return Ok(empty_state(source_page_count, target_lang));
    }

    let mut state: PdfPageTranslationState = read_json(&path)?;
    if state.target_lang != target_lang {
        return Ok(empty_state(source_page_count, target_lang));
    }

    state.source_page_count = source_page_count;
    for page in &mut state.pages {
        if page.status == "translating" || page.status == "queued" {
            page.status = "pending".to_string();
            page.updated_at = timestamp_ms_string();
        }
        if is_unscoped_legacy_page_path(page.translated_pdf_path.as_deref())
            && !is_trusted_legacy_target_lang(target_lang)
        {
            page.status = "pending".to_string();
            page.translated_pdf_path = None;
            page.error = None;
            page.updated_at = timestamp_ms_string();
        }
    }
    Ok(state)
}

pub(crate) fn write_pdf_page_translation_state(
    job_dir: &Path,
    state: &PdfPageTranslationState,
) -> Result<(), String> {
    write_json(
        &job_dir.join(pdf_page_translation_state_filename(&state.target_lang)),
        state,
    )
}

pub(crate) fn upsert_pdf_page(
    state: &mut PdfPageTranslationState,
    page_number: u32,
    status: &str,
    translated_pdf_path: Option<String>,
    error: Option<String>,
) {
    let updated_at = timestamp_ms_string();
    if let Some(page) = state
        .pages
        .iter_mut()
        .find(|page| page.page_number == page_number)
    {
        page.status = status.to_string();
        page.translated_pdf_path = translated_pdf_path;
        page.error = error;
        page.updated_at = updated_at;
        return;
    }

    state.pages.push(PdfPageTranslation {
        page_number,
        status: status.to_string(),
        translated_pdf_path,
        error,
        updated_at,
    });
    state.pages.sort_by_key(|page| page.page_number);
}

pub(crate) fn pdf_page_filename(page_number: u32) -> String {
    format!("page-{page_number:04}.pdf")
}

pub(crate) fn pdf_page_language_dir(target_lang: &str) -> String {
    let mut slug = String::new();
    for ch in target_lang.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch);
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "unknown".to_string()
    } else {
        slug.to_string()
    }
}

pub(crate) fn pdf_page_translation_state_filename(target_lang: &str) -> String {
    format!(
        "pdf_page_translations.{}.json",
        pdf_page_language_dir(target_lang)
    )
}

pub(crate) fn pdf_page_relative_path_for_lang(target_lang: &str, page_number: u32) -> String {
    format!(
        "pdf-pages/{}/{}",
        pdf_page_language_dir(target_lang),
        pdf_page_filename(page_number)
    )
}

pub(crate) fn pdf_page_relative_path(page_number: u32) -> String {
    format!("pdf-pages/{}", pdf_page_filename(page_number))
}

pub(crate) fn pdf_page_status_summary(
    state: &PdfPageTranslationState,
) -> (usize, usize, usize, String) {
    let segment_count = state.source_page_count as usize;
    let completed_segments = state
        .pages
        .iter()
        .filter(|page| page.status == "translated")
        .count();
    let failed_segments = state
        .pages
        .iter()
        .filter(|page| page.status == "failed")
        .count();
    let has_running = state
        .pages
        .iter()
        .any(|page| page.status == "queued" || page.status == "translating");
    let status = if segment_count > 0 && completed_segments >= segment_count {
        "translated"
    } else if has_running {
        "translating"
    } else if failed_segments > 0 {
        "failed"
    } else {
        "untranslated"
    };

    (
        segment_count,
        completed_segments,
        failed_segments,
        status.to_string(),
    )
}

pub(crate) fn empty_state(source_page_count: u32, target_lang: &str) -> PdfPageTranslationState {
    PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count,
        target_lang: target_lang.to_string(),
        pages: Vec::new(),
    }
}

fn parse_page_number(input: &str, source_page_count: u32) -> Result<u32, String> {
    let page = input
        .parse::<u32>()
        .map_err(|_| format!("页码 `{input}` 不是有效数字。"))?;
    if page == 0 {
        return Err("页码必须从 1 开始。".to_string());
    }
    if page > source_page_count {
        return Err(format!(
            "第 {page} 页超出范围，当前 PDF 共 {source_page_count} 页。"
        ));
    }
    Ok(page)
}

fn is_unscoped_legacy_page_path(path: Option<&str>) -> bool {
    path.is_some_and(|path| {
        path.strip_prefix("pdf-pages/page-")
            .is_some_and(|rest| rest.ends_with(".pdf"))
    })
}

fn is_trusted_legacy_target_lang(target_lang: &str) -> bool {
    let normalized = target_lang.trim().to_ascii_lowercase();
    normalized == "zh" || normalized.starts_with("zh-")
}
