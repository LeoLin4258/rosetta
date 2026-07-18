use std::{collections::BTreeSet, fmt};

use lopdf::{Dictionary, Object, ObjectId};

use super::{
    page_index::PdfIndexedPage,
    source_object::{PdfObjectView, PdfSourceObjectError},
};

const MAX_INDIRECT_REFERENCE_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PdfPageObjectContext {
    page_id: ObjectId,
    page_dictionary: Dictionary,
    resources: Dictionary,
}

impl PdfPageObjectContext {
    pub(crate) fn resolve(
        objects: &dyn PdfObjectView,
        indexed_page: &PdfIndexedPage,
    ) -> Result<Self, PdfPageContextError> {
        let page_id = indexed_page.page_id();
        let page_dictionary = object_dictionary(objects, page_id)?;
        if !page_dictionary
            .get(b"Type")
            .and_then(Object::as_name)
            .is_ok_and(|value| value == b"Page")
        {
            return Err(PdfPageContextError::PageTypeInvalid(page_id));
        }

        let mut resource_dictionaries = Vec::new();
        collect_resources(
            objects,
            page_id,
            &page_dictionary,
            &mut resource_dictionaries,
        )?;
        for ancestor_id in indexed_page.ancestor_page_tree_ids().iter().rev() {
            let dictionary = object_dictionary(objects, *ancestor_id)?;
            collect_resources(
                objects,
                *ancestor_id,
                &dictionary,
                &mut resource_dictionaries,
            )?;
        }
        let resources = materialize_resources(objects, &resource_dictionaries)?;

        Ok(Self {
            page_id,
            page_dictionary,
            resources,
        })
    }

    pub(crate) fn page_id(&self) -> ObjectId {
        self.page_id
    }

    pub(crate) fn page_dictionary(&self) -> &Dictionary {
        &self.page_dictionary
    }

    pub(crate) fn resources(&self) -> &Dictionary {
        &self.resources
    }
}

#[derive(Debug)]
pub(crate) enum PdfPageContextError {
    SourceObject(PdfSourceObjectError),
    ObjectNotDictionary(ObjectId),
    PageTypeInvalid(ObjectId),
    ResourcesInvalid(ObjectId),
    ResourceCategoryInvalid {
        object_id: ObjectId,
        category: Vec<u8>,
    },
    ReferenceCycle(ObjectId),
    ReferenceDepthExceeded(ObjectId),
}

impl fmt::Display for PdfPageContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceObject(error) => error.fmt(formatter),
            Self::ObjectNotDictionary(id) => write!(
                formatter,
                "PDF page-context object {} {} is not a dictionary",
                id.0, id.1
            ),
            Self::PageTypeInvalid(id) => write!(
                formatter,
                "PDF selected page object {} {} is not /Type /Page",
                id.0, id.1
            ),
            Self::ResourcesInvalid(id) => write!(
                formatter,
                "PDF page-tree object {} {} has invalid /Resources",
                id.0, id.1
            ),
            Self::ResourceCategoryInvalid {
                object_id,
                category,
            } => write!(
                formatter,
                "PDF resource category /{} on object {} {} is inconsistent",
                String::from_utf8_lossy(category),
                object_id.0,
                object_id.1
            ),
            Self::ReferenceCycle(id) => write!(
                formatter,
                "PDF page resource reference cycle reaches object {} {}",
                id.0, id.1
            ),
            Self::ReferenceDepthExceeded(id) => write!(
                formatter,
                "PDF page resource reference chain exceeds {MAX_INDIRECT_REFERENCE_DEPTH} at object {} {}",
                id.0, id.1
            ),
        }
    }
}

impl std::error::Error for PdfPageContextError {}

impl From<PdfSourceObjectError> for PdfPageContextError {
    fn from(value: PdfSourceObjectError) -> Self {
        Self::SourceObject(value)
    }
}

fn object_dictionary(
    objects: &dyn PdfObjectView,
    object_id: ObjectId,
) -> Result<Dictionary, PdfPageContextError> {
    objects
        .object(object_id)?
        .as_dict()
        .cloned()
        .map_err(|_| PdfPageContextError::ObjectNotDictionary(object_id))
}

