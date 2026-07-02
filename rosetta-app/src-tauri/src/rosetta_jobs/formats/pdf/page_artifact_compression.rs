use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::Instant,
};

use serde_json::json;
use tauri::AppHandle;

use crate::{
    managed_pdf2zh::{layout::Pdf2zhLayout, profile},
    rosetta_jobs::path,
};

use super::{
    diagnostics,
    page_assemble::count_pdf_pages_lopdf,
    page_state::{self, PdfPageTranslation, PdfPageTranslationState},
};

const ARTIFACT_COMPRESSION_ENV: &str = "ROSETTA_PDF_PAGE_ARTIFACT_COMPRESSION";
const MIN_SAVINGS_BYTES: u64 = 64 * 1024;

const PYMUPDF_COMPRESS_SCRIPT: &str = r#"
import os
import sys
import pymupdf

src, dst = sys.argv[1], sys.argv[2]
doc = pymupdf.open(src)
try:
    if doc.page_count != 1:
        raise RuntimeError(f"expected one page, got {doc.page_count}")
    doc.save(
        dst,
        garbage=4,
        deflate=True,
        deflate_images=True,
        deflate_fonts=True,
        use_objstms=1,
    )
finally:
    doc.close()

if not os.path.isfile(dst) or os.path.getsize(dst) <= 0:
    raise RuntimeError("compressed output was not written")
"#;

#[derive(Debug, Clone)]
struct CompressionCandidate {
    page_number: u32,
    relative_path: String,
    artifact_version: Option<String>,
    last_run_id: Option<String>,
}

#[derive(Debug, Clone)]
struct CompressionPaths {
    source: PathBuf,
    temp: PathBuf,
    backup: PathBuf,
}

pub(crate) fn schedule_background_compression(
    app: &AppHandle,
    job_id: &str,
    target_lang: &str,
    source_page_count: u32,
    run_id: Option<&str>,
) {
    if !artifact_compression_enabled() {
        return;
    }

    let Some(profile) = profile::current_profile() else {
        return;
    };
    if profile.platform_os != "windows" {
        return;
    }
    let Ok(layout) = Pdf2zhLayout::from_app(app, profile) else {
        return;
    };
    let python = layout.bin_path(profile);
    if !python.is_file() {
        return;
    }

    let Ok(root) = path::jobs_root(app) else {
        return;
    };
    let Ok(job_dir) = path::checked_job_dir(&root, job_id) else {
        return;
    };
    let Ok(state) =
        page_state::read_pdf_page_translation_state(&job_dir, source_page_count, target_lang)
    else {
        return;
    };
    if compression_candidates(&state, run_id).is_empty() {
        return;
    }

    let key = compression_key(job_id, target_lang, run_id);
    if !try_register_compression_task(&key) {
        return;
    }

    let job_id = job_id.to_string();
    let target_lang = target_lang.to_string();
    let run_id = run_id.map(str::to_string);
    tauri::async_runtime::spawn(async move {
        let key_for_cleanup = key.clone();
        let result = tokio::task::spawn_blocking(move || {
            compress_page_artifacts(
                &job_dir,
                &python,
                &job_id,
                &target_lang,
                source_page_count,
                run_id.as_deref(),
            )
        })
        .await;
        if let Err(error) = result {
            eprintln!("[pdf-page-compress] worker join failed: {error}");
        }
        unregister_compression_task(&key_for_cleanup);
    });
}

pub(crate) fn cleanup_stale_compression_files(
    job_dir: &Path,
    target_lang: &str,
) -> Result<bool, String> {
    let translated_dir = job_dir
        .join("translated-pages")
        .join(page_state::pdf_page_language_dir(target_lang));
    cleanup_stale_compression_files_in_dir(&translated_dir)
}

#[cfg(test)]
pub(crate) fn cleanup_stale_compression_files_in_dir(dir: &Path) -> Result<bool, String> {
    cleanup_stale_compression_files_in_dir_impl(dir)
}

#[cfg(not(test))]
fn cleanup_stale_compression_files_in_dir(dir: &Path) -> Result<bool, String> {
    cleanup_stale_compression_files_in_dir_impl(dir)
}

