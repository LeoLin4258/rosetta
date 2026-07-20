use std::{
    fmt,
    fs::File,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Mutex,
};

use ::pdf::{
    backend::Backend,
    file::{File as PdfFile, FileOptions, NoCache, NoLog},
    object::{PlainRef, Resolve},
    primitive::{PdfString, Primitive},
    PdfError,
};
use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat};
use lru::LruCache;
use memmap2::{Mmap, MmapOptions};

use super::object_delta::PdfObjectDelta;

const DEFAULT_CACHE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_CACHE_ENTRIES: usize = 512;
const DEFAULT_MAX_CACHED_OBJECT_BYTES: usize = 4 * 1024 * 1024;

type UncachedPdfFile = PdfFile<Mmap, NoCache, NoCache, NoLog>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PdfSourceObjectCachePolicy {
    pub maximum_bytes: usize,
    pub maximum_entries: usize,
    pub maximum_object_bytes: usize,
}

impl Default for PdfSourceObjectCachePolicy {
    fn default() -> Self {
        Self {
            maximum_bytes: DEFAULT_CACHE_BYTES,
            maximum_entries: DEFAULT_CACHE_ENTRIES,
            maximum_object_bytes: DEFAULT_MAX_CACHED_OBJECT_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PdfSourceObjectCacheStats {
    pub source_loads: u64,
    pub cache_hits: u64,
    pub resident_entries: usize,
    pub resident_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PdfSourceObjectError {
    InvalidCachePolicy,
    EmptySource,
    InvalidObjectId(ObjectId),
    ObjectNumberOverflow(u64),
    GenerationOverflow(u64),
    MaximumObjectNumberInvalid(i32),
    EncryptedDocument,
    Open { path: PathBuf, message: String },
    Map { path: PathBuf, message: String },
    Parse(String),
    CachePoisoned,
}

impl fmt::Display for PdfSourceObjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCachePolicy => {
                formatter.write_str("PDF source object cache policy is invalid")
            }
            Self::EmptySource => formatter.write_str("PDF source object file is empty"),
            Self::InvalidObjectId((number, generation)) => {
                write!(
                    formatter,
                    "PDF source object ID {number} {generation} is invalid"
                )
            }
            Self::ObjectNumberOverflow(number) => {
                write!(formatter, "PDF source object number {number} exceeds u32")
            }
            Self::GenerationOverflow(generation) => {
                write!(formatter, "PDF source generation {generation} exceeds u16")
            }
            Self::MaximumObjectNumberInvalid(size) => {
                write!(formatter, "PDF source trailer size {size} is invalid")
            }
            Self::EncryptedDocument => {
                formatter.write_str("encrypted PDFs are not supported by the lazy object reader")
            }
            Self::Open { path, message } => {
                write!(
                    formatter,
                    "failed to open PDF source {}: {message}",
                    path.display()
                )
            }
            Self::Map { path, message } => {
                write!(
                    formatter,
                    "failed to map PDF source {}: {message}",
                    path.display()
                )
            }
            Self::Parse(message) => {
                write!(formatter, "failed to parse PDF source object: {message}")
            }
            Self::CachePoisoned => formatter.write_str("PDF source object cache lock is poisoned"),
        }
    }
}

impl std::error::Error for PdfSourceObjectError {}

pub(crate) trait PdfObjectView {
    fn maximum_object_number(&self) -> u32;

    fn trailer(&self) -> &Dictionary;

    fn object(&self, object_id: ObjectId) -> Result<Object, PdfSourceObjectError>;
}

pub(crate) struct PdfSourceObjectStore {
    source_path: PathBuf,
    source_bytes: u64,
    previous_xref_offset: u64,
    maximum_object_number: u32,
    page_count: u32,
    trailer: Dictionary,
    file: UncachedPdfFile,
    cache: Mutex<PdfSourceObjectCache>,
}

impl PdfSourceObjectStore {
    pub(crate) fn open(source_path: impl AsRef<Path>) -> Result<Self, PdfSourceObjectError> {
        Self::open_with_cache_policy(source_path, PdfSourceObjectCachePolicy::default())
    }