fn collect_resources(
    objects: &dyn PdfObjectView,
    owner_id: ObjectId,
    owner: &Dictionary,
    resources: &mut Vec<(ObjectId, Dictionary)>,
) -> Result<(), PdfPageContextError> {
    let Ok(value) = owner.get(b"Resources") else {
        return Ok(());
    };
    if matches!(value, Object::Null) {
        return Ok(());
    }
    let Some(dictionary) = resolve_dictionary(objects, value)? else {
        return Err(PdfPageContextError::ResourcesInvalid(owner_id));
    };
    resources.push((owner_id, dictionary));
    Ok(())
}

fn materialize_resources(
    objects: &dyn PdfObjectView,
    resources: &[(ObjectId, Dictionary)],
) -> Result<Dictionary, PdfPageContextError> {
    let mut keys = BTreeSet::new();
    for (_, dictionary) in resources {
        keys.extend(dictionary.iter().map(|(key, _)| key.clone()));
    }

    let mut materialized = Dictionary::new();
    for key in keys {
        let Some((_, first)) = resources.iter().find_map(|(owner_id, dictionary)| {
            dictionary.get(&key).ok().map(|value| (owner_id, value))
        }) else {
            continue;
        };
        if resolve_dictionary(objects, first)?.is_some() {
            let mut merged = Dictionary::new();
            for (owner_id, dictionary) in resources.iter().rev() {
                let Ok(value) = dictionary.get(&key) else {
                    continue;
                };
                let Some(category) = resolve_dictionary(objects, value)? else {
                    return Err(PdfPageContextError::ResourceCategoryInvalid {
                        object_id: *owner_id,
                        category: key.clone(),
                    });
                };
                for (name, value) in category.iter() {
                    merged.set(name.clone(), value.clone());
                }
            }
            materialized.set(key, Object::Dictionary(merged));
        } else {
            materialized.set(key, first.clone());
        }
    }
    Ok(materialized)
}

