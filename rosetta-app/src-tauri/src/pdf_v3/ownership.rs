use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use lopdf::{Dictionary, Object, ObjectId};

use super::{
    page_context::{PdfPageContextError, PdfPageObjectContext, PdfResourceContext},
    source_object::{PdfObjectView, PdfSourceObjectError},
};

const MAX_PAGE_TREE_DEPTH: usize = 64;
const MAX_FORM_RESOURCE_DEPTH: usize = 32;
const MAX_FORM_RESOURCE_VISITS_PER_PAGE: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PdfStreamOwnership {
    Unreferenced,
    UniqueToPage(u32),
    SharedAcrossPages,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PdfStreamOwnershipIndex {
    page_count: u32,
    ownership_by_stream: BTreeMap<ObjectId, PdfStreamOwnership>,
}

impl PdfStreamOwnershipIndex {
    pub(crate) fn resolve(
        objects: &dyn PdfObjectView,
        targets: &BTreeSet<ObjectId>,
    ) -> Result<Self, PdfStreamOwnershipError> {
        let mut ownership_by_stream = targets
            .iter()
            .copied()
            .map(|stream_id| (stream_id, PdfStreamOwnership::Unreferenced))
            .collect::<BTreeMap<_, _>>();
        if targets.is_empty() {
            return Ok(Self {
                page_count: page_tree_root(objects)?.1,
                ownership_by_stream,
            });
        }

        let mut form_targets = BTreeSet::new();
        for stream_id in targets {
            let stream = objects
                .object(*stream_id)?
                .as_stream()
                .cloned()
                .map_err(|_| PdfStreamOwnershipError::TargetNotStream(*stream_id))?;
            if stream
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|subtype| subtype == b"Form")
            {
                form_targets.insert(*stream_id);
            }
        }

        let (root_id, page_count) = page_tree_root(objects)?;
        let mut traversal = OwnershipTraversal {
            objects,
            targets,
            form_targets: &form_targets,
            ownership_by_stream: &mut ownership_by_stream,
            next_page_number: 1,
            active_page_tree_nodes: BTreeSet::new(),
            visited_page_tree_nodes: BTreeSet::new(),
        };
        let actual_page_count = traversal.visit_pages_node(root_id, &[], 0)?;
        if actual_page_count != page_count {
            return Err(PdfStreamOwnershipError::PageTreeCountMismatch {
                object_id: root_id,
                declared: page_count,
                actual: actual_page_count,
            });
        }

        Ok(Self {
            page_count,
            ownership_by_stream,
        })
    }

    pub(crate) fn page_count(&self) -> u32 {
        self.page_count
    }

    pub(crate) fn ownership(
        &self,
        stream_id: ObjectId,
    ) -> Result<PdfStreamOwnership, PdfStreamOwnershipError> {
        self.ownership_by_stream
            .get(&stream_id)
            .copied()
            .ok_or(PdfStreamOwnershipError::TargetNotIndexed(stream_id))
    }

    pub(crate) fn target_count(&self) -> usize {
        self.ownership_by_stream.len()
    }
}

#[derive(Debug)]
pub(crate) enum PdfStreamOwnershipError {
    SourceObject(PdfSourceObjectError),
    PageContext(PdfPageContextError),
    TrailerRootInvalid,
    CatalogNotDictionary(ObjectId),
    CatalogTypeInvalid(ObjectId),
    CatalogPagesInvalid(ObjectId),
    PageTreeObjectNotDictionary(ObjectId),
    PageTreeNodeTypeInvalid(ObjectId),
    PageTreeCountInvalid(ObjectId),
    PageTreeKidsInvalid(ObjectId),
    PageTreeKidInvalid(ObjectId),
    PageTreeCycle(ObjectId),
    PageTreeNodeRepeated(ObjectId),
    PageTreeDepthExceeded(ObjectId),
    PageTreeCountMismatch {
        object_id: ObjectId,
        declared: u32,
        actual: u32,
    },
    PageNumberOverflow,
    PageContentsInvalid(ObjectId),
    TargetNotStream(ObjectId),
    TargetNotIndexed(ObjectId),
    DirectFormXObject,
    FormResourceCycle(ObjectId),
    FormResourceDepthExceeded(ObjectId),
    FormResourceVisitLimitExceeded(u32),
}

