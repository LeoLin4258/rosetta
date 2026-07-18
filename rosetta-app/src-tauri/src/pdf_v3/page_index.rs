use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use lopdf::{Dictionary, Object, ObjectId};

use super::{
    page_set::{PageSet, PageSetError},
    source_object::{PdfObjectView, PdfSourceObjectError},
};

const MAX_PAGE_TREE_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PdfIndexedPage {
    page_number: u32,
    page_id: ObjectId,
    ancestor_page_tree_ids: Vec<ObjectId>,
    content_stream_ids: Vec<ObjectId>,
}

impl PdfIndexedPage {
    pub(crate) fn page_number(&self) -> u32 {
        self.page_number
    }

    pub(crate) fn page_id(&self) -> ObjectId {
        self.page_id
    }

    pub(crate) fn ancestor_page_tree_ids(&self) -> &[ObjectId] {
        &self.ancestor_page_tree_ids
    }

    pub(crate) fn content_stream_ids(&self) -> &[ObjectId] {
        &self.content_stream_ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PdfPageIndex {
    page_count: u32,
    pages: BTreeMap<u32, PdfIndexedPage>,
}

impl PdfPageIndex {
    pub(crate) fn resolve_page(
        objects: &dyn PdfObjectView,
        page_number: u32,
    ) -> Result<Self, PdfPageIndexError> {
        let selected_pages = PageSet::from_pages([page_number])?;
        Self::resolve(objects, &selected_pages)
    }

    pub(crate) fn resolve(
        objects: &dyn PdfObjectView,
        selected_pages: &PageSet,
    ) -> Result<Self, PdfPageIndexError> {
        let catalog_id = objects
            .trailer()
            .get(b"Root")
            .and_then(Object::as_reference)
            .map_err(|_| PdfPageIndexError::TrailerRootInvalid)?;
        let catalog = object_dictionary(objects, catalog_id)?;
        require_node_type(&catalog, catalog_id, b"Catalog", "Catalog")?;
        let pages_root_id = catalog
            .get(b"Pages")
            .and_then(Object::as_reference)
            .map_err(|_| PdfPageIndexError::CatalogPagesInvalid(catalog_id))?;
        let pages_root = object_dictionary(objects, pages_root_id)?;
        require_node_type(&pages_root, pages_root_id, b"Pages", "Pages")?;
        let page_count = page_tree_count(&pages_root, pages_root_id)?;
        if let Some(&page) = selected_pages.pages().last() {
            if page > page_count {
                return Err(PdfPageIndexError::PageOutOfBounds { page, page_count });
            }
        }

        let mut index = Self {
            page_count,
            pages: BTreeMap::new(),
        };
        let Some(&maximum_selected_page) = selected_pages.pages().last() else {
            return Ok(index);
        };
        let mut traversal = PageTreeTraversal {
            objects,
            selected_pages,
            maximum_selected_page,
            active_nodes: BTreeSet::new(),
            visited_nodes: BTreeSet::new(),
        };
        traversal.visit_pages_node(pages_root_id, 1, &[], 0, &mut index.pages)?;
        if let Some(&missing) = selected_pages
            .pages()
            .iter()
            .find(|page| !index.pages.contains_key(page))
        {
            return Err(PdfPageIndexError::SelectedPageMissing(missing));
        }
        Ok(index)
    }

    pub(crate) fn page_count(&self) -> u32 {
        self.page_count
    }

    pub(crate) fn page(&self, page_number: u32) -> Result<&PdfIndexedPage, PdfPageIndexError> {
        if page_number == 0 || page_number > self.page_count {
            return Err(PdfPageIndexError::PageOutOfBounds {
                page: page_number,
                page_count: self.page_count,
            });
        }
        self.pages
            .get(&page_number)
            .ok_or(PdfPageIndexError::PageNotSelected(page_number))
    }

    pub(crate) fn selected_page_count(&self) -> usize {
        self.pages.len()
    }
}

#[derive(Debug)]
pub(crate) enum PdfPageIndexError {
    SourceObject(PdfSourceObjectError),
    PageSet(PageSetError),
    TrailerRootInvalid,
    CatalogPagesInvalid(ObjectId),
    ObjectNotDictionary(ObjectId),
    NodeTypeInvalid {
        object_id: ObjectId,
        expected: &'static str,
    },
    PageTreeCountInvalid(ObjectId),
    PageTreeKidsInvalid(ObjectId),
    PageTreeKidInvalid(ObjectId),
    PageTreeCycle(ObjectId),
    PageTreeNodeRepeated(ObjectId),
    PageTreeDepthExceeded(ObjectId),
    PageNumberOverflow,
    PageOutOfBounds {
        page: u32,
        page_count: u32,
    },
    SelectedPageMissing(u32),
    PageNotSelected(u32),
    PageContentsInvalid(ObjectId),
}

impl fmt::Display for PdfPageIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceObject(error) => error.fmt(formatter),
            Self::PageSet(error) => error.fmt(formatter),
            Self::TrailerRootInvalid => formatter.write_str("PDF trailer /Root is invalid"),
            Self::CatalogPagesInvalid(id) => write!(
                formatter,
                "PDF catalog {} {} has an invalid /Pages reference",
                id.0, id.1
            ),
            Self::ObjectNotDictionary(id) => {
                write!(
                    formatter,
                    "PDF page-tree object {} {} is not a dictionary",
                    id.0, id.1
                )
            }
            Self::NodeTypeInvalid {
                object_id,
                expected,
            } => write!(
                formatter,
                "PDF page-tree object {} {} is not /Type /{expected}",
                object_id.0, object_id.1
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
                "PDF page-tree object {} {} has multiple owners",
                id.0, id.1
            ),
            Self::PageTreeDepthExceeded(id) => write!(
                formatter,
                "PDF page tree exceeds {MAX_PAGE_TREE_DEPTH} levels at object {} {}",
                id.0, id.1
            ),
            Self::PageNumberOverflow => formatter.write_str("PDF page number overflow"),
            Self::PageOutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
            Self::SelectedPageMissing(page) => {
                write!(
                    formatter,
                    "PDF page tree did not resolve selected page {page}"
                )
            }
            Self::PageNotSelected(page) => {
                write!(
                    formatter,
                    "PDF page {page} is not present in this page index"
                )
            }
            Self::PageContentsInvalid(id) => write!(
                formatter,
                "PDF page object {} {} has invalid /Contents",
                id.0, id.1
            ),
        }
    }
}

