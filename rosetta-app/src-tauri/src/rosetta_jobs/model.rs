use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_IMPORT_BYTES: u64 = 5 * 1024 * 1024;
pub(crate) const MAX_PROJECT_FILES: usize = 200;
pub(crate) const MAX_SEGMENT_CHARS: usize = 1_800;
pub(crate) const JOB_INDEX_FILENAME: &str = "index.json";
pub(crate) const TRANSLATION_REVISIONS_FILENAME: &str = "translation_revisions.json";
pub(crate) const TRANSLATION_FILES_FILENAME: &str = "translation_files.json";
pub(crate) const TRANSLATIONS_DIRNAME: &str = "translations";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RosettaExportKind {
    Translation,
    Bilingual,
}

impl RosettaExportKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Bilingual => "bilingual",
        }
    }
}

impl FromStr for RosettaExportKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "translation" => Ok(Self::Translation),
            "bilingual" => Ok(Self::Bilingual),
            _ => Err("导出类型必须是 translation 或 bilingual。".to_string()),
        }
    }
}

impl fmt::Display for RosettaExportKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranslationRevisionReason {
    FileRetranslation,
    SelectionRetranslation,
    LanguageChange,
}

impl TranslationRevisionReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::FileRetranslation => "file-retranslation",
            Self::SelectionRetranslation => "selection-retranslation",
            Self::LanguageChange => "language-change",
        }
    }
}

impl FromStr for TranslationRevisionReason {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "file-retranslation" => Ok(Self::FileRetranslation),
            "selection-retranslation" => Ok(Self::SelectionRetranslation),
            "language-change" => Ok(Self::LanguageChange),
            _ => Err("历史版本原因不支持。".to_string()),
        }
    }
}

impl fmt::Display for TranslationRevisionReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaDocument {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) filename: String,
    pub(crate) format: String,
    pub(crate) source_lang: Option<String>,
    pub(crate) target_lang: String,
    #[serde(default)]
    pub(crate) files: Vec<RosettaSourceFile>,
    pub(crate) blocks: Vec<RosettaBlock>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaSourceFile {
    pub(crate) id: String,
    pub(crate) filename: String,
    pub(crate) relative_path: String,
    pub(crate) format: String,
    #[serde(default)]
    pub(crate) source_lang: Option<String>,
    #[serde(default)]
    pub(crate) target_lang: Option<String>,
    #[serde(default = "default_file_translation_status")]
    pub(crate) translation_status: String,
    #[serde(default)]
    pub(crate) segment_count: usize,
    #[serde(default)]
    pub(crate) completed_segments: usize,
    #[serde(default)]
    pub(crate) failed_segments: usize,
    #[serde(default)]
    pub(crate) translating_segments: usize,
    pub(crate) block_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaBlock {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) file_id: Option<String>,
    #[serde(rename = "type")]
    pub(crate) block_type: String,
    pub(crate) source_text: String,
    pub(crate) translated_text: Option<String>,
    pub(crate) should_translate: bool,
    pub(crate) order: usize,
    pub(crate) path: Option<String>,
    pub(crate) style: Option<Value>,
    pub(crate) status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub(crate) id: String,
    pub(crate) block_id: String,
    #[serde(default)]
    pub(crate) file_id: Option<String>,
    pub(crate) order: usize,
    pub(crate) source_text: String,
    pub(crate) translated_text: Option<String>,
    pub(crate) source_lang: Option<String>,
    pub(crate) target_lang: String,
    pub(crate) kind: String,
    pub(crate) preserve_whitespace: bool,
    pub(crate) status: String,
    pub(crate) block_order: Option<usize>,
    pub(crate) segment_index_in_block: Option<usize>,
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) translation_history: Vec<TranslationHistoryEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TranslationHistoryEntry {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) run_id: Option<String>,
    pub(crate) translated_text: String,
    pub(crate) created_at: String,
    pub(crate) source_lang: Option<String>,
    pub(crate) target_lang: String,
    pub(crate) reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaJobSummary {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) filename: String,
    pub(crate) format: String,
    pub(crate) source_path: Option<String>,
    pub(crate) source_filename: String,
    #[serde(default = "default_source_kind")]
    pub(crate) source_kind: String,
    #[serde(default = "default_file_count")]
    pub(crate) file_count: usize,
    #[serde(default)]
    pub(crate) source_files: Vec<RosettaSourceFile>,
    pub(crate) status: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) exported_at: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) target_lang: String,
    pub(crate) segment_count: usize,
    pub(crate) completed_segments: usize,
    pub(crate) failed_segments: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaJobBundle {
    pub(crate) schema_version: u32,
    pub(crate) job: RosettaJobSummary,
    pub(crate) document: RosettaDocument,
    pub(crate) segments: Vec<Segment>,
    pub(crate) translation_files: Vec<RosettaTranslationFile>,
    pub(crate) translation_revisions: Vec<TranslationRevision>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaTranslationFile {
    pub(crate) id: String,
    pub(crate) source_file_id: String,
    pub(crate) target_lang: String,
    pub(crate) status: String,
    pub(crate) segment_count: usize,
    pub(crate) completed_segments: usize,
    pub(crate) failed_segments: usize,
    pub(crate) updated_at: String,
    pub(crate) exported_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSegment {
    pub(crate) source_segment_id: String,
    pub(crate) translated_text: Option<String>,
    pub(crate) target_lang: String,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) translation_history: Vec<TranslationHistoryEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaTranslationFileBundle {
    pub(crate) translation_file: RosettaTranslationFile,
    pub(crate) segments: Vec<TranslationSegment>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TranslationRevision {
    pub(crate) id: String,
    pub(crate) job_id: String,
    pub(crate) file_id: String,
    pub(crate) created_at: String,
    pub(crate) source_lang: Option<String>,
    pub(crate) target_lang: String,
    pub(crate) reason: String,
    #[serde(default)]
    pub(crate) scope_block_ids: Option<Vec<String>>,
    pub(crate) segment_translations: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaJobIndex {
    pub(crate) schema_version: u32,
    pub(crate) jobs: Vec<RosettaJobSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaExportResult {
    pub(crate) job: RosettaJobSummary,
    pub(crate) target_path: String,
    pub(crate) kind: String,
    pub(crate) bytes_written: u64,
    pub(crate) files_written: usize,
    pub(crate) message: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaJobFileDeleteResult {
    pub(crate) deleted_job: bool,
    pub(crate) jobs: Vec<RosettaJobSummary>,
    pub(crate) bundle: Option<RosettaJobBundle>,
    pub(crate) message: String,
}

#[derive(Debug)]
pub(crate) struct SourceSnapshot {
    pub(crate) relative_path: String,
    pub(crate) contents: String,
}

pub(crate) fn default_source_kind() -> String {
    "file".to_string()
}

pub(crate) fn default_file_count() -> usize {
    1
}

pub(crate) fn default_file_translation_status() -> String {
    "untranslated".to_string()
}
