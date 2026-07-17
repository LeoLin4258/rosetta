use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
    time::Instant,
};

use lopdf::{content::Content, Dictionary, Document, Object, ObjectId, Stream};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    content_stream::{discover_page_streams, operand_id, ContentStreamProbeError},
    types::FormInvocationStep,
};

const MAX_FORM_XOBJECT_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentOperandRangePatch {
    pub page_number: u32,
    pub stream_object_number: u32,
    pub stream_generation: u16,
    pub operation_index: usize,
    pub operand_index: usize,
    pub array_index: Option<usize>,
    pub encoded_start: usize,
    pub encoded_len: usize,
    pub expected_operand_byte_count: usize,
    pub expected_operand_hash: String,
    pub replacement_bytes: Vec<u8>,
    pub form_invocation_path: Vec<FormInvocationStep>,
}

impl ContentOperandRangePatch {
    fn stream_id(&self) -> ObjectId {
        (self.stream_object_number, self.stream_generation)
    }

    fn target_id(&self) -> String {
        operand_id(
            self.page_number,
            self.stream_id(),
            self.operation_index,
            self.operand_index,
            self.array_index,
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContentPatchApplyResult {
    pub schema: &'static str,
    pub page_number: u32,
    pub patch_count: usize,
    pub modified_stream_count: usize,
    pub replaced_source_bytes: usize,
    pub replacement_bytes: usize,
    pub modified_stream_ids: Vec<String>,
    pub cloned_stream_count: usize,
    pub page_content_rewired: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceReferenceBinding {
    pub category: Vec<u8>,
    pub name: Vec<u8>,
    pub object_id: ObjectId,
}

pub(crate) struct InvocationLocalCopyOnWriteStage {
    pub streams: BTreeMap<ObjectId, Stream>,
    pub page: Dictionary,
    pub next_object_number: u32,
}

pub(crate) struct InvocationLocalCopyOnWriteTarget {
    pub target_stream_id: ObjectId,
    pub form_invocation_path: Vec<FormInvocationStep>,
    pub patched_stream: Stream,
    pub resource_bindings: Vec<ResourceReferenceBinding>,
}

#[derive(Debug)]
pub(crate) enum ContentPatchError {
    Discovery(ContentStreamProbeError),
    PageOutOfBounds {
        page: u32,
        page_count: u32,
    },
    PatchPageMismatch {
        expected: u32,
        actual: u32,
    },
    StreamOutsidePage {
        object_number: u32,
        generation: u16,
    },
    PageContentReferenceAmbiguous {
        object_number: u32,
        generation: u16,
        reference_count: usize,
    },
    FormOwnershipIncomplete(Vec<String>),
    FormInvocationPathRequired {
        object_number: u32,
        generation: u16,
    },
    FormInvocationPathInvalid(String),
    ResourceBindingConflict {
        category: Vec<u8>,
        name: Vec<u8>,
    },
    PageContentRewrite(String),
    StreamRead {
        object_number: u32,
        generation: u16,
        message: String,
    },
    ContentDecode {
        object_number: u32,
        generation: u16,
        message: String,
    },
    OperationMissing {
        operand_id: String,
    },
    OperandMissing {
        operand_id: String,
    },
    OperandLengthMismatch {
        operand_id: String,
        expected: usize,
        actual: usize,
    },
    OperandHashMismatch {
        operand_id: String,
        expected: String,
        actual: String,
    },
    RangeOutOfBounds {
        operand_id: String,
        start: usize,
        len: usize,
        operand_len: usize,
    },
    OverlappingRanges {
        operand_id: String,
    },
    ConflictingOperandIdentity {
        operand_id: String,
    },
    ContentEncode {
        object_number: u32,
        generation: u16,
        message: String,
    },
    StreamWrite {
        object_number: u32,
        generation: u16,
        message: String,
    },
}

impl fmt::Display for ContentPatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => error.fmt(formatter),
            Self::PageOutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
            Self::PatchPageMismatch { expected, actual } => write!(
                formatter,
                "content patch page mismatch: expected page {expected}, received page {actual}"
            ),
            Self::StreamOutsidePage { object_number, generation } => write!(
                formatter,
                "content stream {object_number} {generation} is not reachable from the selected page"
            ),
            Self::PageContentReferenceAmbiguous {
                object_number,
                generation,
                reference_count,
            } => write!(
                formatter,
                "page content stream {object_number} {generation} appears {reference_count} times on the selected page"
            ),
            Self::FormOwnershipIncomplete(reasons) => write!(
                formatter,
                "Form ownership scan is incomplete: {}",
                reasons.join(", ")
            ),
            Self::FormInvocationPathRequired {
                object_number,
                generation,
            } => write!(
                formatter,
                "Form content stream {object_number} {generation} requires a structured invocation path for copy-on-write"
            ),
            Self::FormInvocationPathInvalid(message) => {
                write!(formatter, "Form invocation path is invalid: {message}")
            }
            Self::ResourceBindingConflict { category, name } => write!(
                formatter,
                "resource binding {}/{} conflicts with an existing resource",
                String::from_utf8_lossy(category),
                String::from_utf8_lossy(name)
            ),
            Self::PageContentRewrite(message) => formatter.write_str(message),
            Self::StreamRead { object_number, generation, message } => write!(
                formatter,
                "failed to read content stream {object_number} {generation}: {message}"
            ),
            Self::ContentDecode { object_number, generation, message } => write!(
                formatter,
                "failed to decode content stream {object_number} {generation}: {message}"
            ),
            Self::OperationMissing { operand_id } => {
                write!(formatter, "patch operation is missing for {operand_id}")
            }
            Self::OperandMissing { operand_id } => {
                write!(formatter, "patch operand is missing for {operand_id}")
            }
            Self::OperandLengthMismatch { operand_id, expected, actual } => write!(
                formatter,
                "operand length mismatch for {operand_id}: expected {expected}, found {actual}"
            ),
            Self::OperandHashMismatch { operand_id, expected, actual } => write!(
                formatter,
                "operand hash mismatch for {operand_id}: expected {expected}, found {actual}"
            ),
            Self::RangeOutOfBounds { operand_id, start, len, operand_len } => write!(
                formatter,
                "patch range {start}+{len} is outside operand {operand_id} length {operand_len}"
            ),
            Self::OverlappingRanges { operand_id } => {
                write!(formatter, "patch ranges overlap for {operand_id}")
            }
            Self::ConflictingOperandIdentity { operand_id } => write!(
                formatter,
                "patches disagree about the expected source identity for {operand_id}"
            ),
            Self::ContentEncode { object_number, generation, message } => write!(
                formatter,
                "failed to encode patched stream {object_number} {generation}: {message}"
            ),
            Self::StreamWrite { object_number, generation, message } => write!(
                formatter,
                "failed to stage patched stream {object_number} {generation}: {message}"
            ),
        }
    }
}

impl std::error::Error for ContentPatchError {}

impl From<ContentStreamProbeError> for ContentPatchError {
    fn from(value: ContentStreamProbeError) -> Self {
        Self::Discovery(value)
    }
}

type OperandKey = (usize, usize, Option<usize>);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PatchTargetKey {
    stream_id: ObjectId,
    form_invocation_path: Vec<FormInvocationStep>,
}

impl From<&ContentOperandRangePatch> for PatchTargetKey {
    fn from(patch: &ContentOperandRangePatch) -> Self {
        Self {
            stream_id: patch.stream_id(),
            form_invocation_path: patch.form_invocation_path.clone(),
        }
    }
}

impl PatchTargetKey {
    fn root_stream_id(&self) -> ObjectId {
        self.form_invocation_path
            .first()
            .map(FormInvocationStep::parent_stream_id)
            .unwrap_or(self.stream_id)
    }
}

pub(crate) fn apply_content_operand_patches(
    document: &mut Document,
    page_number: u32,
    patches: &[ContentOperandRangePatch],
) -> Result<ContentPatchApplyResult, ContentPatchError> {
    let started = Instant::now();
    let pages = document.get_pages();
    let page_count = pages.len() as u32;
    let page_id = pages
        .get(&page_number)
        .copied()
        .ok_or(ContentPatchError::PageOutOfBounds {
            page: page_number,
            page_count,
        })?;
    for patch in patches {
        if patch.page_number != page_number {
            return Err(ContentPatchError::PatchPageMismatch {
                expected: page_number,
                actual: patch.page_number,
            });
        }
    }
    if patches.is_empty() {
        return Ok(ContentPatchApplyResult {
            schema: "rosetta-pdf-v3-content-patch-apply/2",
            page_number,
            patch_count: 0,
            modified_stream_count: 0,
            replaced_source_bytes: 0,
            replacement_bytes: 0,
            modified_stream_ids: Vec::new(),
            cloned_stream_count: 0,
            page_content_rewired: false,
            elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        });
    }

    let selected_discovery = discover_page_streams(document, page_id, page_number)?;
    let target_streams = patches
        .iter()
        .map(ContentOperandRangePatch::stream_id)
        .collect::<HashSet<_>>();
    for stream_id in &target_streams {
        if !selected_discovery.streams.contains_key(stream_id) {
            return Err(ContentPatchError::StreamOutsidePage {
                object_number: stream_id.0,
                generation: stream_id.1,
            });
        }
    }
    let page_references = content_stream_page_references(document);
    let targeted_form_streams = target_streams
        .iter()
        .filter(|stream_id| {
            selected_discovery
                .streams
                .get(stream_id)
                .is_some_and(|stream| stream.is_form_xobject)
        })
        .copied()
        .collect::<HashSet<_>>();
    let form_resource_pages = if targeted_form_streams.is_empty() {
        HashMap::new()
    } else {
        form_stream_resource_pages(document, &targeted_form_streams)?
    };
    let target_keys = patches
        .iter()
        .map(PatchTargetKey::from)
        .collect::<BTreeSet<_>>();
    let mut copy_on_write_targets = BTreeSet::new();
    for target in &target_keys {
        let is_form = targeted_form_streams.contains(&target.stream_id);
        if is_form {
            if let Some(last) = target.form_invocation_path.last() {
                if last.form_stream_id() != target.stream_id {
                    return Err(ContentPatchError::FormInvocationPathInvalid(format!(
                        "last child stream is {} {}, expected {} {}",
                        last.form_stream_object_number,
                        last.form_stream_generation,
                        target.stream_id.0,
                        target.stream_id.1
                    )));
                }
            }
            let invocation_count = selected_discovery
                .streams
                .get(&target.stream_id)
                .map(|stream| stream.form_invocation_paths.len())
                .unwrap_or_default();
            let resource_page_count = form_resource_pages
                .get(&target.stream_id)
                .map(BTreeSet::len)
                .unwrap_or_default();
            if invocation_count > 1 || resource_page_count > 1 {
                if target.form_invocation_path.is_empty() {
                    return Err(ContentPatchError::FormInvocationPathRequired {
                        object_number: target.stream_id.0,
                        generation: target.stream_id.1,
                    });
                }
                copy_on_write_targets.insert(target.clone());
            }
        } else {
            if !target.form_invocation_path.is_empty() {
                return Err(ContentPatchError::FormInvocationPathInvalid(
                    "top-level page content patch contains a Form invocation path".to_string(),
                ));
            }
            let selected_reference_count = document
                .get_page_contents(page_id)
                .iter()
                .filter(|stream_id| **stream_id == target.stream_id)
                .count();
            if selected_reference_count > 1 {
                return Err(ContentPatchError::PageContentReferenceAmbiguous {
                    object_number: target.stream_id.0,
                    generation: target.stream_id.1,
                    reference_count: selected_reference_count,
                });
            }
            if page_references
                .get(&target.stream_id)
                .is_some_and(|pages| pages.len() > 1)
            {
                copy_on_write_targets.insert(target.clone());
            }
        }
    }
    let mut grouped =
        BTreeMap::<PatchTargetKey, BTreeMap<OperandKey, Vec<&ContentOperandRangePatch>>>::new();
    for patch in patches {
        grouped
            .entry(PatchTargetKey::from(patch))
            .or_default()
            .entry((
                patch.operation_index,
                patch.operand_index,
                patch.array_index,
            ))
            .or_default()
            .push(patch);
    }

    let mut staged_target_streams = BTreeMap::<PatchTargetKey, Stream>::new();
    let mut replaced_source_bytes = 0usize;
    let mut replacement_bytes = 0usize;
    for (target, operand_groups) in grouped {
        let stream_id = target.stream_id;
        let source_stream = document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .map_err(|error| ContentPatchError::StreamRead {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            })?;
        let source_content =
            source_stream
                .get_plain_content()
                .map_err(|error| ContentPatchError::StreamRead {
                    object_number: stream_id.0,
                    generation: stream_id.1,
                    message: error.to_string(),
                })?;
        let mut content =
            Content::decode(&source_content).map_err(|error| ContentPatchError::ContentDecode {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            })?;
        for ((operation_index, operand_index, array_index), mut operand_patches) in operand_groups {
            let operand_id = operand_id(
                page_number,
                stream_id,
                operation_index,
                operand_index,
                array_index,
            );
            let operation = content.operations.get_mut(operation_index).ok_or_else(|| {
                ContentPatchError::OperationMissing {
                    operand_id: operand_id.clone(),
                }
            })?;
            let bytes =
                text_operand_bytes_mut(operation, operand_index, array_index).ok_or_else(|| {
                    ContentPatchError::OperandMissing {
                        operand_id: operand_id.clone(),
                    }
                })?;
            let expected_len = operand_patches[0].expected_operand_byte_count;
            let expected_hash = &operand_patches[0].expected_operand_hash;
            if operand_patches.iter().any(|patch| {
                patch.expected_operand_byte_count != expected_len
                    || patch.expected_operand_hash != *expected_hash
                    || patch.target_id() != operand_id
            }) {
                return Err(ContentPatchError::ConflictingOperandIdentity { operand_id });
            }
            if bytes.len() != expected_len {
                return Err(ContentPatchError::OperandLengthMismatch {
                    operand_id,
                    expected: expected_len,
                    actual: bytes.len(),
                });
            }
            let actual_hash = byte_hash(bytes);
            if actual_hash != *expected_hash {
                return Err(ContentPatchError::OperandHashMismatch {
                    operand_id,
                    expected: expected_hash.clone(),
                    actual: actual_hash,
                });
            }
            operand_patches.sort_by_key(|patch| patch.encoded_start);
            let mut previous_end = 0usize;
            for patch in &operand_patches {
                let Some(end) = patch.encoded_start.checked_add(patch.encoded_len) else {
                    return Err(ContentPatchError::RangeOutOfBounds {
                        operand_id: operand_id.clone(),
                        start: patch.encoded_start,
                        len: patch.encoded_len,
                        operand_len: bytes.len(),
                    });
                };
                if end > bytes.len() {
                    return Err(ContentPatchError::RangeOutOfBounds {
                        operand_id: operand_id.clone(),
                        start: patch.encoded_start,
                        len: patch.encoded_len,
                        operand_len: bytes.len(),
                    });
                }
                if patch.encoded_start < previous_end {
                    return Err(ContentPatchError::OverlappingRanges {
                        operand_id: operand_id.clone(),
                    });
                }
                previous_end = end;
            }
            for patch in operand_patches.into_iter().rev() {
                let end = patch.encoded_start + patch.encoded_len;
                bytes.splice(
                    patch.encoded_start..end,
                    patch.replacement_bytes.iter().copied(),
                );
                replaced_source_bytes += patch.encoded_len;
                replacement_bytes += patch.replacement_bytes.len();
            }
        }
        let rewritten_content =
            content
                .encode()
                .map_err(|error| ContentPatchError::ContentEncode {
                    object_number: stream_id.0,
                    generation: stream_id.1,
                    message: error.to_string(),
                })?;
        let mut staged_stream = source_stream.clone();
        staged_stream.set_plain_content(rewritten_content);
        staged_stream
            .compress()
            .map_err(|error| ContentPatchError::StreamWrite {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            })?;
        staged_target_streams.insert(target, staged_stream);
    }

    let mut staged_streams = BTreeMap::<ObjectId, Stream>::new();
    let mut staged_page = None;
    let mut cloned_stream_count = 0usize;
    let mut next_object_number = document.max_id;
    if !copy_on_write_targets.is_empty() {
        let copy_on_write_roots = copy_on_write_targets
            .iter()
            .map(PatchTargetKey::root_stream_id)
            .collect::<BTreeSet<_>>();
        let cloned_targets = staged_target_streams
            .keys()
            .filter(|target| copy_on_write_roots.contains(&target.root_stream_id()))
            .cloned()
            .collect::<Vec<_>>();
        let mut batch_targets = Vec::with_capacity(cloned_targets.len());
        for target in cloned_targets {
            let patched_stream = staged_target_streams.remove(&target).ok_or_else(|| {
                ContentPatchError::FormInvocationPathInvalid(
                    "copy-on-write target was not staged".to_string(),
                )
            })?;
            batch_targets.push(InvocationLocalCopyOnWriteTarget {
                target_stream_id: target.stream_id,
                form_invocation_path: target.form_invocation_path,
                patched_stream,
                resource_bindings: Vec::new(),
            });
        }
        let stage = stage_invocation_local_copy_on_write_batch(
            document,
            page_id,
            page_number,
            batch_targets,
            next_object_number,
        )?;
        next_object_number = stage.next_object_number;
        cloned_stream_count = stage.streams.len();
        staged_streams = stage.streams;
        staged_page = Some(stage.page);
    }
    for (target, stream) in staged_target_streams {
        staged_streams.insert(target.stream_id, stream);
    }

    let modified_stream_ids = staged_streams
        .keys()
        .map(|stream_id| format!("{}-{}", stream_id.0, stream_id.1))
        .collect::<Vec<_>>();
    for (stream_id, stream) in staged_streams {
        document.objects.insert(stream_id, Object::Stream(stream));
    }
    if let Some(page) = staged_page {
        document.objects.insert(page_id, Object::Dictionary(page));
        document.max_id = document.max_id.max(next_object_number);
    }
    Ok(ContentPatchApplyResult {
        schema: "rosetta-pdf-v3-content-patch-apply/2",
        page_number,
        patch_count: patches.len(),
        modified_stream_count: modified_stream_ids.len(),
        replaced_source_bytes,
        replacement_bytes,
        modified_stream_ids,
        cloned_stream_count,
        page_content_rewired: cloned_stream_count > 0,
        elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

struct ValidatedInvocationLink {
    step: FormInvocationStep,
    resources: Dictionary,
}

struct ValidatedInvocationPath {
    links: Vec<ValidatedInvocationLink>,
    target_resources: Dictionary,
}

pub(crate) fn stage_invocation_local_copy_on_write(
    document: &Document,
    page_id: ObjectId,
    page_number: u32,
    target_stream_id: ObjectId,
    form_invocation_path: &[FormInvocationStep],
    patched_stream: Stream,
    resource_bindings: &[ResourceReferenceBinding],
    reserved_through: u32,
) -> Result<InvocationLocalCopyOnWriteStage, ContentPatchError> {
    stage_invocation_local_copy_on_write_batch(
        document,
        page_id,
        page_number,
        vec![InvocationLocalCopyOnWriteTarget {
            target_stream_id,
            form_invocation_path: form_invocation_path.to_vec(),
            patched_stream,
            resource_bindings: resource_bindings.to_vec(),
        }],
        reserved_through,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct InvocationNodeKey {
    root_stream_id: ObjectId,
    path: Vec<FormInvocationStep>,
}

struct InvocationNodePlan {
    source_stream_id: ObjectId,
    effective_resources: Option<Dictionary>,
    patched_stream: Option<Stream>,
    resource_bindings: Vec<ResourceReferenceBinding>,
    children: BTreeMap<usize, InvocationNodeKey>,
}

pub(crate) fn stage_invocation_local_copy_on_write_batch(
    document: &Document,
    page_id: ObjectId,
    page_number: u32,
    targets: Vec<InvocationLocalCopyOnWriteTarget>,
    reserved_through: u32,
) -> Result<InvocationLocalCopyOnWriteStage, ContentPatchError> {
    if targets.is_empty() {
        return Err(ContentPatchError::FormInvocationPathInvalid(
            "copy-on-write batch is empty".to_string(),
        ));
    }

    let mut nodes = BTreeMap::<InvocationNodeKey, InvocationNodePlan>::new();
    let mut page_resource_bindings = Vec::new();
    for target in targets {
        let root_stream_id = target
            .form_invocation_path
            .first()
            .map(FormInvocationStep::parent_stream_id)
            .unwrap_or(target.target_stream_id);
        let validated = if target.form_invocation_path.is_empty() {
            let reference_count = document
                .get_page_contents(page_id)
                .iter()
                .filter(|stream_id| **stream_id == target.target_stream_id)
                .count();
            if reference_count != 1 {
                return Err(ContentPatchError::PageContentReferenceAmbiguous {
                    object_number: target.target_stream_id.0,
                    generation: target.target_stream_id.1,
                    reference_count,
                });
            }
            None
        } else {
            Some(validate_invocation_path(
                document,
                page_id,
                page_number,
                target.target_stream_id,
                &target.form_invocation_path,
            )?)
        };

        let root_key = InvocationNodeKey {
            root_stream_id,
            path: Vec::new(),
        };
        nodes
            .entry(root_key.clone())
            .or_insert_with(|| InvocationNodePlan {
                source_stream_id: root_stream_id,
                effective_resources: None,
                patched_stream: None,
                resource_bindings: Vec::new(),
                children: BTreeMap::new(),
            });

        if let Some(validated) = &validated {
            let mut parent_key = root_key;
            for (index, link) in validated.links.iter().enumerate() {
                let child_key = InvocationNodeKey {
                    root_stream_id,
                    path: target.form_invocation_path[..=index].to_vec(),
                };
                let effective_resources = if index + 1 < validated.links.len() {
                    validated.links[index + 1].resources.clone()
                } else {
                    validated.target_resources.clone()
                };
                nodes
                    .entry(child_key.clone())
                    .or_insert_with(|| InvocationNodePlan {
                        source_stream_id: link.step.form_stream_id(),
                        effective_resources: Some(effective_resources),
                        patched_stream: None,
                        resource_bindings: Vec::new(),
                        children: BTreeMap::new(),
                    });
                let parent = nodes.get_mut(&parent_key).ok_or_else(|| {
                    ContentPatchError::FormInvocationPathInvalid(
                        "copy-on-write parent node is missing".to_string(),
                    )
                })?;
                if parent
                    .children
                    .insert(link.step.operation_index, child_key.clone())
                    .is_some_and(|existing| existing != child_key)
                {
                    return Err(ContentPatchError::FormInvocationPathInvalid(format!(
                        "copy-on-write paths conflict at operation {} in stream {} {}",
                        link.step.operation_index,
                        link.step.parent_stream_object_number,
                        link.step.parent_stream_generation
                    )));
                }
                parent_key = child_key;
            }
        }

        let target_key = InvocationNodeKey {
            root_stream_id,
            path: target.form_invocation_path,
        };
        let target_node = nodes.get_mut(&target_key).ok_or_else(|| {
            ContentPatchError::FormInvocationPathInvalid(
                "copy-on-write target node is missing".to_string(),
            )
        })?;
        if target_node
            .patched_stream
            .replace(target.patched_stream)
            .is_some()
        {
            return Err(ContentPatchError::FormInvocationPathInvalid(format!(
                "copy-on-write target {} {} is duplicated",
                target.target_stream_id.0, target.target_stream_id.1
            )));
        }
        if target_key.path.is_empty() {
            page_resource_bindings.extend(target.resource_bindings);
        } else {
            target_node
                .resource_bindings
                .extend(target.resource_bindings);
        }
    }

    let mut page_resources =
        materialize_resource_context(document, &page_resource_context(document, page_id)?);
    let mut page_resources_changed = !page_resource_bindings.is_empty();
    attach_resource_bindings(&mut page_resources, &page_resource_bindings)?;

    let mut node_keys = nodes.keys().cloned().collect::<Vec<_>>();
    node_keys.sort_by(|left, right| {
        right
            .path
            .len()
            .cmp(&left.path.len())
            .then_with(|| left.cmp(right))
    });
    let mut next_object_number = document.max_id.max(reserved_through);
    let mut streams = BTreeMap::new();
    let mut cloned_ids = BTreeMap::<InvocationNodeKey, ObjectId>::new();
    let mut root_replacements = BTreeMap::<ObjectId, ObjectId>::new();
    for key in node_keys {
        let plan = nodes.remove(&key).ok_or_else(|| {
            ContentPatchError::FormInvocationPathInvalid(
                "copy-on-write node disappeared during staging".to_string(),
            )
        })?;
        let mut staged_stream = if let Some(stream) = plan.patched_stream {
            stream
        } else {
            document
                .get_object(plan.source_stream_id)
                .and_then(Object::as_stream)
                .cloned()
                .map_err(|error| ContentPatchError::StreamRead {
                    object_number: plan.source_stream_id.0,
                    generation: plan.source_stream_id.1,
                    message: error.to_string(),
                })?
        };
        let mut node_resources = plan.effective_resources;
        if let Some(resources) = node_resources.as_mut() {
            attach_resource_bindings(resources, &plan.resource_bindings)?;
        }
        if !plan.children.is_empty() {
            let source_content = staged_stream.get_plain_content().map_err(|error| {
                ContentPatchError::StreamRead {
                    object_number: plan.source_stream_id.0,
                    generation: plan.source_stream_id.1,
                    message: error.to_string(),
                }
            })?;
            let mut content = Content::decode(&source_content).map_err(|error| {
                ContentPatchError::ContentDecode {
                    object_number: plan.source_stream_id.0,
                    generation: plan.source_stream_id.1,
                    message: error.to_string(),
                }
            })?;
            for (operation_index, child_key) in plan.children {
                let child_stream_id = cloned_ids.get(&child_key).copied().ok_or_else(|| {
                    ContentPatchError::FormInvocationPathInvalid(
                        "copy-on-write child was not staged before its parent".to_string(),
                    )
                })?;
                let resources = if key.path.is_empty() {
                    page_resources_changed = true;
                    &mut page_resources
                } else {
                    node_resources.as_mut().ok_or_else(|| {
                        ContentPatchError::FormInvocationPathInvalid(
                            "Form clone has no effective resources".to_string(),
                        )
                    })?
                };
                let alias = insert_xobject_alias(resources, child_stream_id);
                let operation = content.operations.get_mut(operation_index).ok_or_else(|| {
                    ContentPatchError::FormInvocationPathInvalid(format!(
                        "operation {operation_index} disappeared from parent stream {} {}",
                        plan.source_stream_id.0, plan.source_stream_id.1
                    ))
                })?;
                if operation.operator != "Do" {
                    return Err(ContentPatchError::FormInvocationPathInvalid(format!(
                        "operation {operation_index} in parent stream {} {} is not Do",
                        plan.source_stream_id.0, plan.source_stream_id.1
                    )));
                }
                let operand = operation.operands.first_mut().ok_or_else(|| {
                    ContentPatchError::FormInvocationPathInvalid(format!(
                        "Do operation {operation_index} has no operand"
                    ))
                })?;
                *operand = Object::Name(alias);
            }
            let rewritten_content =
                content
                    .encode()
                    .map_err(|error| ContentPatchError::ContentEncode {
                        object_number: plan.source_stream_id.0,
                        generation: plan.source_stream_id.1,
                        message: error.to_string(),
                    })?;
            staged_stream.set_plain_content(rewritten_content);
            staged_stream
                .compress()
                .map_err(|error| ContentPatchError::StreamWrite {
                    object_number: plan.source_stream_id.0,
                    generation: plan.source_stream_id.1,
                    message: error.to_string(),
                })?;
        }
        if let Some(resources) = node_resources {
            staged_stream
                .dict
                .set("Resources", Object::Dictionary(resources));
        }
        let staged_stream_id = allocate_staged_object_id(document, &mut next_object_number);
        streams.insert(staged_stream_id, staged_stream);
        cloned_ids.insert(key.clone(), staged_stream_id);
        if key.path.is_empty() {
            root_replacements.insert(key.root_stream_id, staged_stream_id);
        }
    }

    let mut page = page_with_replaced_content_streams(document, page_id, &root_replacements)?;
    if page_resources_changed {
        page.set("Resources", Object::Dictionary(page_resources));
    }
    Ok(InvocationLocalCopyOnWriteStage {
        streams,
        page,
        next_object_number,
    })
}

fn allocate_staged_object_id(document: &Document, next_object_number: &mut u32) -> ObjectId {
    loop {
        *next_object_number += 1;
        let object_id = (*next_object_number, 0);
        if !document.objects.contains_key(&object_id) {
            return object_id;
        }
    }
}

fn validate_invocation_path(
    document: &Document,
    page_id: ObjectId,
    page_number: u32,
    target_stream_id: ObjectId,
    path: &[FormInvocationStep],
) -> Result<ValidatedInvocationPath, ContentPatchError> {
    if path.is_empty() || path.len() > MAX_FORM_XOBJECT_DEPTH {
        return Err(ContentPatchError::FormInvocationPathInvalid(format!(
            "page {page_number} path length {} is outside 1..={MAX_FORM_XOBJECT_DEPTH}",
            path.len()
        )));
    }
    let root_stream_id = path[0].parent_stream_id();
    let root_reference_count = document
        .get_page_contents(page_id)
        .iter()
        .filter(|stream_id| **stream_id == root_stream_id)
        .count();
    if root_reference_count != 1 {
        return Err(ContentPatchError::PageContentReferenceAmbiguous {
            object_number: root_stream_id.0,
            generation: root_stream_id.1,
            reference_count: root_reference_count,
        });
    }

    let mut resources = page_resource_context(document, page_id)?;
    let mut expected_parent_stream_id = root_stream_id;
    let mut active_form_streams = HashSet::new();
    let mut links = Vec::with_capacity(path.len());
    for step in path {
        if step.parent_stream_id() != expected_parent_stream_id {
            return Err(ContentPatchError::FormInvocationPathInvalid(format!(
                "parent stream {} {} does not continue from {} {}",
                step.parent_stream_object_number,
                step.parent_stream_generation,
                expected_parent_stream_id.0,
                expected_parent_stream_id.1
            )));
        }
        let parent_stream = document
            .get_object(expected_parent_stream_id)
            .and_then(Object::as_stream)
            .map_err(|error| ContentPatchError::StreamRead {
                object_number: expected_parent_stream_id.0,
                generation: expected_parent_stream_id.1,
                message: error.to_string(),
            })?;
        let parent_content =
            parent_stream
                .get_plain_content()
                .map_err(|error| ContentPatchError::StreamRead {
                    object_number: expected_parent_stream_id.0,
                    generation: expected_parent_stream_id.1,
                    message: error.to_string(),
                })?;
        let content =
            Content::decode(&parent_content).map_err(|error| ContentPatchError::ContentDecode {
                object_number: expected_parent_stream_id.0,
                generation: expected_parent_stream_id.1,
                message: error.to_string(),
            })?;
        let operation = content
            .operations
            .get(step.operation_index)
            .ok_or_else(|| {
                ContentPatchError::FormInvocationPathInvalid(format!(
                    "operation {} is missing from parent stream {} {}",
                    step.operation_index, expected_parent_stream_id.0, expected_parent_stream_id.1
                ))
            })?;
        if operation.operator != "Do" {
            return Err(ContentPatchError::FormInvocationPathInvalid(format!(
                "operation {} in parent stream {} {} is not Do",
                step.operation_index, expected_parent_stream_id.0, expected_parent_stream_id.1
            )));
        }
        let resource_name = operation
            .operands
            .first()
            .and_then(|operand| operand.as_name().ok())
            .ok_or_else(|| {
                ContentPatchError::FormInvocationPathInvalid(format!(
                    "Do operation {} in parent stream {} {} has no resource name",
                    step.operation_index, expected_parent_stream_id.0, expected_parent_stream_id.1
                ))
            })?;
        let (resolved_stream_id, form_stream) =
            resolve_xobject(document, &resources, resource_name).ok_or_else(|| {
                ContentPatchError::FormInvocationPathInvalid(format!(
                    "resource {} in parent stream {} {} cannot be resolved",
                    String::from_utf8_lossy(resource_name),
                    expected_parent_stream_id.0,
                    expected_parent_stream_id.1
                ))
            })?;
        let resolved_stream_id = resolved_stream_id.ok_or_else(|| {
            ContentPatchError::FormInvocationPathInvalid(
                "direct Form XObject cannot be copy-on-written".to_string(),
            )
        })?;
        if resolved_stream_id != step.form_stream_id() {
            return Err(ContentPatchError::FormInvocationPathInvalid(format!(
                "Do operation resolves to {} {}, expected {} {}",
                resolved_stream_id.0,
                resolved_stream_id.1,
                step.form_stream_object_number,
                step.form_stream_generation
            )));
        }
        if !form_stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|subtype| subtype == b"Form")
        {
            return Err(ContentPatchError::FormInvocationPathInvalid(format!(
                "resolved stream {} {} is not a Form XObject",
                resolved_stream_id.0, resolved_stream_id.1
            )));
        }
        if !active_form_streams.insert(resolved_stream_id) {
            return Err(ContentPatchError::FormInvocationPathInvalid(
                "Form invocation path contains a reference cycle".to_string(),
            ));
        }
        links.push(ValidatedInvocationLink {
            step: step.clone(),
            resources: materialize_resource_context(document, &resources),
        });
        resources = invoked_form_resource_context(document, form_stream, &resources);
        expected_parent_stream_id = resolved_stream_id;
    }
    if expected_parent_stream_id != target_stream_id {
        return Err(ContentPatchError::FormInvocationPathInvalid(format!(
            "path ends at {} {}, expected target {} {}",
            expected_parent_stream_id.0,
            expected_parent_stream_id.1,
            target_stream_id.0,
            target_stream_id.1
        )));
    }
    Ok(ValidatedInvocationPath {
        links,
        target_resources: materialize_resource_context(document, &resources),
    })
}

fn insert_xobject_alias(resources: &mut Dictionary, child_stream_id: ObjectId) -> Vec<u8> {
    let mut xobjects = resources
        .get(b"XObject")
        .ok()
        .and_then(|object| object.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    let mut suffix = 0usize;
    let alias = loop {
        let alias = format!("RosettaCOW{}_{suffix}", child_stream_id.0).into_bytes();
        if !xobjects.has(&alias) {
            break alias;
        }
        suffix += 1;
    };
    xobjects.set(alias.clone(), Object::Reference(child_stream_id));
    resources.set("XObject", Object::Dictionary(xobjects));
    alias
}

fn attach_resource_bindings(
    resources: &mut Dictionary,
    bindings: &[ResourceReferenceBinding],
) -> Result<(), ContentPatchError> {
    for binding in bindings {
        let mut category = match resources.get(&binding.category) {
            Ok(object) => object.as_dict().cloned().map_err(|_| {
                ContentPatchError::ResourceBindingConflict {
                    category: binding.category.clone(),
                    name: binding.name.clone(),
                }
            })?,
            Err(_) => Dictionary::new(),
        };
        if let Ok(existing) = category.get(&binding.name) {
            if existing.as_reference().ok() != Some(binding.object_id) {
                return Err(ContentPatchError::ResourceBindingConflict {
                    category: binding.category.clone(),
                    name: binding.name.clone(),
                });
            }
        } else {
            category.set(binding.name.clone(), Object::Reference(binding.object_id));
        }
        resources.set(binding.category.clone(), Object::Dictionary(category));
    }
    Ok(())
}

fn page_with_replaced_content_streams(
    document: &Document,
    page_id: ObjectId,
    replacements: &BTreeMap<ObjectId, ObjectId>,
) -> Result<Dictionary, ContentPatchError> {
    let mut content_streams = document.get_page_contents(page_id);
    for source_stream_id in replacements.keys() {
        let reference_count = content_streams
            .iter()
            .filter(|stream_id| *stream_id == source_stream_id)
            .count();
        if reference_count != 1 {
            return Err(ContentPatchError::PageContentReferenceAmbiguous {
                object_number: source_stream_id.0,
                generation: source_stream_id.1,
                reference_count,
            });
        }
    }
    for stream_id in &mut content_streams {
        if let Some(replacement_stream_id) = replacements.get(stream_id) {
            *stream_id = *replacement_stream_id;
        }
    }
    let mut page = document.get_dictionary(page_id).cloned().map_err(|error| {
        ContentPatchError::PageContentRewrite(format!(
            "failed to clone selected page dictionary: {error}"
        ))
    })?;
    page.set(
        "Contents",
        Object::Array(content_streams.into_iter().map(Object::Reference).collect()),
    );
    Ok(page)
}

fn materialize_resource_context(document: &Document, context: &ResourceContext<'_>) -> Dictionary {
    let mut keys = BTreeSet::new();
    for dictionary in &context.dictionaries {
        keys.extend(dictionary.iter().map(|(key, _)| key.clone()));
    }
    let mut materialized = Dictionary::new();
    for key in keys {
        let Some(first) = context
            .dictionaries
            .iter()
            .find_map(|dictionary| dictionary.get(&key).ok())
        else {
            continue;
        };
        if dereference_dictionary(first, document).is_some() {
            let mut merged = Dictionary::new();
            for dictionary in context.dictionaries.iter().rev() {
                if let Some(category) = dictionary_entry(dictionary, &key, document) {
                    for (name, value) in category.iter() {
                        merged.set(name.clone(), value.clone());
                    }
                }
            }
            materialized.set(key, Object::Dictionary(merged));
        } else {
            materialized.set(key, first.clone());
        }
    }
    materialized
}

#[derive(Clone)]
struct ResourceContext<'a> {
    dictionaries: Vec<&'a Dictionary>,
}

fn form_stream_resource_pages(
    document: &Document,
    targets: &HashSet<ObjectId>,
) -> Result<HashMap<ObjectId, BTreeSet<u32>>, ContentPatchError> {
    let mut pages_by_stream = HashMap::<ObjectId, BTreeSet<u32>>::new();
    let mut incomplete_reasons = BTreeSet::new();
    for (page_number, page_id) in document.get_pages() {
        let resources = page_resource_context(document, page_id)?;
        let mut active_form_streams = HashSet::new();
        let mut visited_form_streams = HashSet::new();
        visit_form_resource_graph(
            document,
            &resources,
            page_number,
            targets,
            0,
            &mut active_form_streams,
            &mut visited_form_streams,
            &mut pages_by_stream,
            &mut incomplete_reasons,
        );
    }
    if !incomplete_reasons.is_empty() {
        return Err(ContentPatchError::FormOwnershipIncomplete(
            incomplete_reasons.into_iter().collect(),
        ));
    }
    Ok(pages_by_stream)
}

#[allow(clippy::too_many_arguments)]
fn visit_form_resource_graph<'a>(
    document: &'a Document,
    resources: &ResourceContext<'a>,
    page_number: u32,
    targets: &HashSet<ObjectId>,
    depth: usize,
    active_form_streams: &mut HashSet<ObjectId>,
    visited_form_streams: &mut HashSet<ObjectId>,
    pages_by_stream: &mut HashMap<ObjectId, BTreeSet<u32>>,
    incomplete_reasons: &mut BTreeSet<String>,
) {
    for object in xobjects_for_context(document, resources).values() {
        let (stream_id, stream) = match object {
            Object::Reference(stream_id) => {
                let Ok(stream) = document.get_object(*stream_id).and_then(Object::as_stream) else {
                    continue;
                };
                (Some(*stream_id), stream)
            }
            Object::Stream(stream) => (None, stream),
            _ => continue,
        };
        if !stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|subtype| subtype == b"Form")
        {
            continue;
        }
        let Some(stream_id) = stream_id else {
            incomplete_reasons.insert("direct-form-xobject-stream-unsupported".to_string());
            continue;
        };
        if targets.contains(&stream_id) {
            pages_by_stream
                .entry(stream_id)
                .or_default()
                .insert(page_number);
        }
        if depth >= MAX_FORM_XOBJECT_DEPTH {
            incomplete_reasons.insert("form-xobject-depth-limit".to_string());
            continue;
        }
        if active_form_streams.contains(&stream_id) {
            incomplete_reasons.insert("form-xobject-reference-cycle".to_string());
            continue;
        }
        if !visited_form_streams.insert(stream_id) {
            continue;
        }
        active_form_streams.insert(stream_id);
        let child_resources = form_resource_context(document, stream);
        visit_form_resource_graph(
            document,
            &child_resources,
            page_number,
            targets,
            depth + 1,
            active_form_streams,
            visited_form_streams,
            pages_by_stream,
            incomplete_reasons,
        );
        active_form_streams.remove(&stream_id);
    }
}

fn page_resource_context<'a>(
    document: &'a Document,
    page_id: ObjectId,
) -> Result<ResourceContext<'a>, ContentPatchError> {
    let (direct, resource_ids) = document.get_page_resources(page_id).map_err(|error| {
        ContentPatchError::FormOwnershipIncomplete(vec![format!(
            "page-resource-read-failed:{error}"
        )])
    })?;
    let mut dictionaries = Vec::new();
    if let Some(direct) = direct {
        dictionaries.push(direct);
    }
    for resource_id in resource_ids {
        if let Ok(dictionary) = document.get_dictionary(resource_id) {
            if !dictionaries
                .iter()
                .any(|existing| std::ptr::eq(*existing, dictionary))
            {
                dictionaries.push(dictionary);
            }
        }
    }
    Ok(ResourceContext { dictionaries })
}

fn form_resource_context<'a>(document: &'a Document, stream: &'a Stream) -> ResourceContext<'a> {
    let mut dictionaries = Vec::new();
    if let Some(resources) = dictionary_entry(&stream.dict, b"Resources", document) {
        dictionaries.push(resources);
    }
    ResourceContext { dictionaries }
}

fn invoked_form_resource_context<'a>(
    document: &'a Document,
    stream: &'a Stream,
    parent: &ResourceContext<'a>,
) -> ResourceContext<'a> {
    let mut dictionaries = Vec::new();
    if let Some(resources) = dictionary_entry(&stream.dict, b"Resources", document) {
        dictionaries.push(resources);
    }
    for parent in &parent.dictionaries {
        if !dictionaries
            .iter()
            .any(|existing| std::ptr::eq(*existing, *parent))
        {
            dictionaries.push(parent);
        }
    }
    ResourceContext { dictionaries }
}