impl fmt::Display for PdfStreamOwnershipError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceObject(error) => error.fmt(formatter),
            Self::PageContext(error) => error.fmt(formatter),
            Self::TrailerRootInvalid => formatter.write_str("PDF trailer /Root is invalid"),
            Self::CatalogNotDictionary(id) => write!(
                formatter,
                "PDF catalog object {} {} is not a dictionary",
                id.0, id.1
            ),
            Self::CatalogTypeInvalid(id) => write!(
                formatter,
                "PDF catalog object {} {} is not /Type /Catalog",
                id.0, id.1
            ),
            Self::CatalogPagesInvalid(id) => write!(
                formatter,
                "PDF catalog object {} {} has an invalid /Pages reference",
                id.0, id.1
            ),
            Self::PageTreeObjectNotDictionary(id) => write!(
                formatter,
                "PDF page-tree object {} {} is not a dictionary",
                id.0, id.1
            ),
            Self::PageTreeNodeTypeInvalid(id) => write!(
                formatter,
                "PDF page-tree object {} {} has an invalid /Type",
                id.0, id.1
            ),
            Self::PageTreeCountInvalid(id) => write!(
                formatter,
                "PDF /Pages object {} {} has an invalid /Count",
                id.0, id.1
            ),
            Self::PageTreeKidsInvalid(id) => write!(
                formatter,
                "PDF /Pages object {} {} has an invalid /Kids array",
                id.0, id.1
            ),
            Self::PageTreeKidInvalid(id) => write!(
                formatter,
                "PDF /Pages object {} {} has a non-reference kid",
                id.0, id.1
            ),
            Self::PageTreeCycle(id) => write!(
                formatter,
                "PDF page tree contains a cycle at object {} {}",
                id.0, id.1
            ),
            Self::PageTreeNodeRepeated(id) => write!(
                formatter,
                "PDF /Pages object {} {} has multiple owners",
                id.0, id.1
            ),
            Self::PageTreeDepthExceeded(id) => write!(
                formatter,
                "PDF page tree exceeds {MAX_PAGE_TREE_DEPTH} levels at object {} {}",
                id.0, id.1
            ),
            Self::PageTreeCountMismatch {
                object_id,
                declared,
                actual,
            } => write!(
                formatter,
                "PDF /Pages object {} {} declares {declared} pages but contains {actual}",
                object_id.0, object_id.1
            ),
            Self::PageNumberOverflow => formatter.write_str("PDF page number overflow"),
            Self::PageContentsInvalid(id) => write!(
                formatter,
                "PDF page object {} {} has invalid /Contents",
                id.0, id.1
            ),
            Self::TargetNotStream(id) => write!(
                formatter,
                "PDF ownership target {} {} is not a stream",
                id.0, id.1
            ),
            Self::TargetNotIndexed(id) => write!(
                formatter,
                "PDF stream {} {} is not present in the ownership index",
                id.0, id.1
            ),
            Self::DirectFormXObject => formatter.write_str(
                "direct Form XObjects cannot establish stable cross-page ownership",
            ),
            Self::FormResourceCycle(id) => write!(
                formatter,
                "PDF Form resource graph contains a cycle at object {} {}",
                id.0, id.1
            ),
            Self::FormResourceDepthExceeded(id) => write!(
                formatter,
                "PDF Form resource graph exceeds {MAX_FORM_RESOURCE_DEPTH} levels at object {} {}",
                id.0, id.1
            ),
            Self::FormResourceVisitLimitExceeded(page) => write!(
                formatter,
                "PDF page {page} exceeds the {MAX_FORM_RESOURCE_VISITS_PER_PAGE} Form resource visit limit"
            ),
        }
    }
}

impl std::error::Error for PdfStreamOwnershipError {}

impl From<PdfSourceObjectError> for PdfStreamOwnershipError {
    fn from(value: PdfSourceObjectError) -> Self {
        Self::SourceObject(value)
    }
}

impl From<PdfPageContextError> for PdfStreamOwnershipError {
    fn from(value: PdfPageContextError) -> Self {
        Self::PageContext(value)
    }
}

struct OwnershipTraversal<'a> {
    objects: &'a dyn PdfObjectView,
    targets: &'a BTreeSet<ObjectId>,
    form_targets: &'a BTreeSet<ObjectId>,
    ownership_by_stream: &'a mut BTreeMap<ObjectId, PdfStreamOwnership>,
    next_page_number: u32,
    active_page_tree_nodes: BTreeSet<ObjectId>,
    visited_page_tree_nodes: BTreeSet<ObjectId>,
}

