use std::{fmt, path::Path, time::Instant};

use image::RgbaImage;
use pdfium_render::prelude::{PdfPage, PdfPageObjectsCommon, PdfRenderConfig, Pdfium, Pixels};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IdentityProbeError {
    Load(String),
    PageOutOfBounds {
        page: u32,
        page_count: u32,
    },
    PageRead {
        page: u32,
        message: String,
    },
    TextRead {
        page: u32,
        message: String,
    },
    TextReplace {
        page: u32,
        object: usize,
        message: String,
    },
    Regenerate {
        page: u32,
        message: String,
    },
    Save(String),
    Render {
        page: u32,
        message: String,
    },
    ImageDimensionsChanged,
}

impl fmt::Display for IdentityProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(message) | Self::Save(message) => formatter.write_str(message),
            Self::PageOutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
            Self::PageRead { page, message } => {
                write!(formatter, "failed to read PDF page {page}: {message}")
            }
            Self::TextRead { page, message } => {
                write!(
                    formatter,
                    "failed to read PDF text on page {page}: {message}"
                )
            }
            Self::TextReplace {
                page,
                object,
                message,
            } => write!(
                formatter,
                "failed to replace text object {object} on PDF page {page}: {message}"
            ),
            Self::Regenerate { page, message } => {
                write!(formatter, "failed to regenerate PDF page {page}: {message}")
            }
            Self::Render { page, message } => {
                write!(formatter, "failed to render PDF page {page}: {message}")
            }
            Self::ImageDimensionsChanged => {
                formatter.write_str("identity output image dimensions changed")
            }
        }
    }
}

