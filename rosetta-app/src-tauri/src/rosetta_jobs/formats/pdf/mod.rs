//! PDF format support.
//!
//! v1 pipeline (validated by [`experiments/pdf-spike`]): extract text + bbox via
//! pdfium-render → translate per existing block/segment workflow → regenerate
//! a translated PDF that preserves images / vectors / background by copying the
//! source page and clearing only the original text objects, then drawing the
//! translation in an embedded CJK font.
//!
//! Sub-modules are intentionally small and single-purpose so Phase 3 work
//! (layout confidence, complex-page handling) can land in `layout.rs` without
//! touching extract/generate.

pub(crate) mod docling;
pub(crate) mod errors;
pub(crate) mod extract;
pub(crate) mod generate;
pub(crate) mod runtime;

pub(crate) use extract::parse_pdf;
pub(crate) use generate::render_translated_pdf;
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
        PDFIUM_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("pdf-sidecar")
            .join("pdfium")
            .join(if cfg!(target_arch = "aarch64") {
                "mac-arm64"
            } else {
                "mac-x64"
            })
            .join("libpdfium.dylib")
    }

    pub(crate) fn font_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("pdf-sidecar")
            .join("fonts")
            .join("SourceHanSansCN-Regular.otf")
    }

    pub(crate) fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("pdf")
            .join(name)
    }
}