    pub(crate) fn open_with_cache_policy(
        source_path: impl AsRef<Path>,
        cache_policy: PdfSourceObjectCachePolicy,
    ) -> Result<Self, PdfSourceObjectError> {
        if cache_policy.maximum_bytes == 0
            || cache_policy.maximum_entries == 0
            || cache_policy.maximum_object_bytes == 0
            || cache_policy.maximum_object_bytes > cache_policy.maximum_bytes
        {
            return Err(PdfSourceObjectError::InvalidCachePolicy);
        }
        let source_path = source_path.as_ref().to_path_buf();
        let source = File::open(&source_path).map_err(|error| PdfSourceObjectError::Open {
            path: source_path.clone(),
            message: error.to_string(),
        })?;
        let source_bytes = source
            .metadata()
            .map_err(|error| PdfSourceObjectError::Open {
                path: source_path.clone(),
                message: error.to_string(),
            })?
            .len();
        if source_bytes == 0 {
            return Err(PdfSourceObjectError::EmptySource);
        }

        // The map is read-only and owns the OS file mapping for the complete store lifetime.
        // Source identity is revalidated by the incremental writer before final commit.
        let source_map = unsafe { MmapOptions::new().map(&source) }.map_err(|error| {
            PdfSourceObjectError::Map {
                path: source_path.clone(),
                message: error.to_string(),
            }
        })?;
        let source_start = source_map
            .locate_start_offset()
            .map_err(|error| PdfSourceObjectError::Parse(error.to_string()))?;
        let previous_xref_offset = u64::try_from(
            source_map
                .locate_xref_offset()
                .map_err(|error| PdfSourceObjectError::Parse(error.to_string()))?,
        )
        .map_err(|_| PdfSourceObjectError::Parse("xref offset exceeds u64".to_string()))?;
        let file = FileOptions::uncached()
            .load(source_map)
            .map_err(|error| PdfSourceObjectError::Parse(error.to_string()))?;
        if file.trailer.encrypt_dict.is_some() {
            return Err(PdfSourceObjectError::EncryptedDocument);
        }
        // Xref streams may use an indirect /Length. Parse the raw trailer through
        // the initialized file resolver while a second read-only map is transiently
        // available; both mappings share the same OS-backed source pages.
        let trailer_map = unsafe { MmapOptions::new().map(&source) }.map_err(|error| {
            PdfSourceObjectError::Map {
                path: source_path.clone(),
                message: error.to_string(),
            }
        })?;
        let trailer = {
            let resolver = file.resolver();
            let (_, trailer) = trailer_map
                .read_xref_table_and_trailer(source_start, &resolver)
                .map_err(|error| PdfSourceObjectError::Parse(error.to_string()))?;
            dictionary_to_lopdf(trailer, &resolver)?
        };
        let maximum_object_number = file
            .trailer
            .size
            .checked_sub(1)
            .and_then(|number| u32::try_from(number).ok())
            .ok_or(PdfSourceObjectError::MaximumObjectNumberInvalid(
                file.trailer.size,
            ))?;
        let page_count = file.num_pages();

        Ok(Self {
            source_path,
            source_bytes,
            previous_xref_offset,
            maximum_object_number,
            page_count,
            trailer,
            file,
            cache: Mutex::new(PdfSourceObjectCache::new(cache_policy)),
        })
    }

    pub(crate) fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub(crate) fn source_bytes(&self) -> u64 {
        self.source_bytes
    }

    pub(crate) fn previous_xref_offset(&self) -> u64 {
        self.previous_xref_offset
    }

    pub(crate) fn page_count(&self) -> u32 {
        self.page_count
    }

    pub(crate) fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    pub(crate) fn cache_stats(&self) -> Result<PdfSourceObjectCacheStats, PdfSourceObjectError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| PdfSourceObjectError::CachePoisoned)?;
        Ok(cache.stats())
    }

    fn load_object(&self, object_id: ObjectId) -> Result<Object, PdfSourceObjectError> {
        validate_object_id(object_id)?;
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| PdfSourceObjectError::CachePoisoned)?;
            if let Some(cached) = cache.objects.get(&object_id) {
                let object = cached.object.clone();
                cache.cache_hits = cache.cache_hits.saturating_add(1);
                return Ok(object);
            }
            cache.source_loads = cache.source_loads.saturating_add(1);
        }

        let resolver = self.file.resolver();
        let primitive = resolve_indirect_primitive(resolver.resolve(PlainRef {
            id: u64::from(object_id.0),
            gen: u64::from(object_id.1),
        }))?;
        let object = primitive_to_lopdf(primitive, &resolver)?;
        let estimated_bytes = estimate_object_bytes(&object);

        let mut cache = self
            .cache
            .lock()
            .map_err(|_| PdfSourceObjectError::CachePoisoned)?;
        cache.insert(object_id, object.clone(), estimated_bytes);
        Ok(object)
    }
}

