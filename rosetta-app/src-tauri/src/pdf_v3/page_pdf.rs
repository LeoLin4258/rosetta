use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use lopdf::{Dictionary, Document, Object, ObjectId};

use super::{
    page_context::{PdfPageContextError, PdfPageObjectContext},
    page_index::PdfIndexedPage,
    source_object::{PdfObjectView, PdfSourceObjectError},
};

const MAX_MATERIALIZED_OBJECTS: usize = 65_536;
const MAX_OBJECT_DEPTH: usize = 128;

#[derive(Debug)]
pub(crate) enum PdfSinglePageError {
    SourceObject(PdfSourceObjectError),
    PageContext(PdfPageContextError),
    InvalidPageObject(ObjectId),
    InvalidAncestor(ObjectId),
    ObjectLimitExceeded,
    ObjectDepthExceeded,
    Serialization(String),
    InvalidOutput(&'static str),
}

impl fmt::Display for PdfSinglePageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceObject(error) => error.fmt(formatter),
            Self::PageContext(error) => error.fmt(formatter),
            Self::InvalidPageObject((number, generation)) => write!(
                formatter,
                "PDF selected page object {number} {generation} is invalid"
            ),
            Self::InvalidAncestor((number, generation)) => write!(
                formatter,
                "PDF selected page ancestor {number} {generation} is invalid"
            ),
            Self::ObjectLimitExceeded => write!(
                formatter,
                "PDF selected page exceeds the {MAX_MATERIALIZED_OBJECTS}-object limit"
            ),
            Self::ObjectDepthExceeded => write!(
                formatter,
                "PDF selected page exceeds the {MAX_OBJECT_DEPTH}-level object depth limit"
            ),
            Self::Serialization(message) => formatter.write_str(message),
            Self::InvalidOutput(reason) => {
                write!(
                    formatter,
                    "materialized single-page PDF is invalid: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for PdfSinglePageError {}

impl From<PdfSourceObjectError> for PdfSinglePageError {
    fn from(value: PdfSourceObjectError) -> Self {
        Self::SourceObject(value)
    }
}

impl From<PdfPageContextError> for PdfSinglePageError {
    fn from(value: PdfPageContextError) -> Self {
        Self::PageContext(value)
    }
}

pub(crate) fn serialize_single_page_pdf_from_view(
    objects: &dyn PdfObjectView,
    indexed_page: &PdfIndexedPage,
) -> Result<Vec<u8>, PdfSinglePageError> {
    let catalog_id = objects
        .trailer()
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| PdfSinglePageError::InvalidOutput("trailer root"))?;
    let catalog = objects
        .object(catalog_id)?
        .as_dict()
        .cloned()
        .map_err(|_| PdfSinglePageError::InvalidOutput("catalog"))?;
    let pages_id = catalog
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|_| PdfSinglePageError::InvalidOutput("page tree root"))?;
    let page_id = indexed_page.page_id();
    if page_id == catalog_id || page_id == pages_id || catalog_id == pages_id {
        return Err(PdfSinglePageError::InvalidPageObject(page_id));
    }

    let context = PdfPageObjectContext::resolve(objects, indexed_page)?;
    let mut page = context.page_dictionary().clone();
    page.remove(b"Parent");
    page.remove(b"AA");
    page.set("Resources", Object::Dictionary(context.resources().clone()));
    materialize_inherited_page_attributes(objects, indexed_page, &mut page)?;

    let structural_ids = BTreeSet::from([catalog_id, pages_id]);
    let mut materializer = PageObjectMaterializer {
        objects,
        selected_page_id: page_id,
        structural_ids,
        active: BTreeSet::from([page_id]),
        materialized: BTreeMap::new(),
    };
    let Object::Dictionary(mut page) =
        materializer.materialize_value(Object::Dictionary(page), 0)?
    else {
        return Err(PdfSinglePageError::InvalidPageObject(page_id));
    };
    materializer.active.remove(&page_id);
    page.set("Parent", Object::Reference(pages_id));
    materializer
        .materialized
        .insert(page_id, Object::Dictionary(page));

    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
    pages.set("Count", Object::Integer(1));
    materializer
        .materialized
        .insert(pages_id, Object::Dictionary(pages));

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    materializer
        .materialized
        .insert(catalog_id, Object::Dictionary(catalog));

    let mut document = Document::with_version("1.7");
    document.objects = materializer.materialized;
    document.max_id = document
        .objects
        .keys()
        .map(|(number, _)| *number)
        .max()
        .unwrap_or(0);
    document.trailer.set("Root", Object::Reference(catalog_id));
    document.renumber_objects();
    document.compress();

    let mut bytes = Vec::new();
    document.save_to(&mut bytes).map_err(|error| {
        PdfSinglePageError::Serialization(format!(
            "failed to serialize materialized single-page PDF: {error}"
        ))
    })?;
    if !bytes.starts_with(b"%PDF-") {
        return Err(PdfSinglePageError::InvalidOutput("signature"));
    }
    let validated = Document::load_mem(&bytes).map_err(|error| {
        PdfSinglePageError::Serialization(format!(
            "failed to validate materialized single-page PDF: {error}"
        ))
    })?;
    if validated.get_pages().len() != 1 {
        return Err(PdfSinglePageError::InvalidOutput("page count"));
    }
    Ok(bytes)
}

fn materialize_inherited_page_attributes(
    objects: &dyn PdfObjectView,
    indexed_page: &PdfIndexedPage,
    page: &mut Dictionary,
) -> Result<(), PdfSinglePageError> {
    for key in [
        b"MediaBox".as_slice(),
        b"CropBox".as_slice(),
        b"Rotate".as_slice(),
    ] {
        if page.has(key) {
            continue;
        }
        for ancestor_id in indexed_page.ancestor_page_tree_ids().iter().rev() {
            let ancestor = objects
                .object(*ancestor_id)?
                .as_dict()
                .cloned()
                .map_err(|_| PdfSinglePageError::InvalidAncestor(*ancestor_id))?;
            if let Ok(value) = ancestor.get(key) {
                page.set(key, value.clone());
                break;
            }
        }
    }
    if !page.has(b"MediaBox") {
        return Err(PdfSinglePageError::InvalidOutput("MediaBox"));
    }
    Ok(())
}

struct PageObjectMaterializer<'a> {
    objects: &'a dyn PdfObjectView,
    selected_page_id: ObjectId,
    structural_ids: BTreeSet<ObjectId>,
    active: BTreeSet<ObjectId>,
    materialized: BTreeMap<ObjectId, Object>,
}