impl OwnershipTraversal<'_> {
    fn visit_pages_node(
        &mut self,
        node_id: ObjectId,
        ancestors: &[ObjectId],
        depth: usize,
    ) -> Result<u32, PdfStreamOwnershipError> {
        if depth >= MAX_PAGE_TREE_DEPTH {
            return Err(PdfStreamOwnershipError::PageTreeDepthExceeded(node_id));
        }
        if !self.active_page_tree_nodes.insert(node_id) {
            return Err(PdfStreamOwnershipError::PageTreeCycle(node_id));
        }
        if !self.visited_page_tree_nodes.insert(node_id) {
            return Err(PdfStreamOwnershipError::PageTreeNodeRepeated(node_id));
        }
        let result = self.visit_pages_node_inner(node_id, ancestors, depth);
        self.active_page_tree_nodes.remove(&node_id);
        result
    }

    fn visit_pages_node_inner(
        &mut self,
        node_id: ObjectId,
        ancestors: &[ObjectId],
        depth: usize,
    ) -> Result<u32, PdfStreamOwnershipError> {
        let node = object_dictionary(self.objects, node_id)?;
        require_type(&node, node_id, b"Pages")?;
        let declared_count = page_tree_count(&node, node_id)?;
        let kids = node
            .get(b"Kids")
            .and_then(Object::as_array)
            .map_err(|_| PdfStreamOwnershipError::PageTreeKidsInvalid(node_id))?;
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(node_id);
        let mut actual_count = 0u32;

        for kid in kids {
            let kid_id = kid
                .as_reference()
                .map_err(|_| PdfStreamOwnershipError::PageTreeKidInvalid(node_id))?;
            let dictionary = object_dictionary(self.objects, kid_id)?;
            let kid_type = dictionary
                .get(b"Type")
                .and_then(Object::as_name)
                .map_err(|_| PdfStreamOwnershipError::PageTreeNodeTypeInvalid(kid_id))?;
            let child_count = match kid_type {
                b"Page" => {
                    self.visit_page(kid_id, &dictionary, &child_ancestors)?;
                    1
                }
                b"Pages" => self.visit_pages_node(kid_id, &child_ancestors, depth + 1)?,
                _ => return Err(PdfStreamOwnershipError::PageTreeNodeTypeInvalid(kid_id)),
            };
            actual_count = actual_count
                .checked_add(child_count)
                .ok_or(PdfStreamOwnershipError::PageNumberOverflow)?;
        }
        if actual_count != declared_count {
            return Err(PdfStreamOwnershipError::PageTreeCountMismatch {
                object_id: node_id,
                declared: declared_count,
                actual: actual_count,
            });
        }
        Ok(actual_count)
    }

    fn visit_page(
        &mut self,
        page_id: ObjectId,
        page: &Dictionary,
        ancestor_page_tree_ids: &[ObjectId],
    ) -> Result<(), PdfStreamOwnershipError> {
        let page_number = self.next_page_number;
        self.next_page_number = self
            .next_page_number
            .checked_add(1)
            .ok_or(PdfStreamOwnershipError::PageNumberOverflow)?;
        for stream_id in page_content_stream_ids(page, page_id)? {
            if self.targets.contains(&stream_id) {
                record_page(self.ownership_by_stream, stream_id, page_number);
            }
        }
        if self.form_targets.is_empty() {
            return Ok(());
        }

        let context = PdfPageObjectContext::resolve_page_tree_entry(
            self.objects,
            page_id,
            ancestor_page_tree_ids,
        )?;
        let resource_names = context.resource_context().xobject_names(self.objects)?;
        let mut active_forms = BTreeSet::new();
        let mut visit_count = 0usize;
        self.visit_form_resources(
            context.resource_context(),
            resource_names,
            page_number,
            0,
            &mut active_forms,
            &mut visit_count,
        )
    }

    fn visit_form_resources(
        &mut self,
        resources: &PdfResourceContext,
        resource_names: Vec<Vec<u8>>,
        page_number: u32,
        depth: usize,
        active_forms: &mut BTreeSet<ObjectId>,
        visit_count: &mut usize,
    ) -> Result<(), PdfStreamOwnershipError> {
        for name in resource_names {
            let Some(resolved) = resources.resolve_xobject(self.objects, &name)? else {
                continue;
            };
            let stream = resolved.stream();
            if !stream
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|subtype| subtype == b"Form")
            {
                continue;
            }
            let Some(stream_id) = resolved.object_id() else {
                return Err(PdfStreamOwnershipError::DirectFormXObject);
            };
            *visit_count = visit_count.saturating_add(1);
            if *visit_count > MAX_FORM_RESOURCE_VISITS_PER_PAGE {
                return Err(PdfStreamOwnershipError::FormResourceVisitLimitExceeded(
                    page_number,
                ));
            }
            if self.form_targets.contains(&stream_id) {
                record_page(self.ownership_by_stream, stream_id, page_number);
            }
            if depth >= MAX_FORM_RESOURCE_DEPTH {
                return Err(PdfStreamOwnershipError::FormResourceDepthExceeded(
                    stream_id,
                ));
            }
            if !active_forms.insert(stream_id) {
                return Err(PdfStreamOwnershipError::FormResourceCycle(stream_id));
            }
            let child_names =
                PdfResourceContext::invoked_form_xobject_names(self.objects, stream_id, stream)?;
            let child_resources = resources.invoked_form(self.objects, stream_id, stream)?;
            let result = self.visit_form_resources(
                &child_resources,
                child_names,
                page_number,
                depth + 1,
                active_forms,
                visit_count,
            );
            active_forms.remove(&stream_id);
            result?;
        }
        Ok(())
    }
}

