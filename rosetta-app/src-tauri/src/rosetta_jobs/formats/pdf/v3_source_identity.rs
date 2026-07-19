use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use lru::LruCache;

use crate::pdf_v3::document::VerifiedDocumentIdentity;

const SOURCE_IDENTITY_CACHE_ENTRIES: usize = 32;

#[derive(Clone)]
pub struct PdfV3SourceIdentityState {
    entries: Arc<Mutex<LruCache<PathBuf, CachedSourceIdentity>>>,
}

#[derive(Clone)]
struct CachedSourceIdentity {
    stamp: SourceFileStamp,
    fingerprint: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SourceFileStamp {
    byte_count: u64,
    modified_ns: u128,
}

impl Default for PdfV3SourceIdentityState {
    fn default() -> Self {
        Self {
            entries: Arc::new(Mutex::new(LruCache::new(
                std::num::NonZeroUsize::new(SOURCE_IDENTITY_CACHE_ENTRIES)
                    .expect("non-zero source identity cache"),
            ))),
        }
    }
}

impl PdfV3SourceIdentityState {
    pub(crate) fn verify(
        &self,
        source_path: &Path,
        expected_fingerprint: &str,
    ) -> Result<(), String> {
        if !source_path.is_absolute() || !source_path.is_file() {
            return Err("PDF v3 preview source is unavailable".to_string());
        }
        let stamp = source_file_stamp(source_path)?;
        if let Some(cached) = self
            .entries
            .lock()
            .map_err(|_| "PDF v3 source identity cache is unavailable".to_string())?
            .get(source_path)
            .filter(|cached| cached.stamp == stamp)
            .cloned()
        {
            return if cached.fingerprint == expected_fingerprint {
                Ok(())
            } else {
                Err("PDF v3 preview source identity changed".to_string())
            };
        }

        let identity = VerifiedDocumentIdentity::verify(source_path)
            .map_err(|_| "PDF v3 preview source identity could not be verified".to_string())?;
        if source_file_stamp(source_path)? != stamp {
            return Err("PDF v3 preview source changed during verification".to_string());
        }
        let fingerprint = identity.source_fingerprint().to_string();
        self.entries
            .lock()
            .map_err(|_| "PDF v3 source identity cache is unavailable".to_string())?
            .put(
                source_path.to_path_buf(),
                CachedSourceIdentity { stamp, fingerprint },
            );
        if identity.source_fingerprint() != expected_fingerprint {
            return Err("PDF v3 preview source identity changed".to_string());
        }
        Ok(())
    }
}

fn source_file_stamp(path: &Path) -> Result<SourceFileStamp, String> {
    let metadata = fs::metadata(path)
        .map_err(|_| "PDF v3 preview source metadata is unavailable".to_string())?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("PDF v3 preview source is invalid".to_string());
    }
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    Ok(SourceFileStamp {
        byte_count: metadata.len(),
        modified_ns,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use sha2::{Digest, Sha256};

    use super::PdfV3SourceIdentityState;

    #[test]
    fn cached_source_identity_rejects_content_drift() {
        let path = std::env::temp_dir().join(format!(
            "rosetta-pdf-v3-source-identity-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::write(&path, b"first").expect("source");
        let first = format!("sha256:{:x}", Sha256::digest(b"first"));
        let state = PdfV3SourceIdentityState::default();
        state.verify(&path, &first).expect("first verification");
        state.verify(&path, &first).expect("cached verification");

        fs::write(&path, b"second-longer").expect("changed source");
        assert!(state.verify(&path, &first).is_err());
        let _ = fs::remove_file(path);
    }
}
