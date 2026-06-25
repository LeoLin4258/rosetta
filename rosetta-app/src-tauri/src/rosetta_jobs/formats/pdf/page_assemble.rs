use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use lopdf::{dictionary, Document, Object, ObjectId};

use crate::rosetta_jobs::formats::pdf::page_state::PdfPageTranslationState;

pub(crate) fn assemble_pdf_with_page_translations(
    source_path: &Path,
    job_dir: &Path,
    state: &PdfPageTranslationState,
    target_path: &Path,
) -> Result<(), String> {
    let source_doc =
        Document::load(source_path).map_err(|error| format!("无法读取源 PDF 用于导出: {error}"))?;
    let source_page_count = source_doc.get_pages().len() as u32;
    if source_page_count == 0 {
        return Err("源 PDF 没有页面，无法导出。".to_string());
    }

    let mut page_sources = Vec::new();
    for page_number in 1..=source_page_count {
        if let Some(translated_path) = translated_page_path(job_dir, state, page_number) {
            page_sources.push(PageSource {
                path: translated_path,
                page_number: 1,
            });
        } else {
            page_sources.push(PageSource {
                path: source_path.to_path_buf(),
                page_number,
            });
        }
    }

    let mut merged = merge_single_pages(&page_sources)?;
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 PDF 导出目录: {error}"))?;
    }
    merged
        .save(target_path)
        .map_err(|error| format!("无法写入完整译文 PDF: {error}"))?;
    Ok(())
}

/// Count pages of a PDF on disk via lopdf (no pdfium dependency). Used to
/// figure out whether a batch pdf2zh output contains the whole document or
/// only the selected pages.
pub(crate) fn count_pdf_pages_lopdf(path: &Path) -> Result<u32, String> {
    let doc = Document::load(path)
        .map_err(|error| format!("无法读取 PDF {}: {error}", path.display()))?;
    Ok(doc.get_pages().len() as u32)
}

