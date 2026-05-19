//! Docling sidecar integration: long-running Python `docling-serve` process
//! that parses PDFs into layout-aware blocks.
//!
//! Why an external sidecar:
//! - Docling is a Python project with significant native deps (torch, ONNX
//!   runtime, OpenCV). It's not realistic to embed it in the Rust binary.
//! - Spawning Python per import would lose ~5s to module/model load every
//!   time. Keeping the server alive amortizes that to one-time startup.
//! - Matches the pattern we already use for the RWKV translator sidecar.
//!
//! Module layout:
//! - [`sidecar`] — process lifecycle (spawn / health-wait / kill on drop).
//! - [`extract`] — HTTP client that POSTs a PDF to `/v1/convert/file` and
//!   maps the response into Rosetta's block/segment IR.
//!
//! Resource resolution (where the docling-serve binary lives):
//! - **Production**: `<app data dir>/docling-sidecar/bin/docling-serve` —
//!   downloaded as a relocatable Python+venv pack at first PDF import.
//! - **Development**: set `ROSETTA_DOCLING_SERVE_BIN` to the venv binary
//!   inside `experiments/docling-probe/.venv/bin/docling-serve`.
//!
//! See memory: [project-pdf-layout-extractor-choice].

pub(crate) mod extract;
pub(crate) mod sidecar;

#[allow(unused_imports)] // wired into lib.rs in Phase 1.6e
pub(crate) use extract::extract_via_docling;
#[allow(unused_imports)]
pub(crate) use sidecar::{DoclingSidecarRegistry, DoclingSidecarSnapshot};