fn resolve_dictionary(
    objects: &dyn PdfObjectView,
    value: &Object,
) -> Result<Option<Dictionary>, PdfPageContextError> {
    match value {
        Object::Dictionary(dictionary) => Ok(Some(dictionary.clone())),
        Object::Reference(object_id) => {
            let mut current = *object_id;
            let mut visited = BTreeSet::new();
            for _ in 0..MAX_INDIRECT_REFERENCE_DEPTH {
                if !visited.insert(current) {
                    return Err(PdfPageContextError::ReferenceCycle(current));
                }
                match objects.object(current)? {
                    Object::Dictionary(dictionary) => return Ok(Some(dictionary)),
                    Object::Reference(next) => current = next,
                    _ => return Ok(None),
                }
            }
            Err(PdfPageContextError::ReferenceDepthExceeded(current))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use std::fs;

    use lopdf::{Dictionary, Document, Object};

    use super::{PdfPageContextError, PdfPageObjectContext};
    use crate::pdf_v3::{page_index::PdfPageIndex, page_set::PageSet};

    #[cfg(target_os = "windows")]
    use crate::{
        pdf_v3::{
            font::{stage_translation_fonts_page_context, stage_translation_fonts_page_dictionary},
            source_object::PdfSourceObjectStore,
        },
        rosetta_jobs::formats::pdf::test_helpers::fixture_path,
    };

    fn resource_dictionary(entries: &[(&str, Object)]) -> Dictionary {
        let mut dictionary = Dictionary::new();
        for (name, value) in entries {
            dictionary.set(name.as_bytes().to_vec(), value.clone());
        }
        dictionary
    }

    fn nested_resource_document() -> Document {
        let mut document = Document::with_version("1.7");
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference((2, 0)));
        document.objects.insert((1, 0), Object::Dictionary(catalog));

        let mut root = Dictionary::new();
        root.set("Type", Object::Name(b"Pages".to_vec()));
        root.set("Count", Object::Integer(1));
        root.set("Kids", Object::Array(vec![Object::Reference((3, 0))]));
        root.set("Resources", Object::Reference((10, 0)));
        document.objects.insert((2, 0), Object::Dictionary(root));

        let mut branch = Dictionary::new();
        branch.set("Type", Object::Name(b"Pages".to_vec()));
        branch.set("Parent", Object::Reference((2, 0)));
        branch.set("Count", Object::Integer(1));
        branch.set("Kids", Object::Array(vec![Object::Reference((4, 0))]));
        branch.set(
            "Resources",
            Object::Dictionary(resource_dictionary(&[
                (
                    "Font",
                    Object::Dictionary(resource_dictionary(&[(
                        "BranchFont",
                        Object::Reference((21, 0)),
                    )])),
                ),
                (
                    "ProcSet",
                    Object::Array(vec![Object::Name(b"PDF".to_vec())]),
                ),
            ])),
        );
        document.objects.insert((3, 0), Object::Dictionary(branch));

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference((3, 0)));
        page.set("Contents", Object::Null);
        page.set("Resources", Object::Reference((11, 0)));
        document.objects.insert((4, 0), Object::Dictionary(page));

        document.objects.insert(
            (10, 0),
            Object::Dictionary(resource_dictionary(&[(
                "Font",
                Object::Dictionary(resource_dictionary(&[
                    ("RootFont", Object::Reference((20, 0))),
                    ("SharedFont", Object::Reference((22, 0))),
                ])),
            )])),
        );
        document.objects.insert(
            (11, 0),
            Object::Dictionary(resource_dictionary(&[(
                "Font",
                Object::Dictionary(resource_dictionary(&[(
                    "SharedFont",
                    Object::Reference((23, 0)),
                )])),
            )])),
        );
        document.trailer.set("Root", Object::Reference((1, 0)));
        document.max_id = 23;
        document
    }

    #[test]
    fn materializes_page_and_inherited_resources_with_nearest_precedence() {
        let document = nested_resource_document();
        let selected = PageSet::from_pages([1]).expect("selected page");
        let index = PdfPageIndex::resolve(&document, &selected).expect("page index");
        let context = PdfPageObjectContext::resolve(&document, index.page(1).expect("page"))
            .expect("page context");

        assert_eq!(context.page_id(), (4, 0));
        let fonts = context
            .resources()
            .get(b"Font")
            .and_then(Object::as_dict)
            .expect("materialized fonts");
        assert_eq!(
            fonts.get(b"RootFont").expect("root font"),
            &Object::Reference((20, 0))
        );
        assert_eq!(
            fonts.get(b"BranchFont").expect("branch font"),
            &Object::Reference((21, 0))
        );
        assert_eq!(
            fonts.get(b"SharedFont").expect("page override"),
            &Object::Reference((23, 0))
        );
        assert!(context.resources().get(b"ProcSet").is_ok());
    }

    #[test]
    fn rejects_invalid_selected_page_resources() {
        let mut document = nested_resource_document();
        document
            .get_object_mut((4, 0))
            .and_then(Object::as_dict_mut)
            .expect("page dictionary")
            .set("Resources", Object::Integer(7));
        let index = PdfPageIndex::resolve_page(&document, 1).expect("page index");
        assert!(matches!(
            PdfPageObjectContext::resolve(&document, index.page(1).expect("page")),
            Err(PdfPageContextError::ResourcesInvalid((4, 0)))
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn lazy_real_page_context_matches_legacy_resource_materialization() {
        let source_path = fixture_path("2305.13048v2.pdf");
        let source_objects =
            PdfSourceObjectStore::open(&source_path).expect("lazy source object store");
        let page_index = PdfPageIndex::resolve_page(&source_objects, 1).expect("page index");
        let indexed_page = page_index.page(1).expect("indexed page");
        let context = PdfPageObjectContext::resolve(&source_objects, indexed_page)
            .expect("lazy page context");
        let resource_name = b"RosettaContextProbe";
        let type0_font_id = (999_999, 0);
        let lazy = stage_translation_fonts_page_context(
            &context,
            std::iter::once((resource_name.as_slice(), type0_font_id)),
        )
        .expect("lazy staged page");

        let bytes = fs::read(&source_path).expect("source PDF");
        let document = Document::load_mem(&bytes).expect("legacy source document");
        let legacy = stage_translation_fonts_page_dictionary(
            &document,
            indexed_page.page_id(),
            std::iter::once((resource_name.as_slice(), type0_font_id)),
        )
        .expect("legacy staged page");
        assert_eq!(lazy, legacy);

        let stats = source_objects.cache_stats().expect("lazy cache stats");
        assert!(stats.source_loads <= 16);
        assert!(stats.resident_entries <= 16);
        assert!(stats.resident_bytes <= 16 * 1024 * 1024);
    }
}