fn cleanup_stale_compression_files_in_dir_impl(dir: &Path) -> Result<bool, String> {
    if !dir.is_dir() {
        return Ok(false);
    }

    let mut cleaned = false;
    let mut backups = HashMap::<u32, Vec<PathBuf>>::new();
    for entry in
        fs::read_dir(dir).map_err(|error| format!("无法读取 PDF 页压缩临时目录: {error}"))?
    {
        let entry = entry.map_err(|error| format!("无法读取 PDF 页压缩临时项: {error}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.contains(".compressing.tmp.pdf") {
            fs::remove_file(&path)
                .map_err(|error| format!("无法清理 PDF 页压缩临时文件: {error}"))?;
            cleaned = true;
            continue;
        }
        if name.contains(".precompress.bak") {
            if let Some(page_number) = page_number_from_compression_backup_name(name) {
                backups.entry(page_number).or_default().push(path);
            }
        }
    }

    for (page_number, paths) in backups {
        let canonical = dir.join(page_state::pdf_page_filename(page_number));
        let mut restored = false;
        for path in paths {
            if !canonical.exists() && !restored {
                match fs::rename(&path, &canonical) {
                    Ok(_) => {
                        restored = true;
                        cleaned = true;
                    }
                    Err(_) => {
                        let _ = fs::remove_file(&path);
                        cleaned = true;
                    }
                }
            } else {
                fs::remove_file(&path)
                    .map_err(|error| format!("无法清理 PDF 页压缩备份文件: {error}"))?;
                cleaned = true;
            }
        }
    }

    Ok(cleaned)
}

fn compress_page_artifacts(
    job_dir: &Path,
    python: &Path,
    job_id: &str,
    target_lang: &str,
    source_page_count: u32,
    run_id: Option<&str>,
) {
    if !job_dir.is_dir() {
        return;
    }
    let Ok(state) =
        page_state::read_pdf_page_translation_state(job_dir, source_page_count, target_lang)
    else {
        return;
    };
    let candidates = compression_candidates(&state, run_id);
    if candidates.is_empty() {
        return;
    }

    diagnostics::append_timeline_event(
        job_dir,
        diagnostics::PdfTimelineEvent::new(job_id, "artifact-compression", "started")
            .target_lang(target_lang)
            .details(json!({
                "candidatePages": candidates.iter().map(|candidate| candidate.page_number).collect::<Vec<_>>(),
                "candidateCount": candidates.len(),
                "runIdFilter": run_id,
            })),
    );

    let started = Instant::now();
    let mut compressed = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;
    for candidate in candidates {
        if !job_dir.is_dir() {
            break;
        }
        match compress_one_page_artifact(
            job_dir,
            python,
            job_id,
            target_lang,
            source_page_count,
            run_id,
            &candidate,
        ) {
            PageCompressionOutcome::Compressed => compressed += 1,
            PageCompressionOutcome::Skipped => skipped += 1,
            PageCompressionOutcome::Failed => failed += 1,
        }
    }

    diagnostics::append_timeline_event(
        job_dir,
        diagnostics::PdfTimelineEvent::new(job_id, "artifact-compression", "completed")
            .target_lang(target_lang)
            .duration_ms(started.elapsed().as_millis() as u64)
            .details(json!({
                "compressed": compressed,
                "skipped": skipped,
                "failed": failed,
            })),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageCompressionOutcome {
    Compressed,
    Skipped,
    Failed,
}

fn compress_one_page_artifact(
    job_dir: &Path,
    python: &Path,
    job_id: &str,
    target_lang: &str,
    source_page_count: u32,
    run_id: Option<&str>,
    candidate: &CompressionCandidate,
) -> PageCompressionOutcome {
    let page_started = Instant::now();
    let Some(paths) =
        verified_candidate_paths(job_dir, source_page_count, target_lang, run_id, candidate)
    else {
        return PageCompressionOutcome::Skipped;
    };
    let original_bytes = fs::metadata(&paths.source)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    diagnostics::append_timeline_event(
        job_dir,
        diagnostics::PdfTimelineEvent::new(job_id, "artifact-compression", "page.started")
            .target_lang(target_lang)
            .page_number(candidate.page_number)
            .details(json!({
                "originalBytes": original_bytes,
            })),
    );

    let _ = fs::remove_file(&paths.temp);
    let output = Command::new(python)
        .arg("-c")
        .arg(PYMUPDF_COMPRESS_SCRIPT)
        .arg(&paths.source)
        .arg(&paths.temp)
        .output();
    match output {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let message = if stderr.is_empty() { stdout } else { stderr };
            mark_compression_error(
                job_dir,
                source_page_count,
                target_lang,
                run_id,
                candidate,
                format!("PyMuPDF 压缩失败: {message}"),
            );
            let _ = fs::remove_file(&paths.temp);
            diagnostics::append_timeline_event(
                job_dir,
                diagnostics::PdfTimelineEvent::new(job_id, "artifact-compression", "page.failed")
                    .target_lang(target_lang)
                    .page_number(candidate.page_number)
                    .duration_ms(page_started.elapsed().as_millis() as u64)
                    .details(json!({
                        "error": message,
                        "originalBytes": original_bytes,
                    })),
            );
            return PageCompressionOutcome::Failed;
        }
        Err(error) => {
            mark_compression_error(
                job_dir,
                source_page_count,
                target_lang,
                run_id,
                candidate,
                format!("无法启动 PyMuPDF 压缩器: {error}"),
            );
            let _ = fs::remove_file(&paths.temp);
            diagnostics::append_timeline_event(
                job_dir,
                diagnostics::PdfTimelineEvent::new(job_id, "artifact-compression", "page.failed")
                    .target_lang(target_lang)
                    .page_number(candidate.page_number)
                    .duration_ms(page_started.elapsed().as_millis() as u64)
                    .details(json!({
                        "error": error.to_string(),
                        "originalBytes": original_bytes,
                    })),
            );
            return PageCompressionOutcome::Failed;
        }
    }

    if count_pdf_pages_lopdf(&paths.temp).ok() != Some(1) {
        mark_compression_error(
            job_dir,
            source_page_count,
            target_lang,
            run_id,
            candidate,
            "压缩后的 PDF 页无法通过单页校验。".to_string(),
        );
        let _ = fs::remove_file(&paths.temp);
        return PageCompressionOutcome::Failed;
    }

    let compressed_bytes = fs::metadata(&paths.temp)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if compressed_bytes + MIN_SAVINGS_BYTES >= original_bytes {
        let _ = fs::remove_file(&paths.temp);
        mark_compression_skipped(
            job_dir,
            source_page_count,
            target_lang,
            run_id,
            candidate,
            original_bytes,
        );
        diagnostics::append_timeline_event(
            job_dir,
            diagnostics::PdfTimelineEvent::new(job_id, "artifact-compression", "page.skipped")
                .target_lang(target_lang)
                .page_number(candidate.page_number)
                .duration_ms(page_started.elapsed().as_millis() as u64)
                .details(json!({
                    "reason": "not-smaller-enough",
                    "originalBytes": original_bytes,
                    "compressedBytes": compressed_bytes,
                })),
        );
        return PageCompressionOutcome::Skipped;
    }

    if verified_candidate_paths(job_dir, source_page_count, target_lang, run_id, candidate)
        .is_none()
    {
        let _ = fs::remove_file(&paths.temp);
        return PageCompressionOutcome::Skipped;
    }

    match replace_with_backup(&paths.source, &paths.temp, &paths.backup, || {
        candidate_state_still_current(job_dir, source_page_count, target_lang, run_id, candidate)
    }) {
        ReplaceOutcome::Replaced => {}
        ReplaceOutcome::Changed => {
            let _ = fs::remove_file(&paths.temp);
            return PageCompressionOutcome::Skipped;
        }
        ReplaceOutcome::Failed(error) => {
            mark_compression_error(
                job_dir,
                source_page_count,
                target_lang,
                run_id,
                candidate,
                error.clone(),
            );
            diagnostics::append_timeline_event(
                job_dir,
                diagnostics::PdfTimelineEvent::new(job_id, "artifact-compression", "page.failed")
                    .target_lang(target_lang)
                    .page_number(candidate.page_number)
                    .duration_ms(page_started.elapsed().as_millis() as u64)
                    .details(json!({
                        "error": error,
                        "originalBytes": original_bytes,
                        "compressedBytes": compressed_bytes,
                    })),
            );
            return PageCompressionOutcome::Failed;
        }
    }

    if count_pdf_pages_lopdf(&paths.source).ok() != Some(1) {
        let _ = restore_backup_over_target(&paths.source, &paths.backup);
        mark_compression_error(
            job_dir,
            source_page_count,
            target_lang,
            run_id,
            candidate,
            "替换后的 PDF 页无法通过单页校验，已尝试恢复原文件。".to_string(),
        );
        return PageCompressionOutcome::Failed;
    }
    let _ = fs::remove_file(&paths.backup);
    let final_bytes = fs::metadata(&paths.source)
        .map(|metadata| metadata.len())
        .unwrap_or(compressed_bytes);
    mark_compression_completed(
        job_dir,
        source_page_count,
        target_lang,
        run_id,
        candidate,
        final_bytes,
    );
    diagnostics::append_timeline_event(
        job_dir,
        diagnostics::PdfTimelineEvent::new(job_id, "artifact-compression", "page.completed")
            .target_lang(target_lang)
            .page_number(candidate.page_number)
            .duration_ms(page_started.elapsed().as_millis() as u64)
            .details(json!({
                "originalBytes": original_bytes,
                "compressedBytes": final_bytes,
                "savedBytes": original_bytes.saturating_sub(final_bytes),
            })),
    );
    PageCompressionOutcome::Compressed
}

fn verified_candidate_paths(
    job_dir: &Path,
    source_page_count: u32,
    target_lang: &str,
    run_id: Option<&str>,
    candidate: &CompressionCandidate,
) -> Option<CompressionPaths> {
    let state =
        page_state::read_pdf_page_translation_state(job_dir, source_page_count, target_lang)
            .ok()?;
    let page = matching_page(&state, run_id, candidate)?;
    let relative = page.translated_pdf_path.as_deref()?;
    let source = safe_relative_cache_path(job_dir, relative)?;
    if !source.is_file() {
        return None;
    }
    let parent = source.parent()?.to_path_buf();
    let filename = source.file_name()?.to_str()?.to_string();
    let stamp = path::timestamp_ms_string();
    Some(CompressionPaths {
        source,
        temp: parent.join(format!(".{filename}.{stamp}.compressing.tmp.pdf")),
        backup: parent.join(format!(".{filename}.{stamp}.precompress.bak")),
    })
}

fn candidate_state_still_current(
    job_dir: &Path,
    source_page_count: u32,
    target_lang: &str,
    run_id: Option<&str>,
    candidate: &CompressionCandidate,
) -> bool {
    page_state::read_pdf_page_translation_state(job_dir, source_page_count, target_lang)
        .ok()
        .and_then(|state| matching_page(&state, run_id, candidate).map(|_| ()))
        .is_some()
}

fn matching_page<'a>(
    state: &'a PdfPageTranslationState,
    run_id: Option<&str>,
    candidate: &CompressionCandidate,
) -> Option<&'a PdfPageTranslation> {
    let page = state
        .pages
        .iter()
        .find(|page| page.page_number == candidate.page_number)?;
    if page.status != "translated" {
        return None;
    }
    if page.translated_pdf_path.as_deref() != Some(candidate.relative_path.as_str()) {
        return None;
    }
    if page.artifact_version != candidate.artifact_version {
        return None;
    }
    if page.last_run_id != candidate.last_run_id {
        return None;
    }
    if run_id.is_some() && page.last_run_id.as_deref() != run_id {
        return None;
    }
    if matches!(
        page.artifact_compression.as_deref(),
        Some("compressed") | Some("skipped")
    ) {
        return None;
    }
    Some(page)
}