impl PageObjectMaterializer<'_> {
    fn materialize_value(
        &mut self,
        value: Object,
        depth: usize,
    ) -> Result<Object, PdfSinglePageError> {
        if depth > MAX_OBJECT_DEPTH {
            return Err(PdfSinglePageError::ObjectDepthExceeded);
        }
        match value {
            Object::Array(values) => values
                .into_iter()
                .map(|value| self.materialize_value(value, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Object::Array),
            Object::Dictionary(dictionary) => self
                .materialize_dictionary(dictionary, depth + 1)
                .map(Object::Dictionary),
            Object::Stream(mut stream) => {
                stream.dict = self.materialize_dictionary(stream.dict, depth + 1)?;
                Ok(Object::Stream(stream))
            }
            Object::Reference(object_id) => self.materialize_reference(object_id, depth + 1),
            value => Ok(value),
        }
    }

    fn materialize_dictionary(
        &mut self,
        dictionary: Dictionary,
        depth: usize,
    ) -> Result<Dictionary, PdfSinglePageError> {
        let is_annotation = dictionary
            .get(b"Type")
            .ok()
            .and_then(|value| value.as_name().ok())
            .is_some_and(|kind| kind == b"Annot");
        let mut materialized = Dictionary::new();
        for (key, value) in dictionary.iter() {
            if key.as_slice() == b"AA"
                || key.as_slice() == b"OpenAction"
                || (is_annotation && matches!(key.as_slice(), b"A" | b"Dest"))
            {
                continue;
            }
            materialized.set(
                key.clone(),
                self.materialize_value(value.clone(), depth + 1)?,
            );
        }
        Ok(materialized)
    }

    fn materialize_reference(
        &mut self,
        object_id: ObjectId,
        depth: usize,
    ) -> Result<Object, PdfSinglePageError> {
        if object_id == self.selected_page_id
            || self.active.contains(&object_id)
            || self.materialized.contains_key(&object_id)
        {
            return Ok(Object::Reference(object_id));
        }
        if self.structural_ids.contains(&object_id) {
            return Ok(Object::Null);
        }
        if self.materialized.len().saturating_add(self.active.len()) >= MAX_MATERIALIZED_OBJECTS {
            return Err(PdfSinglePageError::ObjectLimitExceeded);
        }
        let object = self.objects.object(object_id)?;
        if is_page_tree_structure(&object) {
            return Ok(Object::Null);
        }
        self.active.insert(object_id);
        let materialized = self.materialize_value(object, depth + 1)?;
        self.active.remove(&object_id);
        self.materialized.insert(object_id, materialized);
        Ok(Object::Reference(object_id))
    }
}