fn resolve_indirect_primitive(
    result: Result<Primitive, PdfError>,
) -> Result<Primitive, PdfSourceObjectError> {
    match result {
        Ok(primitive) => Ok(primitive),
        // ISO 32000 treats references to free or missing xref entries as null.
        Err(PdfError::FreeObject { .. } | PdfError::NullRef { .. }) => Ok(Primitive::Null),
        Err(error) => Err(PdfSourceObjectError::Parse(error.to_string())),
    }
}

impl PdfObjectView for PdfSourceObjectStore {
    fn maximum_object_number(&self) -> u32 {
        self.maximum_object_number
    }

    fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    fn object(&self, object_id: ObjectId) -> Result<Object, PdfSourceObjectError> {
        self.load_object(object_id)
    }
}

impl PdfObjectView for lopdf::Document {
    fn maximum_object_number(&self) -> u32 {
        self.max_id
    }

    fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    fn object(&self, object_id: ObjectId) -> Result<Object, PdfSourceObjectError> {
        lopdf::Document::get_object(self, object_id)
            .cloned()
            .map_err(|error| PdfSourceObjectError::Parse(error.to_string()))
    }
}

pub(crate) struct PdfObjectOverlay<'a> {
    source: &'a dyn PdfObjectView,
    delta: &'a PdfObjectDelta,
}

impl<'a> PdfObjectOverlay<'a> {
    pub(crate) fn new(source: &'a dyn PdfObjectView, delta: &'a PdfObjectDelta) -> Self {
        Self { source, delta }
    }
}

impl PdfObjectView for PdfObjectOverlay<'_> {
    fn maximum_object_number(&self) -> u32 {
        self.source
            .maximum_object_number()
            .max(self.delta.maximum_object_number())
    }

    fn trailer(&self) -> &Dictionary {
        self.source.trailer()
    }

    fn object(&self, object_id: ObjectId) -> Result<Object, PdfSourceObjectError> {
        if let Some(object) = self.delta.objects().get(&object_id) {
            Ok(object.clone())
        } else {
            self.source.object(object_id)
        }
    }
}

struct PdfSourceObjectCache {
    policy: PdfSourceObjectCachePolicy,
    objects: LruCache<ObjectId, CachedPdfObject>,
    resident_bytes: usize,
    source_loads: u64,
    cache_hits: u64,
}

impl PdfSourceObjectCache {
    fn new(policy: PdfSourceObjectCachePolicy) -> Self {
        let capacity = NonZeroUsize::new(policy.maximum_entries)
            .expect("validated non-zero PDF source object cache entries");
        Self {
            policy,
            objects: LruCache::new(capacity),
            resident_bytes: 0,
            source_loads: 0,
            cache_hits: 0,
        }
    }

    fn insert(&mut self, object_id: ObjectId, object: Object, estimated_bytes: usize) {
        if estimated_bytes > self.policy.maximum_object_bytes
            || estimated_bytes > self.policy.maximum_bytes
        {
            return;
        }
        while self.resident_bytes.saturating_add(estimated_bytes) > self.policy.maximum_bytes {
            let Some((_, removed)) = self.objects.pop_lru() else {
                break;
            };
            self.resident_bytes = self.resident_bytes.saturating_sub(removed.estimated_bytes);
        }
        if let Some((_, removed)) = self.objects.push(
            object_id,
            CachedPdfObject {
                object,
                estimated_bytes,
            },
        ) {
            self.resident_bytes = self.resident_bytes.saturating_sub(removed.estimated_bytes);
        }
        self.resident_bytes = self.resident_bytes.saturating_add(estimated_bytes);
    }

    fn stats(&self) -> PdfSourceObjectCacheStats {
        PdfSourceObjectCacheStats {
            source_loads: self.source_loads,
            cache_hits: self.cache_hits,
            resident_entries: self.objects.len(),
            resident_bytes: self.resident_bytes,
        }
    }
}

struct CachedPdfObject {
    object: Object,
    estimated_bytes: usize,
}

fn validate_object_id(object_id: ObjectId) -> Result<(), PdfSourceObjectError> {
    if object_id.0 == 0 || object_id.1 == u16::MAX {
        Err(PdfSourceObjectError::InvalidObjectId(object_id))
    } else {
        Ok(())
    }
}