impl std::error::Error for IdentityProbeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum IdentityProbeMode {
    SaveOnly,
    ReplaceText,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdentityProbeResult {
    pub mode: IdentityProbeMode,
    pub page_number: u32,
    pub source_text_chars: usize,
    pub output_text_chars: usize,
    pub source_text_hash: String,
    pub output_text_hash: String,
    pub first_text_difference_index: Option<usize>,
    pub text_exact_match: bool,
    pub replaced_object_count: usize,
    pub output_bytes: usize,
    pub changed_pixel_count: u64,
    pub changed_pixel_ratio: f64,
    pub mean_absolute_channel_difference: f64,
    pub max_channel_difference: u8,
    pub elapsed_ms: u64,
}

pub(crate) fn probe_save_roundtrip(
    pdfium: &Pdfium,
    source_path: &Path,
    page_number: u32,
    target_width: u32,
) -> Result<IdentityProbeResult, IdentityProbeError> {
    probe_identity(
        pdfium,
        source_path,
        page_number,
        target_width,
        IdentityProbeMode::SaveOnly,
    )
}

pub(crate) fn probe_identity_text_replacement(
    pdfium: &Pdfium,
    source_path: &Path,
    page_number: u32,
    target_width: u32,
) -> Result<IdentityProbeResult, IdentityProbeError> {
    probe_identity(
        pdfium,
        source_path,
        page_number,
        target_width,
        IdentityProbeMode::ReplaceText,
    )
}

fn probe_identity(
    pdfium: &Pdfium,
    source_path: &Path,
    page_number: u32,
    target_width: u32,
    mode: IdentityProbeMode,
) -> Result<IdentityProbeResult, IdentityProbeError> {
    let started = Instant::now();
    let document = pdfium
        .load_pdf_from_file(source_path, None)
        .map_err(|error| IdentityProbeError::Load(format!("failed to load PDF: {error}")))?;
    let source_page_count = document.pages().len() as u32;
    if page_number == 0 || page_number > source_page_count {
        return Err(IdentityProbeError::PageOutOfBounds {
            page: page_number,
            page_count: source_page_count,
        });
    }

    let mut page = document
        .pages()
        .get(page_number as i32 - 1)
        .map_err(|error| IdentityProbeError::PageRead {
            page: page_number,
            message: error.to_string(),
        })?;
    let source_text = page
        .text()
        .map_err(|error| IdentityProbeError::TextRead {
            page: page_number,
            message: error.to_string(),
        })?
        .all();
    let source_image = render_page(&page, page_number, target_width)?;

    let mut replaced_object_count = 0usize;
    if mode == IdentityProbeMode::ReplaceText {
        for (object_index, mut object) in page.objects().iter().enumerate() {
            let Some(text_object) = object.as_text_object_mut() else {
                continue;
            };
            let original_text = text_object.text();
            if original_text.is_empty() {
                continue;
            }
            text_object.set_text(&original_text).map_err(|error| {
                IdentityProbeError::TextReplace {
                    page: page_number,
                    object: object_index,
                    message: error.to_string(),
                }
            })?;
            replaced_object_count += 1;
        }
        page.regenerate_content()
            .map_err(|error| IdentityProbeError::Regenerate {
                page: page_number,
                message: error.to_string(),
            })?;
    }
    drop(page);

    let output_pdf = document
        .save_to_bytes()
        .map_err(|error| IdentityProbeError::Save(format!("failed to save PDF: {error}")))?;
    let output_document = pdfium
        .load_pdf_from_byte_slice(&output_pdf, None)
        .map_err(|error| IdentityProbeError::Load(format!("failed to reload PDF: {error}")))?;
    let output_page = output_document
        .pages()
        .get(page_number as i32 - 1)
        .map_err(|error| IdentityProbeError::PageRead {
            page: page_number,
            message: error.to_string(),
        })?;
    let output_text = output_page
        .text()
        .map_err(|error| IdentityProbeError::TextRead {
            page: page_number,
            message: error.to_string(),
        })?
        .all();
    let output_image = render_page(&output_page, page_number, target_width)?;
    let difference = compare_images(&source_image, &output_image)?;

    Ok(IdentityProbeResult {
        mode,
        page_number,
        source_text_chars: source_text.chars().count(),
        output_text_chars: output_text.chars().count(),
        source_text_hash: text_hash(&source_text),
        output_text_hash: text_hash(&output_text),
        first_text_difference_index: first_text_difference_index(&source_text, &output_text),
        text_exact_match: source_text == output_text,
        replaced_object_count,
        output_bytes: output_pdf.len(),
        changed_pixel_count: difference.changed_pixel_count,
        changed_pixel_ratio: difference.changed_pixel_ratio,
        mean_absolute_channel_difference: difference.mean_absolute_channel_difference,
        max_channel_difference: difference.max_channel_difference,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

pub(super) fn text_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex_digest(hasher.finalize().as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(super) fn first_text_difference_index(source: &str, output: &str) -> Option<usize> {
    let mut source_chars = source.chars();
    let mut output_chars = output.chars();
    let mut index = 0usize;
    loop {
        match (source_chars.next(), output_chars.next()) {
            (Some(source_char), Some(output_char)) if source_char == output_char => index += 1,
            (None, None) => return None,
            _ => return Some(index),
        }
    }
}

pub(super) fn render_page(
    page: &PdfPage<'_>,
    page_number: u32,
    target_width: u32,
) -> Result<RgbaImage, IdentityProbeError> {
    let config = PdfRenderConfig::new().set_target_width(target_width as Pixels);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|error| IdentityProbeError::Render {
            page: page_number,
            message: error.to_string(),
        })?;
    bitmap
        .as_image()
        .map(|image| image.to_rgba8())
        .map_err(|error| IdentityProbeError::Render {
            page: page_number,
            message: format!("bitmap conversion failed: {error:?}"),
        })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ImageDifference {
    pub changed_pixel_count: u64,
    pub changed_pixel_ratio: f64,
    pub mean_absolute_channel_difference: f64,
    pub max_channel_difference: u8,
}

pub(super) fn compare_images(
    source: &RgbaImage,
    output: &RgbaImage,
) -> Result<ImageDifference, IdentityProbeError> {
    if source.dimensions() != output.dimensions() {
        return Err(IdentityProbeError::ImageDimensionsChanged);
    }

    let mut changed_pixel_count = 0u64;
    let mut absolute_channel_difference = 0u64;
    let mut max_channel_difference = 0u8;
    for (source_pixel, output_pixel) in source.pixels().zip(output.pixels()) {
        let mut changed = false;
        for (source_channel, output_channel) in source_pixel.0.iter().zip(output_pixel.0.iter()) {
            let difference = source_channel.abs_diff(*output_channel);
            if difference > 0 {
                changed = true;
            }
            absolute_channel_difference += u64::from(difference);
            max_channel_difference = max_channel_difference.max(difference);
        }
        if changed {
            changed_pixel_count += 1;
        }
    }

    let pixel_count = u64::from(source.width()) * u64::from(source.height());
    let channel_count = pixel_count * 4;
    Ok(ImageDifference {
        changed_pixel_count,
        changed_pixel_ratio: if pixel_count == 0 {
            0.0
        } else {
            changed_pixel_count as f64 / pixel_count as f64
        },
        mean_absolute_channel_difference: if channel_count == 0 {
            0.0
        } else {
            absolute_channel_difference as f64 / channel_count as f64
        },
        max_channel_difference,
    })
}

#[cfg(test)]
mod tests {
    use super::{probe_identity_text_replacement, probe_save_roundtrip, text_hash};
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, pdfium_test_lock, shared_pdfium,
    };

    #[test]
    fn text_hash_keeps_canonical_lowercase_sha256() {
        assert_eq!(
            text_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn simple_pdf_save_roundtrip_is_pixel_exact() {
        let _guard = pdfium_test_lock();
        let result = probe_save_roundtrip(
            shared_pdfium(),
            &fixture_path("002-trivial-libre-office-writer.pdf"),
            1,
            900,
        )
        .expect("save roundtrip");

        assert!(result.text_exact_match);
        assert_eq!(result.changed_pixel_count, 0);
        assert_eq!(result.max_channel_difference, 0);
    }

    #[test]
    fn simple_pdf_identity_replacement_preserves_text_with_bounded_visual_drift() {
        let _guard = pdfium_test_lock();
        let result = probe_identity_text_replacement(
            shared_pdfium(),
            &fixture_path("002-trivial-libre-office-writer.pdf"),
            1,
            900,
        )
        .expect("identity replacement");

        assert!(result.replaced_object_count > 0);
        assert!(result.text_exact_match);
        println!(
            "pdf-v3 simple identity objects={} text_match={} chars={}/{} first_diff={:?} pixels={} ratio={:.6} mean={:.6} max={} bytes={} elapsed={}ms",
            result.replaced_object_count,
            result.text_exact_match,
            result.source_text_chars,
            result.output_text_chars,
            result.first_text_difference_index,
            result.changed_pixel_count,
            result.changed_pixel_ratio,
            result.mean_absolute_channel_difference,
            result.max_channel_difference,
            result.output_bytes,
            result.elapsed_ms
        );
        assert!(result.changed_pixel_ratio < 0.1);
        assert!(result.mean_absolute_channel_difference < 10.0);
    }

    #[test]
    #[ignore = "manual Windows PDFium real-page identity probe"]
    fn manual_windows_real_page_identity_probe() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let save_only =
            probe_save_roundtrip(shared_pdfium(), &source, 1, 1200).expect("save roundtrip");
        let replacement = probe_identity_text_replacement(shared_pdfium(), &source, 1, 1200)
            .expect("identity replacement");

        println!(
            "pdf-v3 real save-only text_match={} chars={}/{} first_diff={:?} pixels={} ratio={:.6} mean={:.6} max={} bytes={} elapsed={}ms",
            save_only.text_exact_match,
            save_only.source_text_chars,
            save_only.output_text_chars,
            save_only.first_text_difference_index,
            save_only.changed_pixel_count,
            save_only.changed_pixel_ratio,
            save_only.mean_absolute_channel_difference,
            save_only.max_channel_difference,
            save_only.output_bytes,
            save_only.elapsed_ms
        );
        println!(
            "pdf-v3 real replacement objects={} text_match={} chars={}/{} first_diff={:?} pixels={} ratio={:.6} mean={:.6} max={} bytes={} elapsed={}ms",
            replacement.replaced_object_count,
            replacement.text_exact_match,
            replacement.source_text_chars,
            replacement.output_text_chars,
            replacement.first_text_difference_index,
            replacement.changed_pixel_count,
            replacement.changed_pixel_ratio,
            replacement.mean_absolute_channel_difference,
            replacement.max_channel_difference,
            replacement.output_bytes,
            replacement.elapsed_ms
        );
    }
}
