//! One-time migrations between Rosetta versions for the managed RWKV
//! runtime's on-disk state.
//!
//! When a profile is superseded (e.g. WebRWKV → MLX in beta.8), the old
//! model directory is left orphaned under `<app-local-data>/managed-rwkv/models/`.
//! For the WebRWKV 1.5B model that's 1.26 GB sitting around forever for
//! every upgrading user. This module sweeps known-legacy directories on
//! startup so the upgrade is free of "go figure out which folder to delete"
//! work.
//!
//! Design notes:
//!
//! - **Idempotent.** Safe to call on every startup; no-ops once the legacy
//!   directories are gone.
//! - **Best-effort.** Failures (permission errors, FS races) are logged but
//!   don't block app startup. We'd rather start in a slightly stale state
//!   than refuse to launch.
//! - **No active-profile cleanup.** This is intentionally only about
//!   *legacy* profiles. The current profile's artifacts are managed by
//!   `install.rs::cleanup_artifacts` and friends, which run inside the
//!   normal install flow.
//! - **No state migration.** We don't try to convert old `manifest.json`
//!   files into new ones or anything similar. The new MLX profile uses a
//!   different model + format, so the on-disk artifacts are fully
//!   replaceable; deletion + redownload is simpler than translation.
//!
//! Adding a new entry: append a `LegacyArtifact` to `LEGACY_ARTIFACTS`. Do
//! NOT remove existing entries even after they've been deployed for a while
//! — users who skip multiple versions still need them to fire.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// A legacy on-disk artifact that should be cleaned up on startup.
///
/// `subpath` is resolved relative to `<app_local_data>/`. Everything else
/// is purely diagnostic — `reason` shows in the startup log when the
/// artifact is found and deleted.
struct LegacyArtifact {
    /// Path under `<app_local_data>/` to remove. May be a file or directory.
    subpath: &'static str,
    /// Why this is being cleaned up — surfaced in eprintln for support.
    reason: &'static str,
}

/// Directories / files that previous Rosetta versions left behind and that
/// the current build no longer needs. Each entry is checked at startup.
///
/// **Do not delete entries from this array** even after they've shipped for
/// a long time; users skipping multiple versions still rely on them.
const LEGACY_ARTIFACTS: &[LegacyArtifact] = &[LegacyArtifact {
    // beta.7 and earlier: WebRWKV 1.5B nf4 model, ~1.26 GB on disk.
    // Superseded by MLX 0.4B 6-bit in beta.8 (different model_directory_name).
    // The directory contains the .prefab weight file + `manifest.json` + any
    // `.part` / `.part.broken` files from partial downloads.
    subpath: "managed-rwkv/models/rwkv-translate-1.5b-nf4",
    reason: "WebRWKV 1.5B 模型（已被 MLX 0.4B 取代，beta.8 起不再使用）",
}];

/// Run all known migrations. Call once at app startup, after the AppHandle
/// is available and before the install plan is computed for the first time.
///
/// Returns total bytes reclaimed across all migrations (mostly informational
/// for logging; callers can ignore it).
pub fn run_migrations(app: &AppHandle) -> u64 {
    let Ok(base) = app.path().app_local_data_dir() else {
        eprintln!("[rwkv-migrate] could not resolve app_local_data_dir; skipping migrations");
        return 0;
    };

    let mut reclaimed_bytes: u64 = 0;
    for artifact in LEGACY_ARTIFACTS {
        let path = base.join(artifact.subpath);
        match cleanup_artifact(&path, artifact.reason) {
            Ok(bytes) => reclaimed_bytes = reclaimed_bytes.saturating_add(bytes),
            Err(error) => {
                eprintln!(
                    "[rwkv-migrate] failed to clean {} ({}): {error}",
                    path.display(),
                    artifact.reason
                );
            }
        }
    }

    if reclaimed_bytes > 0 {
        eprintln!(
            "[rwkv-migrate] reclaimed {:.1} MB total from legacy artifacts",
            reclaimed_bytes as f64 / (1024.0 * 1024.0)
        );
    }
    reclaimed_bytes
}

/// Remove a single legacy artifact (file or directory). Returns the bytes
/// reclaimed (0 if the artifact didn't exist).
fn cleanup_artifact(path: &Path, reason: &str) -> std::io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let metadata = std::fs::symlink_metadata(path)?;
    let bytes = if metadata.is_dir() {
        dir_size_bytes(path).unwrap_or(0)
    } else {
        metadata.len()
    };
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    eprintln!(
        "[rwkv-migrate] removed {} ({}) — {:.1} MB reclaimed",
        path.display(),
        reason,
        bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(bytes)
}

/// Recursive size of `dir`, ignoring symlinks (so we don't double-count or
/// follow into shared paths). Errors mid-walk skip that entry but keep summing.
fn dir_size_bytes(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_artifact_is_noop_when_missing() {
        let tmp = std::env::temp_dir().join(format!(
            "rosetta-migrate-noop-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let bytes = cleanup_artifact(&tmp, "test").expect("noop should not error");
        assert_eq!(bytes, 0);
    }

    #[test]
    fn cleanup_artifact_removes_directory_and_reports_size() {
        let tmp = std::env::temp_dir().join(format!(
            "rosetta-migrate-dir-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let nested = tmp.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("a.bin"), vec![0u8; 1024]).unwrap();
        std::fs::write(tmp.join("b.bin"), vec![0u8; 2048]).unwrap();

        let reclaimed = cleanup_artifact(&tmp, "test").expect("cleanup ok");
        assert_eq!(reclaimed, 3072, "expected sum of file sizes");
        assert!(!tmp.exists(), "tmp dir should be gone");
    }

    #[test]
    fn cleanup_artifact_removes_single_file_and_reports_size() {
        let tmp = std::env::temp_dir().join(format!(
            "rosetta-migrate-file-{}.bin",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&tmp, vec![0u8; 4096]).unwrap();
        let reclaimed = cleanup_artifact(&tmp, "test").expect("cleanup ok");
        assert_eq!(reclaimed, 4096);
        assert!(!tmp.exists());
    }
}