fn primitive_to_lopdf(
    primitive: Primitive,
    resolver: &impl Resolve,
) -> Result<Object, PdfSourceObjectError> {
    match primitive {
        Primitive::Null => Ok(Object::Null),
        Primitive::Integer(value) => Ok(Object::Integer(i64::from(value))),
        Primitive::Number(value) => Ok(Object::Real(value)),
        Primitive::Boolean(value) => Ok(Object::Boolean(value)),
        Primitive::String(value) => Ok(pdf_string_to_lopdf(value)),
        Primitive::Array(values) => values
            .into_iter()
            .map(|value| primitive_to_lopdf(value, resolver))
            .collect::<Result<Vec<_>, _>>()
            .map(Object::Array),
        Primitive::Dictionary(dictionary) => {
            dictionary_to_lopdf(dictionary, resolver).map(Object::Dictionary)
        }
        Primitive::Reference(reference) => {
            let number = u32::try_from(reference.id)
                .map_err(|_| PdfSourceObjectError::ObjectNumberOverflow(reference.id))?;
            let generation = u16::try_from(reference.gen)
                .map_err(|_| PdfSourceObjectError::GenerationOverflow(reference.gen))?;
            Ok(Object::Reference((number, generation)))
        }
        Primitive::Name(name) => Ok(Object::Name(name.as_str().as_bytes().to_vec())),
        Primitive::Stream(stream) => {
            let dictionary = dictionary_to_lopdf(stream.info.clone(), resolver)?;
            let content = stream
                .raw_data(resolver)
                .map_err(|error| PdfSourceObjectError::Parse(error.to_string()))?
                .to_vec();
            Ok(Object::Stream(Stream::new(dictionary, content)))
        }
    }
}

fn dictionary_to_lopdf(
    dictionary: ::pdf::primitive::Dictionary,
    resolver: &impl Resolve,
) -> Result<Dictionary, PdfSourceObjectError> {
    let mut converted = Dictionary::new();
    for (key, value) in dictionary.iter() {
        converted.set(
            key.as_str().as_bytes().to_vec(),
            primitive_to_lopdf(value.clone(), resolver)?,
        );
    }
    Ok(converted)
}

fn pdf_string_to_lopdf(value: PdfString) -> Object {
    let bytes = value.data.as_slice().to_vec();
    let format = if bytes.iter().all(|byte| (0x20..0x80).contains(byte)) {
        StringFormat::Literal
    } else {
        StringFormat::Hexadecimal
    };
    Object::String(bytes, format)
}

fn estimate_object_bytes(object: &Object) -> usize {
    match object {
        Object::Null | Object::Boolean(_) | Object::Integer(_) | Object::Real(_) => {
            std::mem::size_of::<Object>()
        }
        Object::Name(bytes) | Object::String(bytes, _) => {
            std::mem::size_of::<Object>().saturating_add(bytes.len())
        }
        Object::Array(values) => values
            .iter()
            .fold(std::mem::size_of::<Object>(), |total, value| {
                total.saturating_add(estimate_object_bytes(value))
            }),
        Object::Dictionary(dictionary) => estimate_dictionary_bytes(dictionary),
        Object::Stream(stream) => estimate_dictionary_bytes(&stream.dict)
            .saturating_add(stream.content.len())
            .saturating_add(std::mem::size_of::<Object>()),
        Object::Reference(_) => std::mem::size_of::<Object>(),
    }
}