impl std::error::Error for PdfPageIndexError {}

impl From<PdfSourceObjectError> for PdfPageIndexError {
    fn from(value: PdfSourceObjectError) -> Self {
        Self::SourceObject(value)
    }
}

impl From<PageSetError> for PdfPageIndexError {
    fn from(value: PageSetError) -> Self {
        Self::PageSet(value)
    }
}

struct PageTreeTraversal<'a> {
    objects: &'a dyn PdfObjectView,
    selected_pages: &'a PageSet,
    maximum_selected_page: u32,
    active_nodes: BTreeSet<ObjectId>,
    visited_nodes: BTreeSet<ObjectId>,
}

impl PageTreeTraversal<'_> {
    fn visit_pages_node(
        &mut self,
        node_id: ObjectId,
        first_page: u32,
        ancestors: &[ObjectId],
        depth: usize,
        pages: &mut BTreeMap<u32, PdfIndexedPage>,
    ) -> Result<(), PdfPageIndexError> {
        if depth >= MAX_PAGE_TREE_DEPTH {
            return Err(PdfPageIndexError::PageTreeDepthExceeded(node_id));
        }
        if !self.active_nodes.insert(node_id) {
            return Err(PdfPageIndexError::PageTreeCycle(node_id));
        }
        if !self.visited_nodes.insert(node_id) {
            return Err(PdfPageIndexError::PageTreeNodeRepeated(node_id));
        }
        let result = self.visit_pages_node_inner(node_id, first_page, ancestors, depth, pages);
        self.active_nodes.remove(&node_id);
        result
    }

    fn visit_pages_node_inner(
        &mut self,
        node_id: ObjectId,
        first_page: u32,
        ancestors: &[ObjectId],
        depth: usize,
        pages: &mut BTreeMap<u32, PdfIndexedPage>,
    ) -> Result<(), PdfPageIndexError> {
        let node = object_dictionary(self.objects, node_id)?;
        require_node_type(&node, node_id, b"Pages", "Pages")?;
        let kids = node
            .get(b"Kids")
            .and_then(Object::as_array)
            .map_err(|_| PdfPageIndexError::PageTreeKidsInvalid(node_id))?;
        let mut current_page = first_page;
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(node_id);
        for kid in kids {
            if current_page > self.maximum_selected_page {
                break;
            }
            let kid_id = kid
                .as_reference()
                .map_err(|_| PdfPageIndexError::PageTreeKidInvalid(node_id))?;
            let kid_dictionary = object_dictionary(self.objects, kid_id)?;
            let kid_type = kid_dictionary
                .get(b"Type")
                .and_then(Object::as_name)
                .map_err(|_| PdfPageIndexError::NodeTypeInvalid {
                    object_id: kid_id,
                    expected: "Page or Pages",
                })?;
            let child_page_count = match kid_type {
                b"Page" => 1,
                b"Pages" => page_tree_count(&kid_dictionary, kid_id)?,
                _ => {
                    return Err(PdfPageIndexError::NodeTypeInvalid {
                        object_id: kid_id,
                        expected: "Page or Pages",
                    })
                }
            };
            if child_page_count == 0 {
                continue;
            }
            let last_page = current_page
                .checked_add(child_page_count.saturating_sub(1))
                .ok_or(PdfPageIndexError::PageNumberOverflow)?;
            if selection_intersects(self.selected_pages, current_page, last_page) {
                if kid_type == b"Page" {
                    if !self.visited_nodes.insert(kid_id) {
                        return Err(PdfPageIndexError::PageTreeNodeRepeated(kid_id));
                    }
                    if self.selected_pages.contains(current_page) {
                        let page = PdfIndexedPage {
                            page_number: current_page,
                            page_id: kid_id,
                            ancestor_page_tree_ids: child_ancestors.clone(),
                            content_stream_ids: page_content_stream_ids(&kid_dictionary, kid_id)?,
                        };
                        if pages.insert(current_page, page).is_some() {
                            return Err(PdfPageIndexError::PageTreeNodeRepeated(kid_id));
                        }
                    }
                } else {
                    self.visit_pages_node(
                        kid_id,
                        current_page,
                        &child_ancestors,
                        depth + 1,
                        pages,
                    )?;
                }
            }
            current_page = last_page
                .checked_add(1)
                .ok_or(PdfPageIndexError::PageNumberOverflow)?;
        }
        Ok(())
    }
}