enum ReplaceOutcome {
    Replaced,
    Changed,
    Failed(String),
}

fn replace_with_backup(
    source: &Path,
    temp: &Path,
    backup: &Path,
    still_current: impl FnOnce() -> bool,
) -> ReplaceOutcome {
    let _ = fs::remove_file(backup);
    if let Err(error) = fs::rename(source, backup) {
        return ReplaceOutcome::Failed(format!("无法备份原 PDF 页产物: {error}"));
    }
    if !still_current() {
        let _ = restore_backup_if_target_missing(source, backup);
        return ReplaceOutcome::Changed;
    }
    match fs::rename(temp, source) {
        Ok(_) => ReplaceOutcome::Replaced,
        Err(error) => {
            let _ = restore_backup_if_target_missing(source, backup);
            ReplaceOutcome::Failed(format!("无法提交压缩后的 PDF 页产物: {error}"))
        }
    }
}

fn restore_backup_if_target_missing(source: &Path, backup: &Path) -> Result<(), String> {
    if source.exists() {
        let _ = fs::remove_file(backup);
        return Ok(());
    }
    if backup.is_file() {
        fs::rename(backup, source).map_err(|error| format!("无法恢复原 PDF 页产物: {error}"))?;
    }
    Ok(())
}

fn restore_backup_over_target(source: &Path, backup: &Path) -> Result<(), String> {
    if source.exists() {
        let _ = fs::remove_file(source);
    }
    if backup.is_file() {
        fs::rename(backup, source).map_err(|error| format!("无法恢复原 PDF 页产物: {error}"))?;
    }
    Ok(())
}