fn resolve_xobject<'a>(
    document: &'a Document,
    resources: &ResourceContext<'a>,
    resource_name: &[u8],
) -> Option<(Option<ObjectId>, &'a Stream)> {
    for resources in &resources.dictionaries {
        let Some(xobjects) = dictionary_entry(resources, b"XObject", document) else {
            continue;
        };
        let Ok(object) = xobjects.get(resource_name) else {
            continue;
        };
        match object {
            Object::Reference(object_id) => {
                if let Ok(stream) = document.get_object(*object_id).and_then(Object::as_stream) {
                    return Some((Some(*object_id), stream));
                }
            }
            Object::Stream(stream) => return Some((None, stream)),
            _ => {}
        }
    }
    None
}

fn xobjects_for_context<'a>(
    document: &'a Document,
    resources: &ResourceContext<'a>,
) -> BTreeMap<Vec<u8>, &'a Object> {
    let mut xobjects = BTreeMap::new();
    for resources in &resources.dictionaries {
        let Some(dictionary) = dictionary_entry(resources, b"XObject", document) else {
            continue;
        };
        for (name, object) in dictionary.iter() {
            xobjects.entry(name.clone()).or_insert(object);
        }
    }
    xobjects
}

fn dictionary_entry<'a>(
    dictionary: &'a Dictionary,
    key: &[u8],
    document: &'a Document,
) -> Option<&'a Dictionary> {
    dictionary
        .get(key)
        .ok()
        .and_then(|object| dereference_dictionary(object, document))
}