fn is_page_tree_structure(object: &Object) -> bool {
    let dictionary = match object {
        Object::Dictionary(dictionary) => Some(dictionary),
        Object::Stream(stream) => Some(&stream.dict),
        _ => None,
    };
    dictionary
        .and_then(|dictionary| dictionary.get(b"Type").ok())
        .and_then(|value| value.as_name().ok())
        .is_some_and(|kind| matches!(kind, b"Page" | b"Pages" | b"Catalog"))
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use lopdf::{Dictionary, Object};
    use pdfium_render::prelude::{PdfRenderConfig, Pixels};

    use super::serialize_single_page_pdf_from_view;
    use crate::{
        pdf_v3::{page_index::PdfPageIndex, source_object::PdfSourceObjectStore},
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    #[test]
    fn lazy_multi_page_materialization_is_pixel_exact_for_the_selected_page() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("2305.13048v2.pdf");
        let source = PdfSourceObjectStore::open(&source_path).expect("lazy source");
        let index = PdfPageIndex::resolve_page(&source, 2).expect("page index");
        let output =
            serialize_single_page_pdf_from_view(&source, index.page(2).expect("selected page"))
                .expect("single-page PDF");

        let pdfium = shared_pdfium();
        let source_document = pdfium
            .load_pdf_from_file(&source_path, None)
            .expect("source PDFium document");
        let output_document = pdfium
            .load_pdf_from_byte_slice(&output, None)
            .expect("output PDFium document");
        assert!(source_document.pages().len() > 2);
        assert_eq!(output_document.pages().len(), 1);

        let config = PdfRenderConfig::new().set_target_width(1_000 as Pixels);
        let source_image = source_document
            .pages()
            .get(1)
            .expect("source page 2")
            .render_with_config(&config)
            .expect("source render")
            .as_image()
            .expect("source image")
            .to_rgba8();
        let output_image = output_document
            .pages()
            .get(0)
            .expect("output page")
            .render_with_config(&config)
            .expect("output render")
            .as_image()
            .expect("output image")
            .to_rgba8();
        assert_eq!(source_image.dimensions(), output_image.dimensions());
        assert_eq!(source_image.as_raw(), output_image.as_raw());

        let output_objects = lopdf::Document::load_mem(&output).expect("materialized output");
        for object in output_objects.objects.values() {
            assert_no_document_actions(object);
        }

        let stats = source.cache_stats().expect("cache stats");
        assert!(stats.resident_bytes <= 16 * 1024 * 1024);
        assert!(stats.resident_entries <= 512);
    }

    fn assert_no_document_actions(object: &Object) {
        match object {
            Object::Array(values) => values.iter().for_each(assert_no_document_actions),
            Object::Dictionary(dictionary) => assert_safe_dictionary(dictionary),
            Object::Stream(stream) => assert_safe_dictionary(&stream.dict),
            _ => {}
        }
    }

    fn assert_safe_dictionary(dictionary: &Dictionary) {
        assert!(!dictionary.has(b"AA"));
        assert!(!dictionary.has(b"OpenAction"));
        let is_annotation = dictionary
            .get(b"Type")
            .ok()
            .and_then(|value| value.as_name().ok())
            .is_some_and(|kind| kind == b"Annot");
        if is_annotation {
            assert!(!dictionary.has(b"A"));
            assert!(!dictionary.has(b"Dest"));
        }
        for (_, value) in dictionary.iter() {
            assert_no_document_actions(value);
        }
    }
}