fn mark_compression_completed(
    job_dir: &Path,
    source_page_count: u32,
    target_lang: &str,
    run_id: Option<&str>,
    candidate: &CompressionCandidate,
    bytes: u64,
) {
    update_matching_page_state(
        job_dir,
        source_page_count,
        target_lang,
        run_id,
        candidate,
        |page| {
            let now = path::timestamp_ms_string();
            page.artifact_version = Some(now.clone());
            page.artifact_compression = Some("compressed".to_string());
            page.artifact_bytes = Some(bytes);
            page.artifact_compression_error = None;
            page.updated_at = now;
        },
    );
}

fn mark_compression_skipped(
    job_dir: &Path,
    source_page_count: u32,
    target_lang: &str,
    run_id: Option<&str>,
    candidate: &CompressionCandidate,
    bytes: u64,
) {
    update_matching_page_state(
        job_dir,
        source_page_count,
        target_lang,
        run_id,
        candidate,
        |page| {
            page.artifact_compression = Some("skipped".to_string());
            page.artifact_bytes = Some(bytes);
            page.artifact_compression_error = None;
            page.updated_at = path::timestamp_ms_string();
        },
    );
}

fn mark_compression_error(
    job_dir: &Path,
    source_page_count: u32,
    target_lang: &str,
    run_id: Option<&str>,
    candidate: &CompressionCandidate,
    error: String,
) {
    update_matching_page_state(
        job_dir,
        source_page_count,
        target_lang,
        run_id,
        candidate,
        |page| {
            page.artifact_compression = Some("fast".to_string());
            page.artifact_compression_error = Some(error);
            page.updated_at = path::timestamp_ms_string();
        },
    );
}