fn object_dictionary(
    objects: &dyn PdfObjectView,
    object_id: ObjectId,
) -> Result<Dictionary, PdfPageIndexError> {
    objects
        .object(object_id)?
        .as_dict()
        .cloned()
        .map_err(|_| PdfPageIndexError::ObjectNotDictionary(object_id))
}

fn require_node_type(
    dictionary: &Dictionary,
    object_id: ObjectId,
    expected: &'static [u8],
    expected_label: &'static str,
) -> Result<(), PdfPageIndexError> {
    if dictionary
        .get(b"Type")
        .and_then(Object::as_name)
        .is_ok_and(|actual| actual == expected)
    {
        return Ok(());
    }
    Err(PdfPageIndexError::NodeTypeInvalid {
        object_id,
        expected: expected_label,
    })
}

fn page_tree_count(dictionary: &Dictionary, object_id: ObjectId) -> Result<u32, PdfPageIndexError> {
    dictionary
        .get(b"Count")
        .and_then(Object::as_i64)
        .ok()
        .and_then(|count| u32::try_from(count).ok())
        .ok_or(PdfPageIndexError::PageTreeCountInvalid(object_id))
}

fn page_content_stream_ids(
    dictionary: &Dictionary,
    page_id: ObjectId,
) -> Result<Vec<ObjectId>, PdfPageIndexError> {
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
                    .map_err(|_| PdfPageIndexError::PageContentsInvalid(page_id))
            })
            .collect(),
        Object::Null => Ok(Vec::new()),
        _ => Err(PdfPageIndexError::PageContentsInvalid(page_id)),
    }
}

