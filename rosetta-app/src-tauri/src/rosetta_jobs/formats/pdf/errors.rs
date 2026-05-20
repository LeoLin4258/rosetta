//! User-visible PDF-specific failure modes. Each variant carries the
//! Chinese-language wording that surfaces in the import flow, so the rest of
//! the pipeline doesn't have to translate or massage messages.

use std::fmt;

#[derive(Debug)]
pub(crate) enum PdfError {
    /// File doesn't exist, isn't readable, or isn't a PDF.
    Read(String),
    /// pdfium failed to parse the document (malformed, partial download, …).
    Parse(String),
    /// PDF is password-protected; v1 doesn't attempt to prompt or decrypt.
    Encrypted,
    /// No extractable text on any page — almost certainly a scanned/image-only PDF.
    /// v1 doesn't run OCR.
    ImageOnly,
    /// File or page count exceeds the conservative v1 limits.
    TooLarge { reason: String },
    /// pdfium dylib is not staged in resources/pdf-sidecar yet.
    RuntimeMissing(String),
    /// The PDF layout processor failed before producing the expected output file.
    Pdf2zhFailed(String),
    /// Translation was cancelled by the user.
    Cancelled,
}

impl PdfError {
    pub(crate) fn user_message(&self) -> String {
        match self {
            Self::Read(detail) => format!("无法读取 PDF 文件：{detail}"),
            Self::Parse(detail) => format!("PDF 文件解析失败：{detail}"),
            Self::Encrypted => {
                "这份 PDF 加了密码，当前版本暂不支持加密 PDF 的翻译。".to_string()
            }
            Self::ImageOnly => "这份 PDF 看起来是扫描件或纯图片，没有可提取的文字。Rosetta 当前版本暂不做 OCR。".to_string(),
            Self::TooLarge { reason } => format!("PDF 超过当前版本的处理上限：{reason}"),
            Self::RuntimeMissing(detail) => format!("PDF 翻译运行时未就绪：{detail}"),
            Self::Pdf2zhFailed(detail) => format!("PDF 译文生成失败：{detail}"),
            Self::Cancelled => "PDF 翻译已取消。".to_string(),
        }
    }
}

impl fmt::Display for PdfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.user_message())
    }
}

impl From<PdfError> for String {
    fn from(error: PdfError) -> Self {
        error.user_message()
    }
}

/// v1 import limits. Tracked here so a v1.1 bump only touches one place.
pub(crate) const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024; // 100 MB
pub(crate) const MAX_PDF_PAGES: u32 = 500;