fn record_page(
    ownership_by_stream: &mut BTreeMap<ObjectId, PdfStreamOwnership>,
    stream_id: ObjectId,
    page_number: u32,
) {
    let Some(ownership) = ownership_by_stream.get_mut(&stream_id) else {
        return;
    };
    *ownership = match *ownership {
        PdfStreamOwnership::Unreferenced => PdfStreamOwnership::UniqueToPage(page_number),
        PdfStreamOwnership::UniqueToPage(existing) if existing == page_number => *ownership,
        PdfStreamOwnership::UniqueToPage(_) => PdfStreamOwnership::SharedAcrossPages,
        PdfStreamOwnership::SharedAcrossPages => PdfStreamOwnership::SharedAcrossPages,
    };
}

fn page_tree_root(objects: &dyn PdfObjectView) -> Result<(ObjectId, u32), PdfStreamOwnershipError> {
    let catalog_id = objects
        .trailer()
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| PdfStreamOwnershipError::TrailerRootInvalid)?;
    let catalog = objects
        .object(catalog_id)?
        .as_dict()
        .cloned()
        .map_err(|_| PdfStreamOwnershipError::CatalogNotDictionary(catalog_id))?;
    if !catalog
        .get(b"Type")
        .and_then(Object::as_name)
        .is_ok_and(|value| value == b"Catalog")
    {
        return Err(PdfStreamOwnershipError::CatalogTypeInvalid(catalog_id));
    }
    let pages_id = catalog
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|_| PdfStreamOwnershipError::CatalogPagesInvalid(catalog_id))?;
    let pages = object_dictionary(objects, pages_id)?;
    require_type(&pages, pages_id, b"Pages")?;
    Ok((pages_id, page_tree_count(&pages, pages_id)?))
}

fn object_dictionary(
    objects: &dyn PdfObjectView,
    object_id: ObjectId,
) -> Result<Dictionary, PdfStreamOwnershipError> {
    objects
        .object(object_id)?
        .as_dict()
        .cloned()
        .map_err(|_| PdfStreamOwnershipError::PageTreeObjectNotDictionary(object_id))
}

fn require_type(
    dictionary: &Dictionary,
    object_id: ObjectId,
    expected: &[u8],
) -> Result<(), PdfStreamOwnershipError> {
    if dictionary
        .get(b"Type")
        .and_then(Object::as_name)
        .is_ok_and(|actual| actual == expected)
    {
        Ok(())
    } else {
        Err(PdfStreamOwnershipError::PageTreeNodeTypeInvalid(object_id))
    }
}

fn page_tree_count(
    dictionary: &Dictionary,
    object_id: ObjectId,
) -> Result<u32, PdfStreamOwnershipError> {
    dictionary
        .get(b"Count")
        .and_then(Object::as_i64)
        .ok()
        .and_then(|count| u32::try_from(count).ok())
        .ok_or(PdfStreamOwnershipError::PageTreeCountInvalid(object_id))
}

