use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::rosetta_jobs::{
    model::SCHEMA_VERSION,
    path::timestamp_ms_string,
    store::{read_json, write_json},
};

pub(crate) const PDF_SOURCE_FILENAME: &str = "pdf_source.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfSourceMetadata {
    pub schema_version: u32,
    pub page_count: u32,
    pub source_fingerprint: String,
    pub filename: String,
    #[serde(default)]
    pub original_path: Option<String>,
    pub imported_at: String,
    pub updated_at: String,
}

pub(crate) fn read_pdf_source_metadata(
    job_dir: &Path,
) -> Result<Option<PdfSourceMetadata>, String> {
    let path = job_dir.join(PDF_SOURCE_FILENAME);
    if !path.is_file() {
        return Ok(None);
    }
    read_json(&path).map(Some)
}

pub(crate) fn write_pdf_source_metadata(
    job_dir: &Path,
    metadata: &PdfSourceMetadata,
) -> Result<(), String> {
    write_json(&job_dir.join(PDF_SOURCE_FILENAME), metadata)
}

pub(crate) fn build_pdf_source_metadata(
    source_path: &Path,
    page_count: u32,
    filename: String,
    original_path: Option<String>,
    imported_at: Option<String>,
) -> Result<PdfSourceMetadata, String> {
    let now = timestamp_ms_string();
    Ok(PdfSourceMetadata {
        schema_version: SCHEMA_VERSION,
        page_count,
        source_fingerprint: fingerprint_file(source_path)?,
        filename,
        original_path,
        imported_at: imported_at.unwrap_or_else(|| now.clone()),
        updated_at: now,
    })
}

fn fingerprint_file(path: &Path) -> Result<String, String> {
    let file = File::open(path)
        .map_err(|error| format!("无法读取 PDF 指纹 {}: {error}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("无法计算 PDF 指纹 {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
