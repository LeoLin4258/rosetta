use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use tauri::{AppHandle, Manager};

pub(crate) fn relative_path_string(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "文件路径不在所选文件夹内。".to_string())?;
    let mut parts = Vec::new();

    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return Err("文件夹里包含不安全的相对路径。".to_string());
        };
        let Some(part) = part.to_str() else {
            return Err("文件路径包含无法识别的字符。".to_string());
        };
        parts.push(part.to_string());
    }

    if parts.is_empty() {
        return Err("文件路径为空。".to_string());
    }

    Ok(parts.join("/"))
}

pub(crate) fn path_from_relative(relative_path: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::new();
    for part in relative_path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err("文件相对路径不安全。".to_string());
        }
        path.push(part);
    }
    Ok(path)
}

pub(crate) fn cleanup_empty_dirs(current: &Path, stop_at: &Path) -> Result<(), String> {
    if !current.exists() || current == stop_at {
        return Ok(());
    }

    let is_empty = fs::read_dir(current)
        .map_err(|error| format!("无法读取目录 {}: {error}", current.display()))?
        .next()
        .is_none();

    if is_empty {
        fs::remove_dir(current)
            .map_err(|error| format!("无法删除空目录 {}: {error}", current.display()))?;
        if let Some(parent) = current.parent() {
            cleanup_empty_dirs(parent, stop_at)?;
        }
    }

    Ok(())
}

pub(crate) fn jobs_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法读取 Rosetta app data 目录: {error}"))?
        .join("jobs");
    fs::create_dir_all(&root).map_err(|error| format!("无法创建 jobs 目录: {error}"))?;
    Ok(root)
}

pub(crate) fn checked_job_dir(root: &Path, job_id: &str) -> Result<PathBuf, String> {
    if !is_safe_job_id(job_id) {
        return Err("项目 id 不安全。".to_string());
    }
    let dir = root.join(job_id);
    if !dir.starts_with(root) {
        return Err("项目路径越界。".to_string());
    }
    Ok(dir)
}

pub(crate) fn is_safe_job_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

pub(crate) fn translation_file_id(source_file_id: &str, target_lang: &str) -> String {
    format!(
        "tr-{}-{}",
        safe_id_component(source_file_id),
        safe_id_component(target_lang)
    )
}

pub(crate) fn safe_id_component(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if normalized.is_empty() {
        "item".to_string()
    } else {
        normalized
    }
}

pub(crate) fn new_job_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("document")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let stem = if stem.is_empty() {
        "document".to_string()
    } else {
        stem
    };
    format!("job-{}-{stem}", timestamp_ms_string())
}

pub(crate) fn timestamp_ms_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}
