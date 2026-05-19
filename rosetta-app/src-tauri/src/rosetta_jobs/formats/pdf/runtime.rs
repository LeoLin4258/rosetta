//! pdfium native library path resolution.
//!
//! Mirrors the path-resolution pattern used by [`managed_rwkv::status::locate_tokenizer`]
//! so the layout of pdf-sidecar resources stays consistent with rwkv-sidecar:
//! bundled at `<App>.app/Contents/Resources/resources/pdf-sidecar/...` and staged at
//! `<src-tauri>/resources/pdf-sidecar/...` during dev.
//!
//! pdfium-render is built with `thread_safe`, so we hold the bound library in a
//! process-wide `OnceCell<Pdfium>`. First touch from any Tauri command lazily
//! binds the dynamic library; subsequent calls reuse the same handle.

use std::path::{Path, PathBuf};

use once_cell::sync::OnceCell;
use pdfium_render::prelude::Pdfium;
use tauri::{AppHandle, Manager};

static PDFIUM: OnceCell<Pdfium> = OnceCell::new();

/// Returns the platform subdirectory inside `resources/pdf-sidecar/pdfium/`.
/// Matches the layout produced by `scripts/fetch-pdfium.sh`.
const fn platform_dir() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "mac-arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "mac-x64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "win-x64"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x64"
    }
}

const fn lib_filename() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "libpdfium.dylib"
    }
    #[cfg(target_os = "linux")]
    {
        "libpdfium.so"
    }
    #[cfg(target_os = "windows")]
    {
        "pdfium.dll"
    }
}

/// Probe ordered candidate paths for the pdfium dylib. Returns the first one
/// that exists on disk, or `None` if pdfium isn't staged anywhere we expect.
pub(crate) fn locate_pdfium_lib(app: &AppHandle) -> Option<PathBuf> {
    let rel = Path::new("resources")
        .join("pdf-sidecar")
        .join("pdfium")
        .join(platform_dir())
        .join(lib_filename());

    if let Ok(resource_dir) = app.path().resource_dir() {
        for candidate in [
            resource_dir.join(&rel),
            resource_dir.join("_up_").join(&rel),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&rel);
    if dev_path.is_file() {
        return Some(dev_path);
    }

    None
}

/// Lazily bind libpdfium.dylib and return a process-wide [`Pdfium`].
pub(crate) fn get_pdfium(app: &AppHandle) -> Result<&'static Pdfium, String> {
    PDFIUM.get_or_try_init(|| {
        let lib_path = locate_pdfium_lib(app).ok_or_else(|| {
            format!(
                "找不到 pdfium 库文件。期望位置：resources/pdf-sidecar/pdfium/{}/{}",
                platform_dir(),
                lib_filename(),
            )
        })?;
        let bindings = Pdfium::bind_to_library(&lib_path)
            .map_err(|error| format!("加载 pdfium 失败 ({}): {error}", lib_path.display()))?;
        Ok::<_, String>(Pdfium::new(bindings))
    })
}

/// Diagnostic snapshot returned by the smoke-test Tauri command. Surfaces
/// just enough state for a frontend probe to confirm everything is wired up
/// without exposing internal handles.
#[derive(Debug, serde::Serialize)]
pub(crate) struct PdfRuntimeStatus {
    pub pdfium_lib_path: Option<String>,
    pub pdfium_lib_loaded: bool,
    pub pdfium_version_tag: Option<String>,
    pub error: Option<String>,
}

pub(crate) fn probe_status(app: &AppHandle) -> PdfRuntimeStatus {
    let pdfium_lib_path = locate_pdfium_lib(app);

    let pdfium_version_tag = pdfium_lib_path
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.join("VERSION")))
        .and_then(|version_file| std::fs::read_to_string(version_file).ok())
        .map(|tag| tag.trim().to_string());

    let (loaded, error) = match get_pdfium(app) {
        Ok(_) => (true, None),
        Err(error) => (false, Some(error)),
    };

    PdfRuntimeStatus {
        pdfium_lib_path: pdfium_lib_path.map(|p| p.display().to_string()),
        pdfium_lib_loaded: loaded,
        pdfium_version_tag,
        error,
    }
}

#[cfg(test)]
mod tests {
    //! Tests bypass AppHandle entirely and walk the same dev path the runtime
    //! falls back to. They will fail with a clear hint if the developer hasn't
    //! run `scripts/fetch-pdfium.sh` yet.

    use super::*;

    fn dev_pdfium_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("pdf-sidecar")
            .join("pdfium")
            .join(platform_dir())
            .join(lib_filename())
    }

    #[test]
    fn pdfium_dylib_is_staged() {
        let p = dev_pdfium_path();
        assert!(
            p.is_file(),
            "pdfium dylib not staged at {}. Run scripts/fetch-pdfium.sh first.",
            p.display(),
        );
    }

    // Binding pdfium is covered by [`super::extract::tests`] via a shared
    // OnceLock — duplicating it here would race the global FPDF_InitLibrary
    // and SIGTRAP in parallel test runs.
}
