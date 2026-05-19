use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::rosetta_jobs::model::MAX_PROJECT_FILES;

pub(crate) fn document_format(path: &Path) -> Result<SourceFormat, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match extension.as_str() {
        "txt" => Ok(SourceFormat::Txt),
        "md" | "markdown" => Ok(SourceFormat::Markdown),
        "pdf" => Ok(SourceFormat::Pdf),
        _ => Err("当前只支持导入 .txt、.md、.markdown、.pdf 文件。".to_string()),
    }
}

pub(crate) fn collect_supported_source_paths(
    root: &Path,
    current: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(|error| format!("无法读取文件夹 {}: {error}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法读取文件夹条目: {error}"))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("无法读取文件类型 {}: {error}", path.display()))?;
        if file_type.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
            collect_supported_source_paths(root, &path, output)?;
            continue;
        }

        if file_type.is_file() {
            // v1: folder import is TXT/Markdown only. PDFs use the single-file
            // import flow because the parser needs AppHandle access and we
            // don't yet support multi-file PDF projects.
            match document_format(&path) {
                Ok(SourceFormat::Pdf) => continue,
                Ok(_) => {}
                Err(_) => continue,
            }
            ensure_project_relative_path(root, &path)?;
            output.push(path);
            if output.len() > MAX_PROJECT_FILES {
                return Err(format!(
                    "这个文件夹包含超过 {MAX_PROJECT_FILES} 个可导入文件，请先拆分项目。"
                ));
            }
        }
    }

    Ok(())
}

fn ensure_project_relative_path(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "文件路径不在所选文件夹内。".to_string())?;

    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("文件夹里包含不安全的相对路径。".to_string());
    }

    Ok(())
}

pub(crate) mod markdown;
pub(crate) mod pdf;
pub(crate) mod txt;

use crate::rosetta_jobs::model::{RosettaBlock, Segment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceFormat {
    Txt,
    Markdown,
    Pdf,
}

impl SourceFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SourceFormat::Txt => "txt",
            SourceFormat::Markdown => "markdown",
            SourceFormat::Pdf => "pdf",
        }
    }
}

pub(crate) struct ParsedSource {
    pub(crate) blocks: Vec<RosettaBlock>,
    pub(crate) segments: Vec<Segment>,
}

pub(crate) fn parse_source(
    format: SourceFormat,
    document_id: &str,
    contents: &str,
) -> ParsedSource {
    let (blocks, segments) = match format {
        SourceFormat::Txt => txt::parse_txt(document_id, contents),
        SourceFormat::Markdown => markdown::parse_markdown(document_id, contents),
        SourceFormat::Pdf => {
            // PDFs are imported as cached binary sources and translated by
            // pdf2zh, so they never pass through this text parser.
            panic!("parse_source called with SourceFormat::Pdf");
        }
    };

    ParsedSource { blocks, segments }
}