fn estimate_dictionary_bytes(dictionary: &Dictionary) -> usize {
    dictionary
        .iter()
        .fold(std::mem::size_of::<Dictionary>(), |total, (key, value)| {
            total
                .saturating_add(key.len())
                .saturating_add(estimate_object_bytes(value))
        })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Instant};

    use ::pdf::{primitive::Primitive, PdfError};
    use lopdf::{xref::XrefEntry, Document, Object, ObjectStream};

    use super::{
        resolve_indirect_primitive, PdfObjectOverlay, PdfObjectView, PdfSourceObjectCachePolicy,
        PdfSourceObjectStore,
    };
    use crate::{
        pdf_v3::object_delta::PdfObjectDelta,
        rosetta_jobs::formats::pdf::test_helpers::fixture_path,
    };

    #[test]
    fn reads_normal_and_compressed_real_pdf_objects_with_a_bounded_cache() {
        let source_path = fixture_path("2305.13048v2.pdf");
        let policy = PdfSourceObjectCachePolicy {
            maximum_bytes: 256 * 1024,
            maximum_entries: 8,
            maximum_object_bytes: 128 * 1024,
        };
        let open_started = Instant::now();
        let source = PdfSourceObjectStore::open_with_cache_policy(&source_path, policy)
            .expect("lazy source object store");
        let open_elapsed = open_started.elapsed();
        let expected = Document::load(&source_path).expect("lopdf reference document");
        let page_id = *expected.get_pages().get(&1).expect("page 1 ID");
        let content_id = expected
            .get_page_contents(page_id)
            .into_iter()
            .next()
            .expect("page 1 content stream");
        let compressed_id = expected
            .objects
            .values()
            .filter_map(|object| object.as_stream().ok())
            .find_map(|stream| {
                if !stream.dict.type_is(b"ObjStm") {
                    return None;
                }
                let mut stream = stream.clone();
                ObjectStream::new(&mut stream)
                    .ok()?
                    .objects
                    .keys()
                    .next()
                    .copied()
            })
            .expect("fixture compressed object");
        assert!(matches!(
            expected.reference_table.get(content_id.0),
            Some(XrefEntry::Normal { .. })
        ));

        let read_started = Instant::now();
        let page = source.object(page_id).expect("lazy page object");
        let content = source.object(content_id).expect("lazy content object");
        let compressed = source
            .object(compressed_id)
            .expect("lazy compressed object");
        let read_elapsed = read_started.elapsed();
        assert_eq!(
            page,
            expected.get_object(page_id).expect("expected page").clone()
        );
        assert_eq!(
            content,
            expected
                .get_object(content_id)
                .expect("expected content")
                .clone()
        );
        assert_eq!(
            compressed,
            expected
                .get_object(compressed_id)
                .expect("expected compressed object")
                .clone()
        );
        assert!(source.source_path().ends_with("2305.13048v2.pdf"));
        assert_eq!(source.source_bytes(), 1_590_242);
        assert_eq!(source.page_count(), 30);
        assert_eq!(source.maximum_object_number(), expected.max_id);
        assert_eq!(source.previous_xref_offset(), expected.xref_start as u64);
        assert_eq!(
            source.trailer().get(b"Root").expect("lazy trailer Root"),
            expected.trailer.get(b"Root").expect("lopdf trailer Root")
        );

        source.object(page_id).expect("cached page object");
        let stats = source.cache_stats().expect("cache stats");
        assert_eq!(stats.source_loads, 3);
        assert_eq!(stats.cache_hits, 1);
        assert!(stats.resident_entries <= policy.maximum_entries);
        assert!(stats.resident_bytes <= policy.maximum_bytes);
        eprintln!(
            "pdf-v3 lazy source object openMs={} readMs={} sourceLoads={} cacheHits={} residentEntries={} residentBytes={}",
            open_elapsed.as_millis(),
            read_elapsed.as_millis(),
            stats.source_loads,
            stats.cache_hits,
            stats.resident_entries,
            stats.resident_bytes
        );
    }

    #[test]
    fn opens_xref_stream_with_indirect_length_through_file_resolver() {
        let source_path = fixture_path("pdflatex-image.pdf");
        let source = PdfSourceObjectStore::open(&source_path)
            .expect("source with indirect xref-stream length");

        assert!(source.page_count() > 0);
        assert!(source
            .trailer()
            .get(b"Root")
            .and_then(Object::as_reference)
            .is_ok());
        assert!(source.previous_xref_offset() < source.source_bytes());
    }

    #[test]
    fn free_and_missing_indirect_objects_resolve_as_pdf_null() {
        for error in [
            PdfError::FreeObject { obj_nr: 27 },
            PdfError::NullRef { obj_nr: 28 },
        ] {
            assert_eq!(
                resolve_indirect_primitive(Err(error)).expect("null indirect object"),
                Primitive::Null
            );
        }
    }

    #[test]
    fn overlay_prefers_explicit_delta_objects_without_mutating_the_source() {
        let source_path = fixture_path("simple-one-page.pdf");
        let source = PdfSourceObjectStore::open(&source_path).expect("source object store");
        let expected = Document::load(&source_path).expect("reference document");
        let page_id = *expected.get_pages().get(&1).expect("page ID");
        let replacement = Object::string_literal("overlay-only");
        let new_id = (source.maximum_object_number() + 1, 0);
        let delta = PdfObjectDelta::try_from_objects(
            BTreeMap::from([(page_id, replacement.clone()), (new_id, Object::Integer(7))]),
            new_id.0,
        )
        .expect("object delta");
        let overlay = PdfObjectOverlay::new(&source, &delta);

        assert_eq!(overlay.object(page_id).expect("overlay page"), replacement);
        assert_eq!(
            overlay.object(new_id).expect("new overlay object"),
            Object::Integer(7)
        );
        assert_eq!(overlay.maximum_object_number(), new_id.0);
        assert_ne!(source.object(page_id).expect("source page"), replacement);
    }
}
