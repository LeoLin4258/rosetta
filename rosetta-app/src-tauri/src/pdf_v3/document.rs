use std::{
    fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use pdfium_render::prelude::{PdfDocument, Pdfium};
use sha2::{Digest, Sha256};

use super::source_object::{PdfSourceObjectError, PdfSourceObjectStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DocumentHandleError {
    Read(String),
    SourceObject(PdfSourceObjectError),
    PdfiumLoad(String),
    EncryptedDocument,
    PageCountMismatch { source: u32, pdfium: u32 },
}

impl fmt::Display for DocumentHandleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(message) | Self::PdfiumLoad(message) => formatter.write_str(message),
            Self::SourceObject(error) => error.fmt(formatter),
            Self::EncryptedDocument => {
                formatter.write_str("encrypted PDFs are not supported by PDF v3")
            }
            Self::PageCountMismatch { source, pdfium } => write!(
                formatter,
                "PDF engine page-count mismatch: source index reported {source}, PDFium reported {pdfium}"
            ),
        }
    }
}

impl std::error::Error for DocumentHandleError {}

pub(crate) struct DocumentHandle<'pdfium> {
    source_path: PathBuf,
    source_fingerprint: String,
    source_bytes: usize,
    page_count: u32,
    open_elapsed: Duration,
    source_objects: PdfSourceObjectStore,
    pdfium_document: PdfDocument<'pdfium>,
}

impl<'pdfium> DocumentHandle<'pdfium> {
    pub(crate) fn open(
        pdfium: &'pdfium Pdfium,
        source_path: impl AsRef<Path>,
    ) -> Result<Self, DocumentHandleError> {
        let started = Instant::now();
        let source_path = source_path.as_ref().to_path_buf();
        let (source_fingerprint, source_bytes) = fingerprint_file(&source_path)?;
        let source_objects = PdfSourceObjectStore::open(&source_path)?;
        let source_page_count = source_objects.page_count();
        let pdfium_document = pdfium
            .load_pdf_from_file(&source_path, None)
            .map_err(|error| {
                DocumentHandleError::PdfiumLoad(format!("failed to load PDF with PDFium: {error}"))
            })?;
        let pdfium_page_count = pdfium_document.pages().len() as u32;
        if source_page_count != pdfium_page_count {
            return Err(DocumentHandleError::PageCountMismatch {
                source: source_page_count,
                pdfium: pdfium_page_count,
            });
        }

        Ok(Self {
            source_path,
            source_fingerprint,
            source_bytes,
            page_count: source_page_count,
            open_elapsed: started.elapsed(),
            source_objects,
            pdfium_document,
        })
    }

    pub(crate) fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub(crate) fn source_fingerprint(&self) -> &str {
        &self.source_fingerprint
    }

    pub(crate) fn source_bytes(&self) -> usize {
        self.source_bytes
    }

    pub(crate) fn page_count(&self) -> u32 {
        self.page_count
    }

    pub(crate) fn open_elapsed(&self) -> Duration {
        self.open_elapsed
    }

    pub(crate) fn source_objects(&self) -> &PdfSourceObjectStore {
        &self.source_objects
    }

    pub(crate) fn pdfium_document(&self) -> &PdfDocument<'pdfium> {
        &self.pdfium_document
    }
}

impl From<PdfSourceObjectError> for DocumentHandleError {
    fn from(value: PdfSourceObjectError) -> Self {
        match value {
            PdfSourceObjectError::EncryptedDocument => Self::EncryptedDocument,
            value => Self::SourceObject(value),
        }
    }
}

fn fingerprint_file(path: &Path) -> Result<(String, usize), DocumentHandleError> {
    let mut file = File::open(path)
        .map_err(|error| DocumentHandleError::Read(format!("failed to open PDF: {error}")))?;
    let source_bytes = file
        .metadata()
        .map_err(|error| DocumentHandleError::Read(format!("failed to inspect PDF: {error}")))?
        .len();
    let source_bytes = usize::try_from(source_bytes).map_err(|_| {
        DocumentHandleError::Read("PDF byte length exceeds this platform".to_string())
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| DocumentHandleError::Read(format!("failed to hash PDF: {error}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok((
        format!("sha256:{}", hex_digest(hasher.finalize().as_slice())),
        source_bytes,
    ))
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::DocumentHandle;
    use crate::{
        pdf_v3::page_index::PdfPageIndex,
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    #[test]
    fn opens_one_consistent_read_only_document_identity() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("simple-one-page.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");

        assert_eq!(handle.source_path(), source);
        assert_eq!(handle.page_count(), 1);
        assert!(handle.source_bytes() > 0);
        assert!(handle.source_fingerprint().starts_with("sha256:"));
        assert_eq!(handle.source_fingerprint().len(), 71);
        let page_index =
            PdfPageIndex::resolve_page(handle.source_objects(), 1).expect("lazy page index");
        assert_eq!(page_index.page_count(), 1);
        assert_eq!(page_index.selected_page_count(), 1);
        assert_eq!(handle.pdfium_document().pages().len(), 1);
    }

    #[test]
    fn repeated_handles_keep_the_same_source_identity() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("simple-one-page.pdf");
        let first = DocumentHandle::open(shared_pdfium(), &source).expect("first handle");
        let second = DocumentHandle::open(shared_pdfium(), &source).expect("second handle");

        assert_eq!(first.source_fingerprint(), second.source_fingerprint());
        assert_eq!(first.source_bytes(), second.source_bytes());
        assert_eq!(first.page_count(), second.page_count());
    }

    #[test]
    fn resolves_sparse_pages_without_a_complete_object_graph() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        assert_eq!(handle.page_count(), 30);
        for page_number in [1, 15, 30] {
            let page_index = PdfPageIndex::resolve_page(handle.source_objects(), page_number)
                .expect("selected page index");
            assert_eq!(
                page_index.page(page_number).unwrap().page_number(),
                page_number
            );
        }
        let stats = handle
            .source_objects()
            .cache_stats()
            .expect("source cache stats");
        assert!(stats.resident_entries <= 512);
        assert!(stats.resident_bytes <= 16 * 1024 * 1024);
    }
}