fn update_matching_page_state(
    job_dir: &Path,
    source_page_count: u32,
    target_lang: &str,
    run_id: Option<&str>,
    candidate: &CompressionCandidate,
    update: impl FnOnce(&mut PdfPageTranslation),
) {
    let Ok(mut state) =
        page_state::read_pdf_page_translation_state(job_dir, source_page_count, target_lang)
    else {
        return;
    };
    let Some(index) = state
        .pages
        .iter()
        .position(|page| page.page_number == candidate.page_number)
    else {
        return;
    };
    if matching_page(&state, run_id, candidate).is_none() {
        return;
    }
    update(&mut state.pages[index]);
    let _ = page_state::write_pdf_page_translation_state(job_dir, &state);
}

fn compression_candidates(
    state: &PdfPageTranslationState,
    run_id: Option<&str>,
) -> Vec<CompressionCandidate> {
    state
        .pages
        .iter()
        .filter(|page| {
            page.status == "translated"
                && page.translated_pdf_path.is_some()
                && !matches!(
                    page.artifact_compression.as_deref(),
                    Some("compressed") | Some("skipped")
                )
                && run_id.is_none_or(|run_id| page.last_run_id.as_deref() == Some(run_id))
        })
        .filter_map(|page| {
            Some(CompressionCandidate {
                page_number: page.page_number,
                relative_path: page.translated_pdf_path.clone()?,
                artifact_version: page.artifact_version.clone(),
                last_run_id: page.last_run_id.clone(),
            })
        })
        .collect()
}