fn dereference_dictionary<'a>(
    object: &'a Object,
    document: &'a Document,
) -> Option<&'a Dictionary> {
    match object {
        Object::Dictionary(dictionary) => Some(dictionary),
        Object::Reference(id) => document.get_dictionary(*id).ok(),
        _ => None,
    }
}

fn content_stream_page_references(document: &Document) -> BTreeMap<ObjectId, BTreeSet<u32>> {
    let mut references = BTreeMap::<ObjectId, BTreeSet<u32>>::new();
    for (page_number, page_id) in document.get_pages() {
        for stream_id in document.get_page_contents(page_id) {
            references.entry(stream_id).or_default().insert(page_number);
        }
    }
    references
}

fn text_operand_bytes_mut(
    operation: &mut lopdf::content::Operation,
    operand_index: usize,
    array_index: Option<usize>,
) -> Option<&mut Vec<u8>> {
    match array_index {
        Some(array_index) => operation
            .operands
            .get_mut(operand_index)
            .and_then(|operand| operand.as_array_mut().ok())
            .and_then(|items| items.get_mut(array_index))
            .and_then(|item| match item {
                Object::String(bytes, _) => Some(bytes),
                _ => None,
            }),
        None => operation
            .operands
            .get_mut(operand_index)
            .and_then(|operand| match operand {
                Object::String(bytes, _) => Some(bytes),
                _ => None,
            }),
    }
}

