//! PDF format support.
//!
//! Phase 3 pipeline: PDF import is a lightweight pre-flight + source cache,
//! while translation is delegated end-to-end to PDFMathTranslate (`pdf2zh`).
//! Rosetta keeps the existing pdfium rasterizer for source/translated preview.

pub(crate) mod diagnostics;
pub(crate) mod errors;
pub(crate) mod extract;
pub(crate) mod page_assemble;
pub(crate) mod page_state;
pub(crate) mod pdf2zh_invoke;
pub(crate) mod rasterize;
pub(crate) mod run_state;
pub(crate) mod runtime;
pub(crate) mod source_state;

pub(crate) use rasterize::{count_pages, render_page_as_png};
pub(crate) use runtime::{probe_status, PdfRuntimeStatus};

/// Shared test fixtures + a single process-wide pdfium binding for all
/// PDF-module unit tests. pdfium's `FPDF_InitLibrary` is global and can only
/// be active once — without this shared OnceLock, parallel cargo-test threads
/// in different submodules SIGSEGV / SIGTRAP racing init/destroy.
#[cfg(test)]
pub(crate) mod test_helpers {
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use pdfium_render::prelude::Pdfium;

    static PDFIUM_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Serializes pdfium-using tests. Even with the `thread_safe` feature
    /// enabled, pdfium's C library is not safe for concurrent calls — running
    /// two tests in parallel SIGSEGVs inside FPDF_* routines. Each test that
    /// touches pdfium calls this and holds the guard for the whole test body.
    pub(crate) fn pdfium_test_lock() -> MutexGuard<'static, ()> {
        PDFIUM_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn shared_pdfium() -> &'static Pdfium {
        static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
        PDFIUM.get_or_init(|| {
            let lib = pdfium_lib_path();
            let bindings = Pdfium::bind_to_library(&lib).expect("pdfium dylib must be staged");
            Pdfium::new(bindings)
        })
    }

    pub(crate) fn pdfium_lib_path() -> PathBuf {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let platform_dir = "mac-arm64";
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        let platform_dir = "mac-x64";
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        let platform_dir = "win-x64";
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let platform_dir = "linux-x64";

        #[cfg(target_os = "macos")]
        let lib_filename = "libpdfium.dylib";
        #[cfg(target_os = "linux")]
        let lib_filename = "libpdfium.so";
        #[cfg(target_os = "windows")]
        let lib_filename = "pdfium.dll";

        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("pdf-sidecar")
            .join("pdfium")
            .join(platform_dir)
            .join(lib_filename)
    }

    pub(crate) fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("pdf")
            .join(name)
    }
}