/// Extract several single pages from one PDF into separate one-page PDFs.
/// Loads the source document once and clones per page, which is much cheaper
/// than re-parsing the file for every page.
///
/// For each `(requested_page_number, target_path)`: if the requested page
/// exists it is used; if the document has exactly one page, that page is used
/// regardless (pdf2zh sometimes emits a renumbered single-page output).
#[cfg(test)]
pub(crate) fn extract_pages_pdf(
    source_path: &Path,
    extractions: &[(u32, PathBuf)],
) -> Result<(), String> {
    let doc = Document::load(source_path)
        .map_err(|error| format!("无法读取 PDF 页面缓存 {}: {error}", source_path.display()))?;
    let pages = doc.get_pages();

    for (requested_page_number, target_path) in extractions {
        let page_number = if pages.contains_key(requested_page_number) {
            *requested_page_number
        } else if pages.len() == 1 {
            1
        } else {
            return Err(format!(
                "PDF 输出中不存在第 {requested_page_number} 页，无法缓存页级译文。"
            ));
        };

        let mut single = merge_loaded_pages(vec![(doc.clone(), page_number)])?;
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建 PDF 页缓存目录: {error}"))?;
        }
        single
            .save(target_path)
            .map_err(|error| format!("无法写入 PDF 页缓存: {error}"))?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct PageSource {
    path: PathBuf,
    page_number: u32,
}

fn translated_page_path(
    job_dir: &Path,
    state: &PdfPageTranslationState,
    page_number: u32,
) -> Option<PathBuf> {
    let page = state.pages.iter().find(|page| {
        page.page_number == page_number
            && page.status == "translated"
            && page.translated_pdf_path.is_some()
    })?;
    let path = job_dir.join(page.translated_pdf_path.as_ref()?);
    if path.is_file() {
        return Some(path);
    }
    let legacy_lang_path = job_dir.join(
        crate::rosetta_jobs::formats::pdf::page_state::legacy_pdf_page_relative_path_for_lang(
            &state.target_lang,
            page_number,
        ),
    );
    if legacy_lang_path.is_file() {
        return Some(legacy_lang_path);
    }
    let legacy_path = job_dir.join(
        crate::rosetta_jobs::formats::pdf::page_state::legacy_pdf_page_relative_path(page_number),
    );
    legacy_path.is_file().then_some(legacy_path)
}

fn merge_single_pages(page_sources: &[PageSource]) -> Result<Document, String> {
    let mut loaded = Vec::with_capacity(page_sources.len());
    for source in page_sources {
        let doc = Document::load(&source.path)
            .map_err(|error| format!("无法读取 PDF 页面 {}: {error}", source.path.display()))?;
        loaded.push((doc, source.page_number));
    }
    merge_loaded_pages(loaded)
}

fn merge_loaded_pages(loaded: Vec<(Document, u32)>) -> Result<Document, String> {
    let mut documents_pages = BTreeMap::<ObjectId, Object>::new();
    let mut documents_objects = BTreeMap::<ObjectId, Object>::new();
    let mut document = Document::with_version("1.5");
    let mut max_id = 1;

    for (mut doc, page_number) in loaded {
        keep_only_page(&mut doc, page_number)?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        for (_, object_id) in doc.get_pages() {
            let object = doc
                .get_object(object_id)
                .map_err(|error| format!("无法读取 PDF 页面对象: {error}"))?
                .to_owned();
            documents_pages.insert(object_id, object);
        }
        documents_objects.extend(doc.objects);
    }

    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    for (object_id, object) in documents_objects {
        match object.type_name().unwrap_or("") {
            "Catalog" => {
                catalog_object = Some((catalog_object.map_or(object_id, |(id, _)| id), object));
            }
            "Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, old_object)) = &pages_object {
                        if let Ok(old_dictionary) = old_object.as_dict() {
                            dictionary.extend(old_dictionary);
                        }
                    }
                    pages_object = Some((
                        pages_object.map_or(object_id, |(id, _)| id),
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            "Page" | "Outlines" | "Outline" => {}
            _ => {
                document.objects.insert(object_id, object);
            }
        }
    }

    let (pages_id, pages_object) =
        pages_object.ok_or_else(|| "PDF 页面根节点不存在，无法导出。".to_string())?;
    for (object_id, object) in &documents_pages {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_id);
            document
                .objects
                .insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    let (catalog_id, catalog_object) =
        catalog_object.ok_or_else(|| "PDF Catalog 不存在，无法导出。".to_string())?;
    if let Ok(dictionary) = pages_object.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Count", documents_pages.len() as u32);
        dictionary.set(
            "Kids",
            documents_pages
                .into_keys()
                .map(Object::Reference)
                .collect::<Vec<_>>(),
        );
        document
            .objects
            .insert(pages_id, Object::Dictionary(dictionary));
    }
    if let Ok(dictionary) = catalog_object.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_id);
        dictionary.remove(b"Outlines");
        dictionary.remove(b"PageMode");
        document
            .objects
            .insert(catalog_id, Object::Dictionary(dictionary));
    } else {
        document.objects.insert(
            catalog_id,
            Object::Dictionary(dictionary! {
                "Type" => "Catalog",
                "Pages" => pages_id,
            }),
        );
    }
    document.trailer.set("Root", catalog_id);
    document.max_id = document.objects.len() as u32;
    document.renumber_objects();
    Ok(document)
}

fn keep_only_page(doc: &mut Document, page_number: u32) -> Result<(), String> {
    let pages = doc.get_pages();
    if !pages.contains_key(&page_number) {
        return Err(format!("PDF 中不存在第 {page_number} 页。"));
    }
    let delete_pages = pages
        .keys()
        .copied()
        .filter(|page| *page != page_number)
        .collect::<Vec<_>>();
    doc.delete_pages(&delete_pages);
    Ok(())
}
