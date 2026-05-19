use std::{fs, path::Path};

use tauri::AppHandle;

use crate::rosetta_jobs::formats::pdf::{
    count_pages,
    errors::{PdfError, MAX_PDF_BYTES, MAX_PDF_PAGES},
};

/// Cheap PDF validation used at import time. Phase 3 treats PDF translation as
/// a pdf2zh black box, so import no longer extracts Rosetta blocks/segments.
pub(crate) fn pre_flight(app: &AppHandle, source_path: &Path) -> Result<(), PdfError> {
    let metadata = fs::metadata(source_path)
        .map_err(|error| PdfError::Read(format!("无法读取文件信息: {error}")))?;
    if !metadata.is_file() {
        return Err(PdfError::Read("路径不是文件。".to_string()));
    }
    if metadata.len() > MAX_PDF_BYTES {
        return Err(PdfError::TooLarge {
            reason: format!(
                "文件大小 {:.1} MB，当前上限是 100 MB。",
                metadata.len() as f64 / 1024.0 / 1024.0
            ),
        });
    }

    let pages = count_pages(app, source_path)?;
    if pages == 0 {
        return Err(PdfError::ImageOnly);
    }
    if pages > MAX_PDF_PAGES {
        return Err(PdfError::TooLarge {
            reason: format!("页数 {pages}，当前上限是 {MAX_PDF_PAGES} 页。"),
        });
    }
    Ok(())
}