fn selection_intersects(selected_pages: &PageSet, first_page: u32, last_page: u32) -> bool {
    let index = selected_pages
        .pages()
        .partition_point(|page| *page < first_page);
    selected_pages
        .pages()
        .get(index)
        .is_some_and(|page| *page <= last_page)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use lopdf::{Dictionary, Document, Object, Stream};

    use super::{PdfPageIndex, PdfPageIndexError};
    use crate::{
        pdf_v3::{page_set::PageSet, source_object::PdfSourceObjectStore},
        rosetta_jobs::formats::pdf::test_helpers::fixture_path,
    };

    fn nested_page_tree_document() -> Document {
        let mut document = Document::with_version("1.7");
        let catalog_id = (1, 0);
        let root_pages_id = (2, 0);
        let branch_id = (3, 0);
        let empty_branch_id = (9, 0);
        let page_ids = [(4, 0), (5, 0), (6, 0), (7, 0), (8, 0)];
        let content_ids = [(20, 0), (21, 0), (22, 0), (23, 0), (24, 0)];

        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(root_pages_id));
        document
            .objects
            .insert(catalog_id, Object::Dictionary(catalog));

        let mut root = Dictionary::new();
        root.set("Type", Object::Name(b"Pages".to_vec()));
        root.set("Count", Object::Integer(5));
        root.set(
            "Kids",
            Object::Array(
                [
                    empty_branch_id,
                    branch_id,
                    page_ids[2],
                    page_ids[3],
                    page_ids[4],
                ]
                .into_iter()
                .map(Object::Reference)
                .collect(),
            ),
        );
        document
            .objects
            .insert(root_pages_id, Object::Dictionary(root));

        let mut empty_branch = Dictionary::new();
        empty_branch.set("Type", Object::Name(b"Pages".to_vec()));
        empty_branch.set("Parent", Object::Reference(root_pages_id));
        empty_branch.set("Count", Object::Integer(0));
        empty_branch.set("Kids", Object::Array(Vec::new()));
        document
            .objects
            .insert(empty_branch_id, Object::Dictionary(empty_branch));

        let mut branch = Dictionary::new();
        branch.set("Type", Object::Name(b"Pages".to_vec()));
        branch.set("Parent", Object::Reference(root_pages_id));
        branch.set("Count", Object::Integer(2));
        branch.set(
            "Kids",
            Object::Array(
                page_ids[..2]
                    .iter()
                    .copied()
                    .map(Object::Reference)
                    .collect(),
            ),
        );
        document
            .objects
            .insert(branch_id, Object::Dictionary(branch));

        for (index, (page_id, content_id)) in page_ids.into_iter().zip(content_ids).enumerate() {
            let parent = if index < 2 { branch_id } else { root_pages_id };
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set("Parent", Object::Reference(parent));
            page.set("Contents", Object::Reference(content_id));
            document.objects.insert(page_id, Object::Dictionary(page));
            document.objects.insert(
                content_id,
                Object::Stream(Stream::new(Dictionary::new(), Vec::new())),
            );
        }
        document.trailer.set("Root", Object::Reference(catalog_id));
        document.max_id = 24;
        document
    }

    #[test]
    fn resolves_sparse_pages_with_ancestor_and_content_identity() {
        let document = nested_page_tree_document();
        let selected = PageSet::from_pages([2, 5]).expect("selected pages");
        let index = PdfPageIndex::resolve(&document, &selected).expect("page index");

        assert_eq!(index.page_count(), 5);
        assert_eq!(index.selected_page_count(), 2);
        assert_eq!(index.page(2).expect("page 2").page_number(), 2);
        assert_eq!(index.page(2).expect("page 2").page_id(), (5, 0));
        assert_eq!(
            index.page(2).expect("page 2").ancestor_page_tree_ids(),
            &[(2, 0), (3, 0)]
        );
        assert_eq!(
            index.page(2).expect("page 2").content_stream_ids(),
            &[(21, 0)]
        );
        assert_eq!(index.page(5).expect("page 5").page_id(), (8, 0));
        assert_eq!(
            index.page(5).expect("page 5").ancestor_page_tree_ids(),
            &[(2, 0)]
        );
        assert!(matches!(
            index.page(1),
            Err(PdfPageIndexError::PageNotSelected(1))
        ));
    }

    #[test]
    fn rejects_out_of_bounds_and_page_tree_cycles() {
        let document = nested_page_tree_document();
        assert!(matches!(
            PdfPageIndex::resolve_page(&document, 0),
            Err(PdfPageIndexError::PageSet(_))
        ));
        let selected = PageSet::from_pages([6]).expect("selected page");
        assert!(matches!(
            PdfPageIndex::resolve(&document, &selected),
            Err(PdfPageIndexError::PageOutOfBounds {
                page: 6,
                page_count: 5
            })
        ));

        let mut cyclic = nested_page_tree_document();
        cyclic
            .get_object_mut((3, 0))
            .and_then(Object::as_dict_mut)
            .expect("branch dictionary")
            .set("Kids", Object::Array(vec![Object::Reference((3, 0))]));
        cyclic
            .get_object_mut((3, 0))
            .and_then(Object::as_dict_mut)
            .expect("branch dictionary")
            .set("Count", Object::Integer(1));
        let selected = PageSet::from_pages([1]).expect("selected page");
        assert!(matches!(
            PdfPageIndex::resolve(&cyclic, &selected),
            Err(PdfPageIndexError::PageTreeCycle((3, 0)))
        ));

        let mut repeated = nested_page_tree_document();
        repeated
            .get_object_mut((2, 0))
            .and_then(Object::as_dict_mut)
            .expect("root page tree")
            .set(
                "Kids",
                Object::Array(
                    [(3, 0), (6, 0), (6, 0), (8, 0)]
                        .into_iter()
                        .map(Object::Reference)
                        .collect(),
                ),
            );
        let selected = PageSet::from_pages([3, 4]).expect("repeated page selection");
        assert!(matches!(
            PdfPageIndex::resolve(&repeated, &selected),
            Err(PdfPageIndexError::PageTreeNodeRepeated((6, 0)))
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn lazy_real_page_index_matches_lopdf_without_enumerating_later_pages() {
        let source_path = fixture_path("2305.13048v2.pdf");
        let source = PdfSourceObjectStore::open(&source_path).expect("lazy source object store");
        let first_only = PageSet::from_pages([1]).expect("first page");
        let first_index = PdfPageIndex::resolve(&source, &first_only).expect("first page index");
        assert_eq!(first_index.page_count(), 30);
        assert_eq!(first_index.selected_page_count(), 1);
        assert!(source.cache_stats().expect("cache stats").source_loads <= 4);

        let selected = PageSet::from_pages([1, 3, 30]).expect("sparse pages");
        let index = PdfPageIndex::resolve(&source, &selected).expect("sparse page index");
        let bytes = fs::read(&source_path).expect("source PDF");
        let document = Document::load_mem(&bytes).expect("lopdf source");
        let lopdf_pages = document.get_pages();
        assert_eq!(index.page_count(), source.page_count());
        for page_number in selected.pages() {
            let page = index.page(*page_number).expect("indexed page");
            let expected_page_id = lopdf_pages[page_number];
            assert_eq!(page.page_id(), expected_page_id);
            assert_eq!(
                page.content_stream_ids(),
                document.get_page_contents(expected_page_id)
            );
        }
    }
}