fn page_content_stream_ids(
    dictionary: &Dictionary,
    page_id: ObjectId,
) -> Result<Vec<ObjectId>, PdfStreamOwnershipError> {
    let Ok(contents) = dictionary.get(b"Contents") else {
        return Ok(Vec::new());
    };
    match contents {
        Object::Reference(id) => Ok(vec![*id]),
        Object::Array(contents) => contents
            .iter()
            .map(|content| {
                content
                    .as_reference()
                    .map_err(|_| PdfStreamOwnershipError::PageContentsInvalid(page_id))
            })
            .collect(),
        Object::Null => Ok(Vec::new()),
        _ => Err(PdfStreamOwnershipError::PageContentsInvalid(page_id)),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::BTreeSet};

    use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

    use super::{PdfStreamOwnership, PdfStreamOwnershipError, PdfStreamOwnershipIndex};
    use crate::pdf_v3::source_object::{PdfObjectView, PdfSourceObjectError};

    fn stream(dictionary: Dictionary) -> Object {
        Object::Stream(Stream::new(dictionary, Vec::new()))
    }

    fn ownership_document() -> Document {
        let mut document = Document::with_version("1.7");
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference((2, 0)));
        document.objects.insert((1, 0), Object::Dictionary(catalog));

        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Count", Object::Integer(3));
        pages.set(
            "Kids",
            Object::Array(
                [(3, 0), (4, 0), (5, 0)]
                    .into_iter()
                    .map(Object::Reference)
                    .collect(),
            ),
        );
        document.objects.insert((2, 0), Object::Dictionary(pages));

        let mut page_one_resources = Dictionary::new();
        let mut page_one_xobjects = Dictionary::new();
        page_one_xobjects.set("Outer", Object::Reference((30, 0)));
        page_one_resources.set("XObject", Object::Dictionary(page_one_xobjects));
        let mut page_two_resources = Dictionary::new();
        let mut page_two_xobjects = Dictionary::new();
        page_two_xobjects.set("Target", Object::Reference((31, 0)));
        page_two_resources.set("XObject", Object::Dictionary(page_two_xobjects));

        for (page_id, content_id, resources) in [
            ((3, 0), (20, 0), Some(page_one_resources)),
            ((4, 0), (21, 0), Some(page_two_resources)),
            ((5, 0), (20, 0), None),
        ] {
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set("Parent", Object::Reference((2, 0)));
            page.set("Contents", Object::Reference(content_id));
            if let Some(resources) = resources {
                page.set("Resources", Object::Dictionary(resources));
            }
            document.objects.insert(page_id, Object::Dictionary(page));
        }

        for content_id in [(20, 0), (21, 0)] {
            document
                .objects
                .insert(content_id, stream(Dictionary::new()));
        }
        let mut outer = Dictionary::new();
        outer.set("Type", Object::Name(b"XObject".to_vec()));
        outer.set("Subtype", Object::Name(b"Form".to_vec()));
        let mut outer_resources = Dictionary::new();
        let mut outer_xobjects = Dictionary::new();
        outer_xobjects.set("Nested", Object::Reference((31, 0)));
        outer_resources.set("XObject", Object::Dictionary(outer_xobjects));
        outer.set("Resources", Object::Dictionary(outer_resources));
        document.objects.insert((30, 0), stream(outer));

        let mut nested = Dictionary::new();
        nested.set("Type", Object::Name(b"XObject".to_vec()));
        nested.set("Subtype", Object::Name(b"Form".to_vec()));
        document.objects.insert((31, 0), stream(nested));
        document.trailer.set("Root", Object::Reference((1, 0)));
        document.max_id = 31;
        document
    }

    #[test]
    fn indexes_direct_and_nested_form_ownership_without_retaining_pages() {
        let document = ownership_document();
        let targets = BTreeSet::from([(20, 0), (21, 0), (30, 0), (31, 0)]);
        let index = PdfStreamOwnershipIndex::resolve(&document, &targets).expect("ownership index");

        assert_eq!(index.page_count(), 3);
        assert_eq!(index.target_count(), 4);
        assert_eq!(
            index.ownership((20, 0)).expect("shared content"),
            PdfStreamOwnership::SharedAcrossPages
        );
        assert_eq!(
            index.ownership((21, 0)).expect("unique content"),
            PdfStreamOwnership::UniqueToPage(2)
        );
        assert_eq!(
            index.ownership((30, 0)).expect("outer Form"),
            PdfStreamOwnership::UniqueToPage(1)
        );
        assert_eq!(
            index.ownership((31, 0)).expect("nested Form"),
            PdfStreamOwnership::SharedAcrossPages
        );
    }

    struct RecordingView<'a> {
        document: &'a Document,
        loaded: RefCell<BTreeSet<ObjectId>>,
    }

    impl PdfObjectView for RecordingView<'_> {
        fn maximum_object_number(&self) -> u32 {
            self.document.max_id
        }

        fn trailer(&self) -> &Dictionary {
            &self.document.trailer
        }

        fn object(&self, object_id: ObjectId) -> Result<Object, PdfSourceObjectError> {
            self.loaded.borrow_mut().insert(object_id);
            self.document.object(object_id)
        }
    }

    #[test]
    fn ordinary_content_targets_do_not_load_unselected_content_or_form_streams() {
        let document = ownership_document();
        let view = RecordingView {
            document: &document,
            loaded: RefCell::new(BTreeSet::new()),
        };
        let index = PdfStreamOwnershipIndex::resolve(&view, &BTreeSet::from([(21, 0)]))
            .expect("ordinary ownership index");

        assert_eq!(
            index.ownership((21, 0)).expect("unique content"),
            PdfStreamOwnership::UniqueToPage(2)
        );
        let loaded = view.loaded.borrow();
        assert!(loaded.contains(&(21, 0)));
        assert!(!loaded.contains(&(20, 0)));
        assert!(!loaded.contains(&(30, 0)));
        assert!(!loaded.contains(&(31, 0)));
    }

    #[test]
    fn rejects_malformed_page_counts_and_form_cycles() {
        let mut wrong_count = ownership_document();
        wrong_count
            .get_object_mut((2, 0))
            .and_then(Object::as_dict_mut)
            .expect("pages")
            .set("Count", Object::Integer(4));
        assert!(matches!(
            PdfStreamOwnershipIndex::resolve(&wrong_count, &BTreeSet::from([(20, 0)])),
            Err(PdfStreamOwnershipError::PageTreeCountMismatch {
                object_id: (2, 0),
                declared: 4,
                actual: 3
            })
        ));

        let mut cyclic = ownership_document();
        let target = cyclic
            .get_object_mut((31, 0))
            .and_then(Object::as_stream_mut)
            .expect("target Form");
        let mut resources = Dictionary::new();
        let mut xobjects = Dictionary::new();
        xobjects.set("Self", Object::Reference((31, 0)));
        resources.set("XObject", Object::Dictionary(xobjects));
        target.dict.set("Resources", Object::Dictionary(resources));
        assert!(matches!(
            PdfStreamOwnershipIndex::resolve(&cyclic, &BTreeSet::from([(31, 0)])),
            Err(PdfStreamOwnershipError::FormResourceCycle((31, 0)))
        ));
    }

    #[test]
    fn thousand_page_scan_retains_only_requested_ownership_states() {
        let mut document = Document::with_version("1.7");
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference((2, 0)));
        document.objects.insert((1, 0), Object::Dictionary(catalog));

        let page_ids = (0u32..1_000)
            .map(|index| (10 + index, 0))
            .collect::<Vec<_>>();
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Count", Object::Integer(1_000));
        pages.set(
            "Kids",
            Object::Array(page_ids.iter().copied().map(Object::Reference).collect()),
        );
        document.objects.insert((2, 0), Object::Dictionary(pages));

        for (index, page_id) in page_ids.into_iter().enumerate() {
            let content_id = match index {
                0 | 999 => (5_000, 0),
                499 => (5_001, 0),
                _ => (10_000 + index as u32, 0),
            };
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set("Parent", Object::Reference((2, 0)));
            page.set("Contents", Object::Reference(content_id));
            document.objects.insert(page_id, Object::Dictionary(page));
        }
        document
            .objects
            .insert((5_000, 0), stream(Dictionary::new()));
        document
            .objects
            .insert((5_001, 0), stream(Dictionary::new()));
        document.trailer.set("Root", Object::Reference((1, 0)));
        document.max_id = 10_999;

        let targets = BTreeSet::from([(5_000, 0), (5_001, 0)]);
        let index = PdfStreamOwnershipIndex::resolve(&document, &targets)
            .expect("thousand-page ownership index");

        assert_eq!(index.page_count(), 1_000);
        assert_eq!(index.target_count(), 2);
        assert_eq!(
            index.ownership((5_000, 0)).expect("shared target"),
            PdfStreamOwnership::SharedAcrossPages
        );
        assert_eq!(
            index.ownership((5_001, 0)).expect("unique target"),
            PdfStreamOwnership::UniqueToPage(500)
        );
    }
}