fn safe_relative_cache_path(job_dir: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    let is_safe = path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if is_safe {
        Some(job_dir.join(path))
    } else {
        None
    }
}

fn page_number_from_compression_backup_name(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(".page-")?;
    let (number, rest) = rest.split_once(".pdf.")?;
    if !rest.ends_with(".precompress.bak") {
        return None;
    }
    number.parse::<u32>().ok()
}

fn artifact_compression_enabled() -> bool {
    let Ok(value) = std::env::var(ARTIFACT_COMPRESSION_ENV) else {
        return true;
    };
    let normalized = value.trim().to_ascii_lowercase();
    !matches!(
        normalized.as_str(),
        "" | "0" | "false" | "off" | "none" | "fast"
    )
}

fn compression_key(job_id: &str, target_lang: &str, _run_id: Option<&str>) -> String {
    format!(
        "{job_id}:{}",
        page_state::pdf_page_language_dir(target_lang)
    )
}

fn active_compression_tasks() -> &'static Mutex<BTreeSet<String>> {
    static ACTIVE: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn try_register_compression_task(key: &str) -> bool {
    let Ok(mut active) = active_compression_tasks().lock() else {
        return false;
    };
    active.insert(key.to_string())
}

fn unregister_compression_task(key: &str) {
    if let Ok(mut active) = active_compression_tasks().lock() {
        active.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::rosetta_jobs::{
        formats::pdf::page_state::{
            write_pdf_page_translation_state, PdfPageTranslation, PdfPageTranslationState,
        },
        model::SCHEMA_VERSION,
    };

    #[test]
    fn state_guard_survives_canonical_file_being_temporarily_backed_up() {
        let dir = unique_temp_dir("pdf-compress-state-guard");
        let pages_dir = dir.join("translated-pages").join("zh-CN");
        fs::create_dir_all(&pages_dir).expect("create pages dir");
        let page = pages_dir.join("page-0001.pdf");
        let backup = pages_dir.join(".page-0001.pdf.1.precompress.bak");
        fs::write(&backup, b"backup").expect("write backup");
        assert!(!page.exists());
        let state = PdfPageTranslationState {
            schema_version: SCHEMA_VERSION,
            source_page_count: 1,
            target_lang: "zh-CN".to_string(),
            pages: vec![PdfPageTranslation {
                page_number: 1,
                status: "translated".to_string(),
                translated_pdf_path: Some("translated-pages/zh-CN/page-0001.pdf".to_string()),
                artifact_version: Some("v1".to_string()),
                artifact_compression: Some("fast".to_string()),
                artifact_bytes: Some(100),
                artifact_compression_error: None,
                error: None,
                updated_at: "1".to_string(),
                last_run_id: Some("run-1".to_string()),
            }],
        };
        write_pdf_page_translation_state(&dir, &state).expect("write page state");
        let candidate = super::CompressionCandidate {
            page_number: 1,
            relative_path: "translated-pages/zh-CN/page-0001.pdf".to_string(),
            artifact_version: Some("v1".to_string()),
            last_run_id: Some("run-1".to_string()),
        };

        assert!(super::candidate_state_still_current(
            &dir,
            1,
            "zh-CN",
            Some("run-1"),
            &candidate,
        ));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn compression_task_key_serializes_same_job_language_across_run_filters() {
        assert_eq!(
            super::compression_key("job-1", "zh-CN", Some("run-new")),
            super::compression_key("job-1", "zh-CN", None)
        );
        assert_ne!(
            super::compression_key("job-1", "zh-CN", Some("run-new")),
            super::compression_key("job-1", "ja", Some("run-new"))
        );
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rosetta-{prefix}-{}-{}",
            std::process::id(),
            crate::rosetta_jobs::path::timestamp_ms_string()
        ))
    }
}
