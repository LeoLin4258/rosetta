use std::fmt;

use image::{
    codecs::png::{CompressionType, FilterType, PngEncoder},
    ImageEncoder,
};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium, Pixels};

use super::{
    patch_renderer::{
        translation_patch_page_pdf_cache_key, TranslationPatchPagePdf, TranslationPatchRenderError,
    },
    render_cache::{
        RenderCache, RenderCacheError, RenderCacheInsertOutcome, RenderCacheKey,
        RenderCacheOptions, RenderCacheOutputKind,
    },
    types::TranslationPatch,
};

pub(crate) const MIN_PREVIEW_PIXEL_WIDTH: u32 = 200;
pub(crate) const MAX_PREVIEW_PIXEL_WIDTH: u32 = 1_800;
pub(crate) const TRANSLATION_PATCH_PREVIEW_RASTERIZER_VERSION: &str =
    "rosetta-pdf-v3-preview-rasterizer/1";

#[derive(Debug)]
pub(crate) struct TranslationPatchPreviewPng {
    cache_key: RenderCacheKey,
    pixel_width: u32,
    pixel_height: u32,
    png_bytes: Vec<u8>,
}

impl TranslationPatchPreviewPng {
    pub(crate) fn pixel_width(&self) -> u32 {
        self.pixel_width
    }

    pub(crate) fn pixel_height(&self) -> u32 {
        self.pixel_height
    }

    pub(crate) fn png_bytes(&self) -> &[u8] {
        &self.png_bytes
    }

    pub(crate) fn into_png_bytes(self) -> Vec<u8> {
        self.png_bytes
    }
}

#[derive(Debug)]
pub(crate) enum TranslationPatchPreviewError {
    InvalidPixelWidth { requested: u32 },
    PagePdf(TranslationPatchRenderError),
    PdfiumLoad(String),
    InvalidPageCount(u32),
    PageRead(String),
    Render(String),
    PngEncode(String),
    InvalidPng(&'static str),
    Cache(RenderCacheError),
}

impl fmt::Display for TranslationPatchPreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPixelWidth { requested } => write!(
                formatter,
                "PDF preview width {requested} is outside {MIN_PREVIEW_PIXEL_WIDTH}..={MAX_PREVIEW_PIXEL_WIDTH}"
            ),
            Self::PagePdf(error) => error.fmt(formatter),
            Self::PdfiumLoad(message) => {
                write!(formatter, "failed to load translated page PDF in PDFium: {message}")
            }
            Self::InvalidPageCount(page_count) => write!(
                formatter,
                "translated page PDF has {page_count} pages instead of exactly one"
            ),
            Self::PageRead(message) => {
                write!(formatter, "failed to read translated preview page: {message}")
            }
            Self::Render(message) => {
                write!(formatter, "failed to rasterize translated preview page: {message}")
            }
            Self::PngEncode(message) => {
                write!(formatter, "failed to encode translated preview PNG: {message}")
            }
            Self::InvalidPng(reason) => {
                write!(formatter, "translated preview PNG is invalid: {reason}")
            }
            Self::Cache(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TranslationPatchPreviewError {}

impl From<TranslationPatchRenderError> for TranslationPatchPreviewError {
    fn from(value: TranslationPatchRenderError) -> Self {
        Self::PagePdf(value)
    }
}

impl From<RenderCacheError> for TranslationPatchPreviewError {
    fn from(value: RenderCacheError) -> Self {
        Self::Cache(value)
    }
}

pub(crate) fn translation_patch_preview_png_cache_key(
    source_fingerprint: &str,
    patch: &TranslationPatch,
    pixel_width: u32,
) -> Result<RenderCacheKey, TranslationPatchPreviewError> {
    validate_pixel_width(pixel_width)?;
    let mut key = translation_patch_page_pdf_cache_key(source_fingerprint, patch)?;
    key.renderer_version = format!(
        "{}+{TRANSLATION_PATCH_PREVIEW_RASTERIZER_VERSION}",
        key.renderer_version
    );
    key.options = RenderCacheOptions {
        output_kind: RenderCacheOutputKind::PreviewPng,
        pixel_width: Some(pixel_width),
        scale_milli: None,
    };
    Ok(key)
}

pub(crate) fn render_translation_patch_preview_png(
    pdfium: &Pdfium,
    page_pdf: &TranslationPatchPagePdf,
    pixel_width: u32,
) -> Result<TranslationPatchPreviewPng, TranslationPatchPreviewError> {
    let cache_key = translation_patch_preview_png_cache_key(
        page_pdf.source_fingerprint(),
        &page_pdf.render().resolved_patch,
        pixel_width,
    )?;
    let document = pdfium
        .load_pdf_from_byte_slice(page_pdf.pdf_bytes(), None)
        .map_err(|error| TranslationPatchPreviewError::PdfiumLoad(error.to_string()))?;
    let page_count = u32::try_from(document.pages().len()).unwrap_or(u32::MAX);
    if page_count != 1 {
        return Err(TranslationPatchPreviewError::InvalidPageCount(page_count));
    }
    let page = document
        .pages()
        .get(0)
        .map_err(|error| TranslationPatchPreviewError::PageRead(error.to_string()))?;
    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(pixel_width as Pixels))
        .map_err(|error| TranslationPatchPreviewError::Render(error.to_string()))?;
    let image = bitmap
        .as_image()
        .map_err(|error| {
            TranslationPatchPreviewError::Render(format!("bitmap conversion failed: {error:?}"))
        })?
        .to_rgba8();
    if image.width() != pixel_width || image.height() == 0 {
        return Err(TranslationPatchPreviewError::InvalidPng(
            "unexpected raster dimensions",
        ));
    }

    let mut png_bytes = Vec::with_capacity(64 * 1024);
    PngEncoder::new_with_quality(&mut png_bytes, CompressionType::Fast, FilterType::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| TranslationPatchPreviewError::PngEncode(error.to_string()))?;
    if !png_bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(TranslationPatchPreviewError::InvalidPng("signature"));
    }

    Ok(TranslationPatchPreviewPng {
        cache_key,
        pixel_width: image.width(),
        pixel_height: image.height(),
        png_bytes,
    })
}

pub(crate) fn insert_translation_patch_preview_png_cache(
    cache: &RenderCache,
    artifact: &TranslationPatchPreviewPng,
) -> Result<RenderCacheInsertOutcome, TranslationPatchPreviewError> {
    Ok(cache.insert(&artifact.cache_key, &artifact.png_bytes)?)
}

pub(crate) fn open_translation_patch_preview_png_cache(
    cache: &RenderCache,
    source_fingerprint: &str,
    patch: &TranslationPatch,
    pixel_width: u32,
) -> Result<Option<Vec<u8>>, TranslationPatchPreviewError> {
    let key = translation_patch_preview_png_cache_key(source_fingerprint, patch, pixel_width)?;
    let Some(lease) = cache.open(&key)? else {
        return Ok(None);
    };
    match lease.read_bytes() {
        Ok(bytes) => Ok(Some(bytes)),
        Err(RenderCacheError::CorruptArtifact { .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn validate_pixel_width(pixel_width: u32) -> Result<(), TranslationPatchPreviewError> {
    if !(MIN_PREVIEW_PIXEL_WIDTH..=MAX_PREVIEW_PIXEL_WIDTH).contains(&pixel_width) {
        return Err(TranslationPatchPreviewError::InvalidPixelWidth {
            requested: pixel_width,
        });
    }
    Ok(())
}