fn byte_hash(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path};

    use lopdf::{
        content::{Content, Operation},
        Dictionary, Document, Object, ObjectId, Stream,
    };

    use super::{
        apply_content_operand_patches, byte_hash, ContentOperandRangePatch, ContentPatchError,
    };
    use crate::{
        pdf_v3::{
            identity::{compare_images, render_page},
            mapping::map_page_atoms_to_content_operands,
            types::FormInvocationStep,
        },
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    #[test]
    fn unique_form_identity_patch_is_text_and_pixel_exact() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("2305.13048v2.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let patch = identity_patch_for_stream(&document, 1, (24, 0));

        let result =
            apply_content_operand_patches(&mut document, 1, &[patch]).expect("identity patch");
        assert_eq!(result.patch_count, 1);
        assert_eq!(result.modified_stream_count, 1);
        println!(
            "pdf-v3 targeted identity patches={} streams={} source_bytes={} replacement_bytes={} elapsed={}ms",
            result.patch_count,
            result.modified_stream_count,
            result.replaced_source_bytes,
            result.replacement_bytes,
            result.elapsed_ms
        );

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save patched document");
        assert_pdfium_page_identity(&source_path, &output, 1);
        if let Ok(path) = env::var("ROSETTA_PDF_V3_PATCH_OUTPUT") {
            fs::write(path, &output).expect("write targeted patch output");
        }
    }

    #[test]
    fn shared_form_patch_requires_structured_invocation_path() {
        let source_path = fixture_path("2305.13048v2.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let patch = ContentOperandRangePatch {
            page_number: 1,
            stream_object_number: 59,
            stream_generation: 0,
            operation_index: 0,
            operand_index: 0,
            array_index: None,
            encoded_start: 0,
            encoded_len: 0,
            expected_operand_byte_count: 0,
            expected_operand_hash: byte_hash(&[]),
            replacement_bytes: Vec::new(),
            form_invocation_path: Vec::new(),
        };

        let error = apply_content_operand_patches(&mut document, 1, &[patch])
            .expect_err("shared Form must be rejected");
        assert!(matches!(
            error,
            ContentPatchError::FormInvocationPathRequired {
                object_number: 59,
                generation: 0,
            }
        ));
    }

    #[test]
    fn cross_page_form_reachability_requires_structured_invocation_path() {
        let source_path = fixture_path("2305.13048v2.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let patch = identity_patch_for_stream(&document, 1, (24, 0));
        let second_page_id = document.get_pages()[&2];
        let mut xobjects = Dictionary::new();
        xobjects.set("UnusedTarget", Object::Reference((24, 0)));
        let mut resources = Dictionary::new();
        resources.set("XObject", Object::Dictionary(xobjects));
        document
            .get_object_mut(second_page_id)
            .and_then(Object::as_dict_mut)
            .expect("second page dictionary")
            .set("Resources", Object::Dictionary(resources));

        let error = apply_content_operand_patches(&mut document, 1, &[patch])
            .expect_err("cross-page resource reachability must be rejected");
        assert!(matches!(
            error,
            ContentPatchError::FormInvocationPathRequired {
                object_number: 24,
                generation: 0,
            }
        ));
    }

    #[test]
    fn shared_form_identity_patch_clones_one_invocation_chain() {
        let _guard = pdfium_test_lock();
        let (mut document, source_bytes, page_id, root_stream_id, form_stream_id) =
            repeated_form_document();
        let original_form_content = document
            .get_object(form_stream_id)
            .and_then(Object::as_stream)
            .expect("original Form")
            .content
            .clone();
        let mut patch = identity_patch_for_stream(&document, 1, form_stream_id);
        patch.form_invocation_path = vec![FormInvocationStep {
            parent_stream_object_number: root_stream_id.0,
            parent_stream_generation: root_stream_id.1,
            operation_index: 1,
            form_stream_object_number: form_stream_id.0,
            form_stream_generation: form_stream_id.1,
        }];

        let result = apply_content_operand_patches(&mut document, 1, &[patch])
            .expect("shared Form copy-on-write");
        assert_eq!(result.cloned_stream_count, 2);
        assert!(result.page_content_rewired);
        assert_eq!(result.modified_stream_count, 2);
        assert_eq!(
            document
                .get_object(form_stream_id)
                .and_then(Object::as_stream)
                .expect("unchanged source Form")
                .content,
            original_form_content
        );

        let discovery = super::discover_page_streams(&document, page_id, 1)
            .expect("discover copy-on-write streams");
        assert_eq!(
            discovery
                .streams
                .get(&form_stream_id)
                .expect("original Form remains reachable")
                .form_invocation_paths
                .len(),
            1
        );
        assert_eq!(
            discovery
                .streams
                .iter()
                .filter(|(stream_id, stream)| {
                    **stream_id != form_stream_id
                        && stream.is_form_xobject
                        && stream.form_invocation_paths.len() == 1
                })
                .count(),
            1
        );

        let mut output = Vec::new();
        document.save_to(&mut output).expect("save COW output");
        assert_pdfium_bytes_identity(&source_bytes, &output, 1);
    }

    #[test]
    fn shared_form_identity_batch_merges_the_common_root_clone() {
        let _guard = pdfium_test_lock();
        let (mut document, source_bytes, page_id, root_stream_id, form_stream_id) =
            repeated_form_document();
        let before_max_id = document.max_id;
        let original_root_content = document
            .get_object(root_stream_id)
            .and_then(Object::as_stream)
            .expect("original root stream")
            .content
            .clone();
        let original_form_content = document
            .get_object(form_stream_id)
            .and_then(Object::as_stream)
            .expect("original Form")
            .content
            .clone();
        let mut first = identity_patch_for_stream(&document, 1, form_stream_id);
        first.form_invocation_path = vec![FormInvocationStep {
            parent_stream_object_number: root_stream_id.0,
            parent_stream_generation: root_stream_id.1,
            operation_index: 1,
            form_stream_object_number: form_stream_id.0,
            form_stream_generation: form_stream_id.1,
        }];
        let mut second = first.clone();
        second.form_invocation_path[0].operation_index = 4;

        let result = apply_content_operand_patches(&mut document, 1, &[first, second])
            .expect("multi-invocation Form copy-on-write");
        assert_eq!(result.cloned_stream_count, 3);
        assert_eq!(result.modified_stream_count, 3);
        assert!(result.page_content_rewired);
        assert_eq!(document.max_id, before_max_id + 3);
        assert_eq!(
            document
                .get_object(root_stream_id)
                .and_then(Object::as_stream)
                .expect("unchanged source root")
                .content,
            original_root_content
        );
        assert_eq!(
            document
                .get_object(form_stream_id)
                .and_then(Object::as_stream)
                .expect("unchanged source Form")
                .content,
            original_form_content
        );

        let discovery = super::discover_page_streams(&document, page_id, 1)
            .expect("discover merged copy-on-write tree");
        assert!(!discovery.streams.contains_key(&form_stream_id));
        assert_eq!(
            discovery
                .streams
                .values()
                .filter(|stream| stream.is_form_xobject)
                .count(),
            2
        );

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save merged COW output");
        assert_pdfium_bytes_identity(&source_bytes, &output, 1);
    }

    #[test]
    fn invalid_second_form_target_keeps_multi_target_batch_atomic() {
        let (mut document, _, _, root_stream_id, form_stream_id) = repeated_form_document();
        let before = document.clone();
        let mut first = identity_patch_for_stream(&document, 1, form_stream_id);
        first.form_invocation_path = vec![FormInvocationStep {
            parent_stream_object_number: root_stream_id.0,
            parent_stream_generation: root_stream_id.1,
            operation_index: 1,
            form_stream_object_number: form_stream_id.0,
            form_stream_generation: form_stream_id.1,
        }];
        let mut invalid = first.clone();
        invalid.form_invocation_path[0].operation_index = 0;

        let error = apply_content_operand_patches(&mut document, 1, &[first, invalid])
            .expect_err("invalid second invocation must reject the complete batch");
        assert!(matches!(
            error,
            ContentPatchError::FormInvocationPathInvalid(_)
        ));
        assert_eq!(document.max_id, before.max_id);
        assert_eq!(document.objects, before.objects);
    }

    #[test]
    fn nested_form_identity_batch_merges_every_common_ancestor() {
        let _guard = pdfium_test_lock();
        let (mut document, source_bytes, page_id, root_stream_id, parent_form_id, leaf_form_id) =
            nested_repeated_form_document();
        let before_max_id = document.max_id;
        let mut first = identity_patch_for_stream(&document, 1, leaf_form_id);
        first.form_invocation_path = vec![
            FormInvocationStep {
                parent_stream_object_number: root_stream_id.0,
                parent_stream_generation: root_stream_id.1,
                operation_index: 1,
                form_stream_object_number: parent_form_id.0,
                form_stream_generation: parent_form_id.1,
            },
            FormInvocationStep {
                parent_stream_object_number: parent_form_id.0,
                parent_stream_generation: parent_form_id.1,
                operation_index: 1,
                form_stream_object_number: leaf_form_id.0,
                form_stream_generation: leaf_form_id.1,
            },
        ];
        let mut second = first.clone();
        second.form_invocation_path[1].operation_index = 4;

        let result = apply_content_operand_patches(&mut document, 1, &[first, second])
            .expect("nested multi-invocation Form copy-on-write");
        assert_eq!(result.cloned_stream_count, 4);
        assert_eq!(result.modified_stream_count, 4);
        assert!(result.page_content_rewired);
        assert_eq!(document.max_id, before_max_id + 4);

        let discovery = super::discover_page_streams(&document, page_id, 1)
            .expect("discover nested merged copy-on-write tree");
        assert!(!discovery.streams.contains_key(&parent_form_id));
        assert!(!discovery.streams.contains_key(&leaf_form_id));
        assert_eq!(
            discovery
                .streams
                .values()
                .filter(|stream| stream.is_form_xobject)
                .count(),
            3
        );

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save nested merged COW output");
        assert_pdfium_bytes_identity(&source_bytes, &output, 1);
        if let Ok(path) = env::var("ROSETTA_PDF_V3_MULTI_COW_SOURCE_OUTPUT") {
            fs::write(path, &source_bytes).expect("write nested multi-COW source");
        }
        if let Ok(path) = env::var("ROSETTA_PDF_V3_MULTI_COW_OUTPUT") {
            fs::write(path, &output).expect("write nested multi-COW output");
        }
    }

    #[test]
    fn cross_page_reachable_form_clones_selected_invocation_chain() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("2305.13048v2.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("recursive mapping");
        let invocation_path = mapping
            .text_shows
            .iter()
            .find(|show| show.stream_object_number == 24 && show.stream_generation == 0)
            .expect("target Form text show")
            .form_invocation_path
            .clone();
        assert!(!invocation_path.is_empty());

        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let second_page_id = document.get_pages()[&2];
        let mut second_page_resources = {
            let context = super::page_resource_context(&document, second_page_id)
                .expect("second page resources");
            super::materialize_resource_context(&document, &context)
        };
        super::insert_xobject_alias(&mut second_page_resources, (24, 0));
        document
            .get_object_mut(second_page_id)
            .and_then(Object::as_dict_mut)
            .expect("second page dictionary")
            .set("Resources", Object::Dictionary(second_page_resources));
        let mut baseline_document = document.clone();
        let mut baseline = Vec::new();
        baseline_document
            .save_to(&mut baseline)
            .expect("save cross-page Form baseline");
        let original_content = document
            .get_object((24, 0))
            .and_then(Object::as_stream)
            .expect("source Form")
            .content
            .clone();
        let mut patch = identity_patch_for_stream(&document, 1, (24, 0));
        patch.form_invocation_path = invocation_path.clone();

        let result = apply_content_operand_patches(&mut document, 1, &[patch])
            .expect("cross-page Form copy-on-write");
        assert_eq!(result.cloned_stream_count, invocation_path.len() + 1);
        assert!(result.page_content_rewired);
        assert_eq!(
            document
                .get_object((24, 0))
                .and_then(Object::as_stream)
                .expect("unchanged source Form")
                .content,
            original_content
        );

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save cross-page Form COW output");
        assert_pdfium_bytes_identity(&baseline, &output, 1);
        assert_pdfium_bytes_identity(&baseline, &output, 2);
        if let Ok(path) = env::var("ROSETTA_PDF_V3_COW_SOURCE_OUTPUT") {
            fs::write(path, &baseline).expect("write COW source output");
        }
        if let Ok(path) = env::var("ROSETTA_PDF_V3_COW_OUTPUT") {
            fs::write(path, &output).expect("write COW output");
        }
    }

    #[test]
    fn invalid_form_invocation_path_leaves_document_unchanged() {
        let (mut document, _, _, root_stream_id, form_stream_id) = repeated_form_document();
        let before = document.clone();
        let mut patch = identity_patch_for_stream(&document, 1, form_stream_id);
        patch.form_invocation_path = vec![FormInvocationStep {
            parent_stream_object_number: root_stream_id.0,
            parent_stream_generation: root_stream_id.1,
            operation_index: 0,
            form_stream_object_number: form_stream_id.0,
            form_stream_generation: form_stream_id.1,
        }];

        let error = apply_content_operand_patches(&mut document, 1, &[patch])
            .expect_err("invalid invocation path must fail");
        assert!(matches!(
            error,
            ContentPatchError::FormInvocationPathInvalid(_)
        ));
        assert_eq!(document.max_id, before.max_id);
        assert_eq!(document.objects, before.objects);
    }

    #[test]
    fn shared_page_stream_identity_patch_rewires_only_selected_page() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("2305.13048v2.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let pages = document.get_pages();
        let first_page_id = pages[&1];
        let second_page_id = pages[&2];
        let shared_stream_id = document.get_page_contents(first_page_id)[0];
        let first_page_resources = {
            let context = super::page_resource_context(&document, first_page_id)
                .expect("first page resources");
            super::materialize_resource_context(&document, &context)
        };
        document
            .get_object_mut(second_page_id)
            .and_then(Object::as_dict_mut)
            .expect("second page dictionary")
            .set("Resources", Object::Dictionary(first_page_resources));
        document
            .get_object_mut(second_page_id)
            .and_then(Object::as_dict_mut)
            .expect("second page dictionary")
            .set("Contents", Object::Reference(shared_stream_id));
        let mut baseline_document = document.clone();
        let mut baseline = Vec::new();
        baseline_document
            .save_to(&mut baseline)
            .expect("save shared baseline");
        let original_content = document
            .get_object(shared_stream_id)
            .and_then(Object::as_stream)
            .expect("shared stream")
            .content
            .clone();
        let patch = identity_patch_for_stream(&document, 1, shared_stream_id);

        let result = apply_content_operand_patches(&mut document, 1, &[patch])
            .expect("shared page stream copy-on-write");
        assert_eq!(result.cloned_stream_count, 1);
        assert!(result.page_content_rewired);
        assert_ne!(
            document.get_page_contents(first_page_id)[0],
            shared_stream_id
        );
        assert_eq!(
            document.get_page_contents(second_page_id),
            [shared_stream_id]
        );
        assert_eq!(
            document
                .get_object(shared_stream_id)
                .and_then(Object::as_stream)
                .expect("unchanged shared stream")
                .content,
            original_content
        );

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save shared COW output");
        assert_pdfium_bytes_identity(&baseline, &output, 1);
        assert_pdfium_bytes_identity(&baseline, &output, 2);
    }

    #[test]
    fn hash_mismatch_does_not_mutate_the_document() {
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let mut patch = first_page_identity_patch(&document, 1);
        let stream_id = patch.stream_id();
        let before = document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .expect("source stream")
            .content
            .clone();
        patch.expected_operand_hash = "sha256-mismatch".to_string();

        let error = apply_content_operand_patches(&mut document, 1, &[patch])
            .expect_err("hash mismatch must fail");
        assert!(matches!(
            error,
            ContentPatchError::OperandHashMismatch { .. }
        ));
        let after = &document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .expect("unchanged stream")
            .content;
        assert_eq!(&before, after);
    }

    #[test]
    fn overlapping_ranges_do_not_mutate_the_document() {
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let patch = first_page_identity_patch_with_min_len(&document, 1, 2);
        let mut first = patch.clone();
        first.encoded_start = 0;
        first.encoded_len = 2;
        first.replacement_bytes = first.replacement_bytes[..2].to_vec();
        let mut second = patch;
        second.encoded_start = 1;
        second.encoded_len = 1;
        second.replacement_bytes = second.replacement_bytes[1..2].to_vec();
        let stream_id = first.stream_id();
        let before = document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .expect("source stream")
            .content
            .clone();

        let error = apply_content_operand_patches(&mut document, 1, &[first, second])
            .expect_err("overlap must fail");
        assert!(matches!(error, ContentPatchError::OverlappingRanges { .. }));
        let after = &document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .expect("unchanged stream")
            .content;
        assert_eq!(&before, after);
    }

    #[test]
    fn out_of_bounds_range_does_not_mutate_the_document() {
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let mut patch = first_page_identity_patch(&document, 1);
        let stream_id = patch.stream_id();
        let before = document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .expect("source stream")
            .content
            .clone();
        patch.encoded_start = patch.expected_operand_byte_count;
        patch.encoded_len = 1;
        patch.replacement_bytes = vec![0];

        let error = apply_content_operand_patches(&mut document, 1, &[patch])
            .expect_err("out-of-bounds range must fail");
        assert!(matches!(error, ContentPatchError::RangeOutOfBounds { .. }));
        let after = &document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .expect("unchanged stream")
            .content;
        assert_eq!(&before, after);
    }

    #[test]
    fn later_stream_failure_keeps_all_streams_unmodified() {
        let source_path = fixture_path("2305.13048v2.pdf");
        let source_bytes = std::fs::read(&source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let page_id = document.get_pages()[&1];
        let stream_ids = document.get_page_contents(page_id);
        assert_eq!(stream_ids.len(), 2);
        let first = identity_patch_for_stream(&document, 1, stream_ids[0]);
        let mut second = identity_patch_for_stream(&document, 1, stream_ids[1]);
        second.expected_operand_hash = "sha256-mismatch".to_string();
        let before = stream_ids
            .iter()
            .map(|stream_id| {
                (
                    *stream_id,
                    document
                        .get_object(*stream_id)
                        .and_then(Object::as_stream)
                        .expect("source stream")
                        .content
                        .clone(),
                )
            })
            .collect::<Vec<_>>();

        let error = apply_content_operand_patches(&mut document, 1, &[first, second])
            .expect_err("later stream failure must abort the transaction");
        assert!(matches!(
            error,
            ContentPatchError::OperandHashMismatch { .. }
        ));
        for (stream_id, expected) in before {
            let actual = &document
                .get_object(stream_id)
                .and_then(Object::as_stream)
                .expect("unchanged stream")
                .content;
            assert_eq!(&expected, actual);
        }
    }

    fn repeated_form_document() -> (Document, Vec<u8>, ObjectId, ObjectId, ObjectId) {
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let source_bytes = std::fs::read(source_path).expect("source bytes");
        let mut document = Document::load_mem(&source_bytes).expect("source document");
        let page_id = document.get_pages()[&1];
        let page_resources = {
            let context = super::page_resource_context(&document, page_id).expect("page resources");
            super::materialize_resource_context(&document, &context)
        };
        let mut form_operations = Vec::new();
        for stream_id in document.get_page_contents(page_id) {
            let stream = document
                .get_object(stream_id)
                .and_then(Object::as_stream)
                .expect("page content stream");
            let source = stream.get_plain_content().expect("page content");
            form_operations.extend(
                Content::decode(&source)
                    .expect("decoded page content")
                    .operations,
            );
        }
        let form_content = Content {
            operations: form_operations,
        }
        .encode()
        .expect("encoded Form content");
        let mut form_dictionary = Dictionary::new();
        form_dictionary.set("Type", Object::Name(b"XObject".to_vec()));
        form_dictionary.set("Subtype", Object::Name(b"Form".to_vec()));
        form_dictionary.set("FormType", Object::Integer(1));
        form_dictionary.set(
            "BBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(1000),
                Object::Integer(1000),
            ]),
        );
        form_dictionary.set("Resources", Object::Dictionary(page_resources.clone()));
        let mut form_stream = Stream::new(form_dictionary, form_content);
        form_stream.compress().expect("compressed Form stream");
        let form_stream_id = document.add_object(form_stream);

        let mut page_resources = page_resources;
        let mut xobjects = page_resources
            .get(b"XObject")
            .ok()
            .and_then(|object| object.as_dict().ok())
            .cloned()
            .unwrap_or_default();
        xobjects.set("SharedForm", Object::Reference(form_stream_id));
        page_resources.set("XObject", Object::Dictionary(xobjects));
        document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page dictionary")
            .set("Resources", Object::Dictionary(page_resources));

        let root_content = Content {
            operations: vec![
                Operation::new("q", Vec::new()),
                Operation::new("Do", vec![Object::Name(b"SharedForm".to_vec())]),
                Operation::new("Q", Vec::new()),
                Operation::new("q", Vec::new()),
                Operation::new("Do", vec![Object::Name(b"SharedForm".to_vec())]),
                Operation::new("Q", Vec::new()),
            ],
        }
        .encode()
        .expect("encoded root content");
        let mut root_stream = Stream::new(Dictionary::new(), root_content);
        root_stream.compress().expect("compressed root stream");
        let root_stream_id = document.add_object(root_stream);
        document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page dictionary")
            .set("Contents", Object::Reference(root_stream_id));

        let mut baseline_document = document.clone();
        let mut baseline = Vec::new();
        baseline_document
            .save_to(&mut baseline)
            .expect("save repeated Form baseline");
        (document, baseline, page_id, root_stream_id, form_stream_id)
    }

    fn nested_repeated_form_document() -> (Document, Vec<u8>, ObjectId, ObjectId, ObjectId, ObjectId)
    {
        let (mut document, _, page_id, parent_form_id, leaf_form_id) = repeated_form_document();
        let mut page_resources = {
            let context = super::page_resource_context(&document, page_id).expect("page resources");
            super::materialize_resource_context(&document, &context)
        };
        {
            let parent_form = document
                .get_object_mut(parent_form_id)
                .and_then(Object::as_stream_mut)
                .expect("parent Form stream");
            parent_form
                .dict
                .set("Type", Object::Name(b"XObject".to_vec()));
            parent_form
                .dict
                .set("Subtype", Object::Name(b"Form".to_vec()));
            parent_form.dict.set("FormType", Object::Integer(1));
            parent_form.dict.set(
                "BBox",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(1000),
                    Object::Integer(1000),
                ]),
            );
            parent_form
                .dict
                .set("Resources", Object::Dictionary(page_resources.clone()));
        }
        let mut xobjects = page_resources
            .get(b"XObject")
            .ok()
            .and_then(|object| object.as_dict().ok())
            .cloned()
            .unwrap_or_default();
        xobjects.set("ParentForm", Object::Reference(parent_form_id));
        page_resources.set("XObject", Object::Dictionary(xobjects));
        document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page dictionary")
            .set("Resources", Object::Dictionary(page_resources));

        let root_content = Content {
            operations: vec![
                Operation::new("q", Vec::new()),
                Operation::new("Do", vec![Object::Name(b"ParentForm".to_vec())]),
                Operation::new("Q", Vec::new()),
            ],
        }
        .encode()
        .expect("encoded nested root content");
        let mut root_stream = Stream::new(Dictionary::new(), root_content);
        root_stream.compress().expect("compressed nested root");
        let root_stream_id = document.add_object(root_stream);
        document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page dictionary")
            .set("Contents", Object::Reference(root_stream_id));

        let mut baseline_document = document.clone();
        let mut baseline = Vec::new();
        baseline_document
            .save_to(&mut baseline)
            .expect("save nested repeated Form baseline");
        (
            document,
            baseline,
            page_id,
            root_stream_id,
            parent_form_id,
            leaf_form_id,
        )
    }

    fn first_page_identity_patch(
        document: &Document,
        page_number: u32,
    ) -> ContentOperandRangePatch {
        first_page_identity_patch_with_min_len(document, page_number, 0)
    }

    fn first_page_identity_patch_with_min_len(
        document: &Document,
        page_number: u32,
        minimum_len: usize,
    ) -> ContentOperandRangePatch {
        let page_id = document.get_pages()[&page_number];
        let stream_id = document.get_page_contents(page_id)[0];
        identity_patch_for_stream_with_min_len(document, page_number, stream_id, minimum_len)
    }

    fn identity_patch_for_stream(
        document: &Document,
        page_number: u32,
        stream_id: ObjectId,
    ) -> ContentOperandRangePatch {
        identity_patch_for_stream_with_min_len(document, page_number, stream_id, 0)
    }

    fn identity_patch_for_stream_with_min_len(
        document: &Document,
        page_number: u32,
        stream_id: ObjectId,
        minimum_len: usize,
    ) -> ContentOperandRangePatch {
        let stream = document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .expect("content stream");
        let source = stream.get_plain_content().expect("plain stream");
        let content = Content::decode(&source).expect("decoded content");
        for (operation_index, operation) in content.operations.iter().enumerate() {
            match operation.operator.as_str() {
                "Tj" | "'" => {
                    if let Some(bytes) = operation
                        .operands
                        .first()
                        .and_then(|value| value.as_str().ok())
                        .filter(|bytes| bytes.len() >= minimum_len)
                    {
                        return patch_for_bytes(
                            page_number,
                            stream_id,
                            operation_index,
                            0,
                            None,
                            bytes,
                        );
                    }
                }
                "\"" => {
                    if let Some(bytes) = operation
                        .operands
                        .get(2)
                        .and_then(|value| value.as_str().ok())
                        .filter(|bytes| bytes.len() >= minimum_len)
                    {
                        return patch_for_bytes(
                            page_number,
                            stream_id,
                            operation_index,
                            2,
                            None,
                            bytes,
                        );
                    }
                }
                "TJ" => {
                    if let Some((array_index, bytes)) = operation
                        .operands
                        .first()
                        .and_then(|value| value.as_array().ok())
                        .and_then(|items| {
                            items.iter().enumerate().find_map(|(index, item)| {
                                item.as_str()
                                    .ok()
                                    .filter(|bytes| bytes.len() >= minimum_len)
                                    .map(|bytes| (index, bytes))
                            })
                        })
                    {
                        return patch_for_bytes(
                            page_number,
                            stream_id,
                            operation_index,
                            0,
                            Some(array_index),
                            bytes,
                        );
                    }
                }
                _ => {}
            }
        }
        panic!("stream {stream_id:?} has no text operand")
    }

    fn patch_for_bytes(
        page_number: u32,
        stream_id: ObjectId,
        operation_index: usize,
        operand_index: usize,
        array_index: Option<usize>,
        bytes: &[u8],
    ) -> ContentOperandRangePatch {
        ContentOperandRangePatch {
            page_number,
            stream_object_number: stream_id.0,
            stream_generation: stream_id.1,
            operation_index,
            operand_index,
            array_index,
            encoded_start: 0,
            encoded_len: bytes.len(),
            expected_operand_byte_count: bytes.len(),
            expected_operand_hash: byte_hash(bytes),
            replacement_bytes: bytes.to_vec(),
            form_invocation_path: Vec::new(),
        }
    }

    fn assert_pdfium_page_identity(source_path: &Path, output: &[u8], page_number: u32) {
        let source = std::fs::read(source_path).expect("source PDF bytes");
        assert_pdfium_bytes_identity(&source, output, page_number);
    }

    fn assert_pdfium_bytes_identity(source: &[u8], output: &[u8], page_number: u32) {
        let pdfium = shared_pdfium();
        let source = pdfium
            .load_pdf_from_byte_slice(source, None)
            .expect("source PDFium document");
        let output = pdfium
            .load_pdf_from_byte_slice(output, None)
            .expect("output PDFium document");
        let source_page = source
            .pages()
            .get(page_number as i32 - 1)
            .expect("source page");
        let output_page = output
            .pages()
            .get(page_number as i32 - 1)
            .expect("output page");
        assert_eq!(
            source_page.text().expect("source text").all(),
            output_page.text().expect("output text").all()
        );
        let source_image = render_page(&source_page, page_number, 900).expect("source image");
        let output_image = render_page(&output_page, page_number, 900).expect("output image");
        let difference = compare_images(&source_image, &output_image).expect("image difference");
        assert_eq!(difference.changed_pixel_count, 0);
        assert_eq!(difference.max_channel_difference, 0);
    }
}
