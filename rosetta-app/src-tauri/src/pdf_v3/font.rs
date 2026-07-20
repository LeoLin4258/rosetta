use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use lopdf::{content::Content, Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subsetter::{subset, GlyphRemapper};
use ttf_parser::{Face, GlyphId, Permissions};

use super::{
    object_delta::{PdfObjectDelta, PdfObjectDeltaError},
    page_context::PdfPageObjectContext,
    source_object::{PdfObjectView, PdfSourceObjectError},
};

pub(crate) const SOURCE_HAN_SANS_CN_REGULAR: &str = "SourceHanSansCN-Regular.ttf";
pub(crate) const SOURCE_HAN_SANS_CN_BOLD: &str = "SourceHanSansCN-Bold.ttf";
pub(crate) const GO_NOTO_KURRENT_REGULAR: &str = "GoNotoKurrent-Regular.ttf";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TranslationFontWeight {
    Regular,
    Bold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationFontFamilySpec {
    pub family_id: &'static str,
    pub regular_filename: &'static str,
    pub bold_filename: Option<&'static str>,
}

impl TranslationFontFamilySpec {
    pub(crate) fn filename_for(&self, weight: TranslationFontWeight) -> &'static str {
        match weight {
            TranslationFontWeight::Regular => self.regular_filename,
            TranslationFontWeight::Bold => self.bold_filename.unwrap_or(self.regular_filename),
        }
    }
}

pub(crate) fn recommended_translation_font_family(
    target_language: &str,
) -> TranslationFontFamilySpec {
    let normalized = target_language.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "zh" | "zh-cn" | "zh-hans" | "zh-sg") {
        TranslationFontFamilySpec {
            family_id: "source-han-sans-cn",
            regular_filename: SOURCE_HAN_SANS_CN_REGULAR,
            bold_filename: Some(SOURCE_HAN_SANS_CN_BOLD),
        }
    } else {
        TranslationFontFamilySpec {
            family_id: "go-noto-kurrent",
            regular_filename: GO_NOTO_KURRENT_REGULAR,
            bold_filename: None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum TranslationFontError {
    Read(String),
    Parse(String),
    EmbeddingRestricted,
    SubsettingRestricted,
    UnsupportedOutline,
    MissingGlyphs(Vec<u32>),
    Subset(String),
    MissingPreparedGlyph(char),
    DuplicatePreparedWeight(TranslationFontWeight),
    MissingDocumentFont(TranslationFontWeight),
    DocumentFontIdentityMismatch(TranslationFontWeight),
    DocumentFontObjectInvalid(TranslationFontWeight),
    ObjectIdOverflow,
    PageResources(String),
    Content(String),
    ObjectDelta(PdfObjectDeltaError),
    SourceObject(PdfSourceObjectError),
}

impl fmt::Display for TranslationFontError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(message)
            | Self::Parse(message)
            | Self::Subset(message)
            | Self::PageResources(message)
            | Self::Content(message) => formatter.write_str(message),
            Self::ObjectDelta(error) => error.fmt(formatter),
            Self::SourceObject(error) => error.fmt(formatter),
            Self::EmbeddingRestricted => {
                formatter.write_str("translation font does not permit outline embedding")
            }
            Self::SubsettingRestricted => {
                formatter.write_str("translation font does not permit subsetting")
            }
            Self::UnsupportedOutline => formatter
                .write_str("translation font must use TrueType glyf outlines in this renderer"),
            Self::MissingGlyphs(codepoints) => write!(
                formatter,
                "translation font is missing {} required codepoints",
                codepoints.len()
            ),
            Self::MissingPreparedGlyph(character) => write!(
                formatter,
                "translation font subset does not contain U+{:04X}",
                *character as u32
            ),
            Self::DuplicatePreparedWeight(weight) => {
                write!(
                    formatter,
                    "duplicate prepared translation font weight {weight:?}"
                )
            }
            Self::MissingDocumentFont(weight) => {
                write!(
                    formatter,
                    "document translation font registry has no {weight:?} face"
                )
            }
            Self::DocumentFontIdentityMismatch(weight) => write!(
                formatter,
                "document translation font registry {weight:?} face identity changed"
            ),
            Self::DocumentFontObjectInvalid(weight) => write!(
                formatter,
                "document translation font registry {weight:?} Type0 object is missing or invalid"
            ),
            Self::ObjectIdOverflow => {
                formatter.write_str("PDF object ID overflow while staging translation font")
            }
        }
    }
}

impl std::error::Error for TranslationFontError {}

impl From<PdfObjectDeltaError> for TranslationFontError {
    fn from(value: PdfObjectDeltaError) -> Self {
        Self::ObjectDelta(value)
    }
}

impl From<PdfSourceObjectError> for TranslationFontError {
    fn from(value: PdfSourceObjectError) -> Self {
        Self::SourceObject(value)
    }
}

#[derive(Clone)]
pub(crate) struct TranslationFontAsset {
    asset_id: String,
    weight: TranslationFontWeight,
    face_index: u32,
    bytes: Arc<[u8]>,
    fingerprint: String,
}

#[derive(Default)]
pub(crate) struct TranslationFontAssetCache {
    assets: BTreeMap<(PathBuf, u32, String, TranslationFontWeight), TranslationFontAsset>,
}

impl TranslationFontAssetCache {
    pub(crate) fn load(
        &mut self,
        asset_id: impl Into<String>,
        path: &Path,
        face_index: u32,
    ) -> Result<TranslationFontAsset, TranslationFontError> {
        self.load_weighted(asset_id, TranslationFontWeight::Regular, path, face_index)
    }

    pub(crate) fn load_weighted(
        &mut self,
        asset_id: impl Into<String>,
        weight: TranslationFontWeight,
        path: &Path,
        face_index: u32,
    ) -> Result<TranslationFontAsset, TranslationFontError> {
        let asset_id = asset_id.into();
        let key = (path.to_path_buf(), face_index, asset_id.clone(), weight);
        if let Some(asset) = self.assets.get(&key) {
            return Ok(asset.clone());
        }
        let asset = TranslationFontAsset::open_weighted(asset_id, weight, path, face_index)?;
        self.assets.insert(key, asset.clone());
        Ok(asset)
    }

    pub(crate) fn cached_asset_count(&self) -> usize {
        self.assets.len()
    }

    pub(crate) fn cached_byte_count(&self) -> usize {
        self.assets
            .values()
            .map(TranslationFontAsset::byte_count)
            .sum()
    }
}

impl TranslationFontAsset {
    pub(crate) fn open(
        asset_id: impl Into<String>,
        path: &Path,
        face_index: u32,
    ) -> Result<Self, TranslationFontError> {
        Self::open_weighted(asset_id, TranslationFontWeight::Regular, path, face_index)
    }

    pub(crate) fn open_weighted(
        asset_id: impl Into<String>,
        weight: TranslationFontWeight,
        path: &Path,
        face_index: u32,
    ) -> Result<Self, TranslationFontError> {
        let bytes = std::fs::read(path).map_err(|error| {
            TranslationFontError::Read(format!(
                "failed to read translation font {}: {error}",
                path.display()
            ))
        })?;
        Self::from_bytes_weighted(asset_id, weight, bytes, face_index)
    }

    pub(crate) fn from_bytes(
        asset_id: impl Into<String>,
        bytes: Vec<u8>,
        face_index: u32,
    ) -> Result<Self, TranslationFontError> {
        Self::from_bytes_weighted(asset_id, TranslationFontWeight::Regular, bytes, face_index)
    }

    pub(crate) fn from_bytes_weighted(
        asset_id: impl Into<String>,
        weight: TranslationFontWeight,
        bytes: Vec<u8>,
        face_index: u32,
    ) -> Result<Self, TranslationFontError> {
        let face = Face::parse(&bytes, face_index).map_err(|error| {
            TranslationFontError::Parse(format!("failed to parse translation font: {error:?}"))
        })?;
        if !face.is_outline_embedding_allowed()
            || face.permissions() == Some(Permissions::Restricted)
        {
            return Err(TranslationFontError::EmbeddingRestricted);
        }
        if !face.is_subsetting_allowed() {
            return Err(TranslationFontError::SubsettingRestricted);
        }
        if face.tables().glyf.is_none() {
            return Err(TranslationFontError::UnsupportedOutline);
        }
        let fingerprint = byte_hash(&bytes);
        Ok(Self {
            asset_id: asset_id.into(),
            weight,
            face_index,
            bytes: Arc::from(bytes),
            fingerprint,
        })
    }

    pub(crate) fn byte_count(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn asset_id(&self) -> &str {
        &self.asset_id
    }

    pub(crate) fn face_index(&self) -> u32 {
        self.face_index
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub(crate) fn weight(&self) -> TranslationFontWeight {
        self.weight
    }

    pub(crate) fn prepare(
        &self,
        plan: &UnifiedTranslationFontPlan,
    ) -> Result<PreparedTranslationFont, TranslationFontError> {
        self.prepare_plan(plan, false)
    }

    pub(crate) fn prepare_supported_characters(
        &self,
        plan: &UnifiedTranslationFontPlan,
    ) -> Result<PreparedTranslationFont, TranslationFontError> {
        self.prepare_plan(plan, true)
    }

    fn prepare_plan(
        &self,
        plan: &UnifiedTranslationFontPlan,
        preserve_missing: bool,
    ) -> Result<PreparedTranslationFont, TranslationFontError> {
        let face = Face::parse(&self.bytes, self.face_index).map_err(|error| {
            TranslationFontError::Parse(format!("failed to parse translation font: {error:?}"))
        })?;
        let mut missing = Vec::new();
        let mut source_glyphs = BTreeMap::<char, GlyphId>::new();
        for character in &plan.characters {
            if let Some(glyph_id) = face.glyph_index(*character) {
                source_glyphs.insert(*character, glyph_id);
            } else {
                missing.push(*character as u32);
            }
        }
        if !preserve_missing && !missing.is_empty() {
            return Err(TranslationFontError::MissingGlyphs(missing));
        }

        let mut remapper = GlyphRemapper::new();
        for glyph_id in source_glyphs.values() {
            remapper.remap(glyph_id.0);
        }
        let subset_bytes = subset(&self.bytes, self.face_index, &remapper)
            .map_err(|error| TranslationFontError::Subset(error.to_string()))?;
        let units_per_em = face.units_per_em();
        let mut glyphs = BTreeMap::new();
        for (index, (character, source_glyph_id)) in source_glyphs.into_iter().enumerate() {
            let cid =
                u16::try_from(index + 1).map_err(|_| TranslationFontError::ObjectIdOverflow)?;
            let subset_glyph_id = remapper.get(source_glyph_id.0).ok_or_else(|| {
                TranslationFontError::Subset(format!(
                    "subsetter did not remap glyph {}",
                    source_glyph_id.0
                ))
            })?;
            let advance = face
                .glyph_hor_advance(source_glyph_id)
                .unwrap_or(units_per_em);
            glyphs.insert(
                character,
                PreparedGlyph {
                    cid,
                    subset_glyph_id,
                    width_1000: scale_font_unit(advance.into(), units_per_em),
                },
            );
        }
        let subset_name = subset_font_name(&self.fingerprint, &plan.characters, &self.asset_id);
        let bbox = face.global_bounding_box();
        let metrics = PreparedFontMetrics {
            ascent_1000: scale_font_unit(face.ascender().into(), units_per_em),
            descent_1000: scale_font_unit(face.descender().into(), units_per_em),
            cap_height_1000: scale_font_unit(
                face.capital_height().unwrap_or(face.ascender()).into(),
                units_per_em,
            ),
            bbox_1000: [
                scale_font_unit(bbox.x_min.into(), units_per_em),
                scale_font_unit(bbox.y_min.into(), units_per_em),
                scale_font_unit(bbox.x_max.into(), units_per_em),
                scale_font_unit(bbox.y_max.into(), units_per_em),
            ],
        };
        Ok(PreparedTranslationFont {
            asset_id: self.asset_id.clone(),
            weight: self.weight,
            source_fingerprint: self.fingerprint.clone(),
            source_byte_count: self.bytes.len(),
            subset_name,
            subset_bytes,
            glyphs,
            metrics,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct UnifiedTranslationFontPlan {
    characters: BTreeSet<char>,
}

impl UnifiedTranslationFontPlan {
    pub(crate) fn add_text(&mut self, text: &str) {
        self.characters
            .extend(text.chars().filter(|character| !character.is_control()));
    }

    pub(crate) fn try_add_text(
        &mut self,
        text: &str,
        maximum_characters: usize,
    ) -> Result<(), usize> {
        let mut next = self.characters.clone();
        for character in text.chars().filter(|character| !character.is_control()) {
            next.insert(character);
            if next.len() > maximum_characters {
                return Err(next.len());
            }
        }
        self.characters = next;
        Ok(())
    }

    pub(crate) fn character_count(&self) -> usize {
        self.characters.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.characters.is_empty()
    }
}

#[derive(Debug, Clone)]
struct PreparedGlyph {
    cid: u16,
    subset_glyph_id: u16,
    width_1000: i64,
}

#[derive(Debug, Clone)]
struct PreparedFontMetrics {
    ascent_1000: i64,
    descent_1000: i64,
    cap_height_1000: i64,
    bbox_1000: [i64; 4],
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedTranslationFont {
    pub asset_id: String,
    pub weight: TranslationFontWeight,
    pub source_fingerprint: String,
    pub source_byte_count: usize,
    pub subset_name: String,
    pub subset_bytes: Vec<u8>,
    glyphs: BTreeMap<char, PreparedGlyph>,
    metrics: PreparedFontMetrics,
}

#[derive(Debug, Clone)]
struct DocumentTranslationFontResource {
    resource_name: Vec<u8>,
    type0_font_id: ObjectId,
    asset_id: String,
    source_fingerprint: String,
    subset_name: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DocumentTranslationFontRegistry {
    fonts: BTreeMap<TranslationFontWeight, DocumentTranslationFontResource>,
}

#[derive(Debug, Clone)]
pub(crate) struct StagedDocumentTranslationFonts {
    pub registry: DocumentTranslationFontRegistry,
    pub object_delta: PdfObjectDelta,
}

impl DocumentTranslationFontRegistry {
    pub(crate) fn font_count(&self) -> usize {
        self.fonts.len()
    }

    pub(crate) fn binding_for(
        &self,
        objects: &dyn PdfObjectView,
        font: &PreparedTranslationFont,
    ) -> Result<(&[u8], ObjectId), TranslationFontError> {
        let resource = self
            .fonts
            .get(&font.weight)
            .ok_or(TranslationFontError::MissingDocumentFont(font.weight))?;
        if resource.asset_id != font.asset_id
            || resource.source_fingerprint != font.source_fingerprint
            || resource.subset_name != font.subset_name
        {
            return Err(TranslationFontError::DocumentFontIdentityMismatch(
                font.weight,
            ));
        }
        let type0 = objects.object(resource.type0_font_id)?;
        let valid_type0 = type0.as_dict().is_ok_and(|dictionary| {
            dictionary
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|name| name == b"Type0")
                && dictionary
                    .get(b"BaseFont")
                    .and_then(Object::as_name)
                    .is_ok_and(|name| name == resource.subset_name.as_bytes())
        });
        if !valid_type0 {
            return Err(TranslationFontError::DocumentFontObjectInvalid(font.weight));
        }
        Ok((&resource.resource_name, resource.type0_font_id))
    }
}

pub(crate) fn stage_document_translation_fonts(
    document: &mut Document,
    fonts: &[&PreparedTranslationFont],
) -> Result<DocumentTranslationFontRegistry, TranslationFontError> {
    let staged = stage_document_translation_font_registry(document, fonts)?;
    staged.object_delta.apply_to(document);
    Ok(staged.registry)
}

pub(crate) fn stage_document_translation_font_registry(
    objects: &dyn PdfObjectView,
    fonts: &[&PreparedTranslationFont],
) -> Result<StagedDocumentTranslationFonts, TranslationFontError> {
    let mut fonts_by_weight = BTreeMap::new();
    for font in fonts {
        if fonts_by_weight.insert(font.weight(), *font).is_some() {
            return Err(TranslationFontError::DuplicatePreparedWeight(font.weight()));
        }
    }

    let mut reserved_through = objects.maximum_object_number();
    let mut staged_fonts = Vec::with_capacity(fonts_by_weight.len());
    let mut registry = DocumentTranslationFontRegistry::default();
    for (weight, font) in fonts_by_weight {
        let staged = font.stage_after(
            objects,
            translation_font_resource_name(weight).to_vec(),
            reserved_through,
        )?;
        reserved_through = staged.next_object_number;
        registry.fonts.insert(
            weight,
            DocumentTranslationFontResource {
                resource_name: staged.resource_name.clone(),
                type0_font_id: staged.type0_font_id,
                asset_id: font.asset_id.clone(),
                source_fingerprint: font.source_fingerprint.clone(),
                subset_name: font.subset_name.clone(),
            },
        );
        staged_fonts.push(staged);
    }

    let mut objects = BTreeMap::new();
    for staged in staged_fonts {
        for (object_id, object) in staged.objects {
            if objects.insert(object_id, object).is_some() {
                return Err(TranslationFontError::Content(format!(
                    "staged document font object {} {} is duplicated",
                    object_id.0, object_id.1
                )));
            }
        }
    }
    let object_delta = PdfObjectDelta::try_from_objects(objects, reserved_through)?;
    Ok(StagedDocumentTranslationFonts {
        registry,
        object_delta,
    })
}

pub(crate) fn translation_font_resource_name(weight: TranslationFontWeight) -> &'static [u8] {
    match weight {
        TranslationFontWeight::Regular => b"RosettaTranslationRegular",
        TranslationFontWeight::Bold => b"RosettaTranslationBold",
    }
}

impl PreparedTranslationFont {
    pub(crate) fn weight(&self) -> TranslationFontWeight {
        self.weight
    }

    pub(crate) fn glyph_count(&self) -> usize {
        self.glyphs.len() + 1
    }

    pub(crate) fn encode_text(&self, text: &str) -> Result<Vec<u8>, TranslationFontError> {
        let mut encoded = Vec::with_capacity(text.chars().count() * 2);
        for character in text.chars().filter(|character| !character.is_control()) {
            let glyph = self
                .glyphs
                .get(&character)
                .ok_or(TranslationFontError::MissingPreparedGlyph(character))?;
            encoded.extend_from_slice(&glyph.cid.to_be_bytes());
        }
        Ok(encoded)
    }

    pub(crate) fn text_advance_1000(&self, text: &str) -> Result<i64, TranslationFontError> {
        text.chars()
            .filter(|character| !character.is_control())
            .try_fold(0i64, |total, character| {
                let glyph = self
                    .glyphs
                    .get(&character)
                    .ok_or(TranslationFontError::MissingPreparedGlyph(character))?;
                Ok(total + glyph.width_1000)
            })
    }

    pub(crate) fn stage(
        &self,
        source_objects: &dyn PdfObjectView,
        resource_name: impl Into<Vec<u8>>,
    ) -> Result<StagedTranslationFont, TranslationFontError> {
        self.stage_after(
            source_objects,
            resource_name,
            source_objects.maximum_object_number(),
        )
    }

    pub(crate) fn stage_after(
        &self,
        source_objects: &dyn PdfObjectView,
        resource_name: impl Into<Vec<u8>>,
        reserved_through: u32,
    ) -> Result<StagedTranslationFont, TranslationFontError> {
        let resource_name = resource_name.into();
        let mut next_object_number = source_objects.maximum_object_number().max(reserved_through);
        let mut objects = BTreeMap::new();

        let font_file_id = allocate_object_id(&objects, &mut next_object_number)?;
        let mut font_file = Stream::new(Dictionary::new(), self.subset_bytes.clone());
        font_file
            .dict
            .set("Length1", self.subset_bytes.len() as i64);
        font_file
            .compress()
            .map_err(|error| TranslationFontError::Subset(error.to_string()))?;
        objects.insert(font_file_id, Object::Stream(font_file));

        let cid_to_gid_id = allocate_object_id(&objects, &mut next_object_number)?;
        let mut cid_to_gid = Stream::new(Dictionary::new(), self.cid_to_gid_map());
        cid_to_gid
            .compress()
            .map_err(|error| TranslationFontError::Subset(error.to_string()))?;
        objects.insert(cid_to_gid_id, Object::Stream(cid_to_gid));

        let descriptor_id = allocate_object_id(&objects, &mut next_object_number)?;
        let mut descriptor = Dictionary::new();
        descriptor.set("Type", Object::Name(b"FontDescriptor".to_vec()));
        descriptor.set(
            "FontName",
            Object::Name(self.subset_name.as_bytes().to_vec()),
        );
        descriptor.set("Flags", Object::Integer(4));
        descriptor.set(
            "FontBBox",
            Object::Array(
                self.metrics
                    .bbox_1000
                    .iter()
                    .copied()
                    .map(Object::Integer)
                    .collect(),
            ),
        );
        descriptor.set("ItalicAngle", Object::Integer(0));
        descriptor.set("Ascent", Object::Integer(self.metrics.ascent_1000));
        descriptor.set("Descent", Object::Integer(self.metrics.descent_1000));
        descriptor.set("CapHeight", Object::Integer(self.metrics.cap_height_1000));
        descriptor.set("StemV", Object::Integer(80));
        descriptor.set("FontFile2", Object::Reference(font_file_id));
        objects.insert(descriptor_id, Object::Dictionary(descriptor));

        let descendant_id = allocate_object_id(&objects, &mut next_object_number)?;
        let mut cid_system_info = Dictionary::new();
        cid_system_info.set(
            "Registry",
            Object::String(b"Adobe".to_vec(), StringFormat::Literal),
        );
        cid_system_info.set(
            "Ordering",
            Object::String(b"Identity".to_vec(), StringFormat::Literal),
        );
        cid_system_info.set("Supplement", Object::Integer(0));
        let widths = self
            .glyphs
            .values()
            .map(|glyph| Object::Integer(glyph.width_1000))
            .collect::<Vec<_>>();
        let mut descendant = Dictionary::new();
        descendant.set("Type", Object::Name(b"Font".to_vec()));
        descendant.set("Subtype", Object::Name(b"CIDFontType2".to_vec()));
        descendant.set(
            "BaseFont",
            Object::Name(self.subset_name.as_bytes().to_vec()),
        );
        descendant.set("CIDSystemInfo", Object::Dictionary(cid_system_info));
        descendant.set("FontDescriptor", Object::Reference(descriptor_id));
        descendant.set(
            "W",
            Object::Array(vec![Object::Integer(1), Object::Array(widths)]),
        );
        descendant.set("CIDToGIDMap", Object::Reference(cid_to_gid_id));
        objects.insert(descendant_id, Object::Dictionary(descendant));

        let to_unicode_id = allocate_object_id(&objects, &mut next_object_number)?;
        let mut to_unicode = Stream::new(Dictionary::new(), self.to_unicode_cmap());
        to_unicode
            .compress()
            .map_err(|error| TranslationFontError::Subset(error.to_string()))?;
        objects.insert(to_unicode_id, Object::Stream(to_unicode));

        let type0_id = allocate_object_id(&objects, &mut next_object_number)?;
        let mut type0 = Dictionary::new();
        type0.set("Type", Object::Name(b"Font".to_vec()));
        type0.set("Subtype", Object::Name(b"Type0".to_vec()));
        type0.set(
            "BaseFont",
            Object::Name(self.subset_name.as_bytes().to_vec()),
        );
        type0.set("Encoding", Object::Name(b"Identity-H".to_vec()));
        type0.set(
            "DescendantFonts",
            Object::Array(vec![Object::Reference(descendant_id)]),
        );
        type0.set("ToUnicode", Object::Reference(to_unicode_id));
        objects.insert(type0_id, Object::Dictionary(type0));

        Ok(StagedTranslationFont {
            resource_name,
            type0_font_id: type0_id,
            objects,
            next_object_number,
        })
    }

    fn cid_to_gid_map(&self) -> Vec<u8> {
        let mut map = vec![0, 0];
        for glyph in self.glyphs.values() {
            map.extend_from_slice(&glyph.subset_glyph_id.to_be_bytes());
        }
        map
    }

    fn to_unicode_cmap(&self) -> Vec<u8> {
        let mut output = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /RosettaUnifiedToUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let entries = self.glyphs.iter().collect::<Vec<_>>();
        for chunk in entries.chunks(100) {
            output.push_str(&format!("{} beginbfchar\n", chunk.len()));
            for (character, glyph) in chunk {
                output.push_str(&format!(
                    "<{:04X}> <{}>\n",
                    glyph.cid,
                    utf16_hex(**character)
                ));
            }
            output.push_str("endbfchar\n");
        }
        output.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        output.into_bytes()
    }
}

pub(crate) struct StagedTranslationFont {
    pub resource_name: Vec<u8>,
    pub type0_font_id: ObjectId,
    pub(super) objects: BTreeMap<ObjectId, Object>,
    pub(super) next_object_number: u32,
}

impl StagedTranslationFont {
    pub(crate) fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub(crate) fn commit(self, document: &mut Document) {
        for (object_id, object) in self.objects {
            document.objects.insert(object_id, object);
        }
        document.max_id = document.max_id.max(self.next_object_number);
    }
}

pub(crate) fn attach_translation_font_to_page(
    document: &mut Document,
    page_id: ObjectId,
    resource_name: &[u8],
    type0_font_id: ObjectId,
) -> Result<(), TranslationFontError> {
    let page =
        stage_translation_font_page_dictionary(document, page_id, resource_name, type0_font_id)?;
    document.objects.insert(page_id, Object::Dictionary(page));
    Ok(())
}

pub(crate) fn stage_translation_font_page_dictionary(
    document: &Document,
    page_id: ObjectId,
    resource_name: &[u8],
    type0_font_id: ObjectId,
) -> Result<Dictionary, TranslationFontError> {
    stage_translation_fonts_page_dictionary(
        document,
        page_id,
        std::iter::once((resource_name, type0_font_id)),
    )
}

pub(crate) fn stage_translation_fonts_page_dictionary<'a>(
    document: &Document,
    page_id: ObjectId,
    fonts_to_attach: impl IntoIterator<Item = (&'a [u8], ObjectId)>,
) -> Result<Dictionary, TranslationFontError> {
    let resources = materialize_page_resources(document, page_id)?;
    let page = document.get_dictionary(page_id).cloned().map_err(|error| {
        TranslationFontError::PageResources(format!(
            "failed to clone page dictionary for font resources: {error}"
        ))
    })?;
    stage_translation_fonts_materialized_page(page, resources, fonts_to_attach)
}

pub(crate) fn stage_translation_fonts_page_context<'a>(
    context: &PdfPageObjectContext,
    fonts_to_attach: impl IntoIterator<Item = (&'a [u8], ObjectId)>,
) -> Result<Dictionary, TranslationFontError> {
    stage_translation_fonts_materialized_page(
        context.page_dictionary().clone(),
        context.resources().clone(),
        fonts_to_attach,
    )
}

fn stage_translation_fonts_materialized_page<'a>(
    mut page: Dictionary,
    mut resources: Dictionary,
    fonts_to_attach: impl IntoIterator<Item = (&'a [u8], ObjectId)>,
) -> Result<Dictionary, TranslationFontError> {
    let mut fonts = match resources.get(b"Font") {
        Ok(object) => object.as_dict().cloned().map_err(|_| {
            TranslationFontError::PageResources(
                "materialized page /Font resources are not a dictionary".to_string(),
            )
        })?,
        Err(_) => Dictionary::new(),
    };
    for (resource_name, type0_font_id) in fonts_to_attach {
        if let Ok(existing) = fonts.get(resource_name) {
            if existing.as_reference().ok() != Some(type0_font_id) {
                return Err(TranslationFontError::PageResources(format!(
                    "page font resource {} already exists",
                    String::from_utf8_lossy(resource_name)
                )));
            }
        } else {
            fonts.set(resource_name.to_vec(), Object::Reference(type0_font_id));
        }
    }
    resources.set("Font", Object::Dictionary(fonts));
    page.set("Resources", Object::Dictionary(resources));
    Ok(page)
}

pub(crate) fn append_translation_text(
    document: &mut Document,
    page_id: ObjectId,
    font: &PreparedTranslationFont,
    resource_name: &[u8],
    text: &str,
    x: f32,
    y: f32,
    font_size: f32,
) -> Result<(), TranslationFontError> {
    let encoded = font.encode_text(text)?;
    let content = Content {
        operations: vec![
            lopdf::content::Operation::new("BT", Vec::new()),
            lopdf::content::Operation::new(
                "Tf",
                vec![
                    Object::Name(resource_name.to_vec()),
                    Object::Real(font_size),
                ],
            ),
            lopdf::content::Operation::new(
                "Tm",
                vec![
                    Object::Integer(1),
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(1),
                    Object::Real(x),
                    Object::Real(y),
                ],
            ),
            lopdf::content::Operation::new(
                "Tj",
                vec![Object::String(encoded, StringFormat::Hexadecimal)],
            ),
            lopdf::content::Operation::new("ET", Vec::new()),
        ],
    }
    .encode()
    .map_err(|error| TranslationFontError::Content(error.to_string()))?;
    document
        .add_page_contents(page_id, content)
        .map_err(|error| TranslationFontError::Content(error.to_string()))
}

fn allocate_object_id(
    staged: &BTreeMap<ObjectId, Object>,
    next_object_number: &mut u32,
) -> Result<ObjectId, TranslationFontError> {
    loop {
        *next_object_number = next_object_number
            .checked_add(1)
            .ok_or(TranslationFontError::ObjectIdOverflow)?;
        let object_id = (*next_object_number, 0);
        if !staged.contains_key(&object_id) {
            return Ok(object_id);
        }
    }
}

fn materialize_page_resources(
    document: &Document,
    page_id: ObjectId,
) -> Result<Dictionary, TranslationFontError> {
    let (direct, resource_ids) = document.get_page_resources(page_id).map_err(|error| {
        TranslationFontError::PageResources(format!("failed to inspect page resources: {error}"))
    })?;
    let mut dictionaries = Vec::new();
    if let Some(direct) = direct {
        dictionaries.push(direct);
    }
    for resource_id in resource_ids {
        if let Ok(dictionary) = document.get_dictionary(resource_id) {
            dictionaries.push(dictionary);
        }
    }
    let mut keys = BTreeSet::new();
    for dictionary in &dictionaries {
        keys.extend(dictionary.iter().map(|(key, _)| key.clone()));
    }
    let mut materialized = Dictionary::new();
    for key in keys {
        let Some(first) = dictionaries
            .iter()
            .find_map(|dictionary| dictionary.get(&key).ok())
        else {
            continue;
        };
        if dereference_dictionary(first, document).is_some() {
            let mut merged = Dictionary::new();
            for dictionary in dictionaries.iter().rev() {
                if let Ok(value) = dictionary.get(&key) {
                    if let Some(category) = dereference_dictionary(value, document) {
                        for (name, object) in category.iter() {
                            merged.set(name.clone(), object.clone());
                        }
                    }
                }
            }
            materialized.set(key, Object::Dictionary(merged));
        } else {
            materialized.set(key, first.clone());
        }
    }
    Ok(materialized)
}

fn dereference_dictionary<'a>(
    object: &'a Object,
    document: &'a Document,
) -> Option<&'a Dictionary> {
    match object {
        Object::Dictionary(dictionary) => Some(dictionary),
        Object::Reference(object_id) => document.get_dictionary(*object_id).ok(),
        _ => None,
    }
}

fn subset_font_name(
    source_fingerprint: &str,
    characters: &BTreeSet<char>,
    asset_id: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_fingerprint.as_bytes());
    for character in characters {
        hasher.update((*character as u32).to_be_bytes());
    }
    let tag = hasher
        .finalize()
        .iter()
        .take(3)
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let base = asset_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    format!("{tag}+{base}")
}

fn scale_font_unit(value: i32, units_per_em: u16) -> i64 {
    (f64::from(value) * 1000.0 / f64::from(units_per_em)).round() as i64
}

fn utf16_hex(character: char) -> String {
    let mut units = [0u16; 2];
    character
        .encode_utf16(&mut units)
        .iter()
        .map(|unit| format!("{unit:04X}"))
        .collect()
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
    use std::{env, fs, path::PathBuf, sync::Arc, time::Instant};

    use lopdf::Document;

    use super::{
        append_translation_text, attach_translation_font_to_page,
        recommended_translation_font_family, stage_document_translation_font_registry,
        stage_document_translation_fonts, TranslationFontAsset, TranslationFontAssetCache,
        TranslationFontError, TranslationFontWeight, UnifiedTranslationFontPlan,
        GO_NOTO_KURRENT_REGULAR, SOURCE_HAN_SANS_CN_BOLD, SOURCE_HAN_SANS_CN_REGULAR,
    };
    use crate::pdf_v3::source_object::{PdfObjectOverlay, PdfObjectView, PdfSourceObjectStore};
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, pdfium_test_lock, shared_pdfium,
    };

    #[test]
    fn target_language_selects_one_translation_font_family() {
        let chinese = recommended_translation_font_family("zh-CN");
        assert_eq!(chinese.family_id, "source-han-sans-cn");
        assert_eq!(chinese.regular_filename, SOURCE_HAN_SANS_CN_REGULAR);
        assert_eq!(chinese.bold_filename, Some(SOURCE_HAN_SANS_CN_BOLD));
        assert_eq!(
            chinese.filename_for(TranslationFontWeight::Regular),
            SOURCE_HAN_SANS_CN_REGULAR
        );
        assert_eq!(
            chinese.filename_for(TranslationFontWeight::Bold),
            SOURCE_HAN_SANS_CN_BOLD
        );

        let english = recommended_translation_font_family("en");
        assert_eq!(english.family_id, "go-noto-kurrent");
        assert_eq!(english.regular_filename, GO_NOTO_KURRENT_REGULAR);
        assert_eq!(english.bold_filename, None);
        assert_eq!(
            english.filename_for(TranslationFontWeight::Bold),
            GO_NOTO_KURRENT_REGULAR
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_font_subset_is_deterministic_and_reused_across_pages() {
        let _guard = pdfium_test_lock();
        let mut cache = TranslationFontAssetCache::default();
        let font_path = PathBuf::from(r"C:\Windows\Fonts\arial.ttf");
        let asset = cache
            .load("ArialRegular", &font_path, 0)
            .expect("Windows Arial font");
        let repeated_asset = cache
            .load("ArialRegular", &font_path, 0)
            .expect("cached Windows Arial font");
        assert_eq!(cache.cached_asset_count(), 1);
        assert_eq!(cache.cached_byte_count(), asset.byte_count());
        assert_eq!(asset.fingerprint(), repeated_asset.fingerprint());
        assert!(Arc::ptr_eq(&asset.bytes, &repeated_asset.bytes));
        let mut unsupported_plan = UnifiedTranslationFontPlan::default();
        unsupported_plan.add_text("统一");
        assert!(matches!(
            asset.prepare(&unsupported_plan),
            Err(TranslationFontError::MissingGlyphs(codepoints)) if !codepoints.is_empty()
        ));
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text("Rosetta unified font 123");
        let prepared = asset.prepare(&plan).expect("prepared subset");
        let repeated = asset.prepare(&plan).expect("repeated subset");
        let mut larger_plan = plan.clone();
        larger_plan.add_text("additional glyph coverage XYZ");
        let larger = asset.prepare(&larger_plan).expect("larger subset");
        assert_eq!(plan.character_count() + 1, prepared.glyph_count());
        assert!(prepared.subset_bytes.len() < asset.byte_count());
        assert_eq!(prepared.subset_name, repeated.subset_name);
        assert_eq!(prepared.subset_bytes, repeated.subset_bytes);
        assert_eq!(
            prepared
                .text_advance_1000("Rosetta unified font 123")
                .expect("page subset metrics"),
            larger
                .text_advance_1000("Rosetta unified font 123")
                .expect("document subset metrics")
        );

        let source = fs::read(fixture_path("2305.13048v2.pdf")).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let pages = document.get_pages();
        let first_page_id = pages[&1];
        let second_page_id = pages[&2];
        let before_max_id = document.max_id;
        let staged = prepared
            .stage(&document, b"RosettaTranslationRegular".to_vec())
            .expect("staged font");
        assert_eq!(staged.object_count(), 6);
        assert_eq!(document.max_id, before_max_id);
        let resource_name = staged.resource_name.clone();
        let font_id = staged.type0_font_id;
        staged.commit(&mut document);
        attach_translation_font_to_page(&mut document, first_page_id, &resource_name, font_id)
            .expect("attach first page font");
        attach_translation_font_to_page(&mut document, second_page_id, &resource_name, font_id)
            .expect("attach second page font");
        append_translation_text(
            &mut document,
            first_page_id,
            &prepared,
            &resource_name,
            "Rosetta unified font 123",
            72.0,
            500.0,
            14.0,
        )
        .expect("append first page text");
        append_translation_text(
            &mut document,
            second_page_id,
            &prepared,
            &resource_name,
            "Rosetta unified font 123",
            72.0,
            500.0,
            14.0,
        )
        .expect("append second page text");

        let mut output = Vec::new();
        document.save_to(&mut output).expect("save output");
        let pdfium = shared_pdfium();
        let output_document = pdfium
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output");
        for page_index in [0, 1] {
            let text = output_document
                .pages()
                .get(page_index)
                .expect("output page")
                .text()
                .expect("output text")
                .all();
            assert!(text.contains("Rosetta unified font 123"));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn weighted_font_asset_keeps_face_intent_through_subsetting() {
        let mut cache = TranslationFontAssetCache::default();
        let regular = cache
            .load_weighted(
                "ArialRegular",
                TranslationFontWeight::Regular,
                PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
                0,
            )
            .expect("regular font");
        let bold = cache
            .load_weighted(
                "ArialBold",
                TranslationFontWeight::Bold,
                PathBuf::from(r"C:\Windows\Fonts\arialbd.ttf").as_path(),
                0,
            )
            .expect("bold font");
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text("Weighted subset");
        let regular_prepared = regular.prepare(&plan).expect("regular subset");
        let bold_prepared = bold.prepare(&plan).expect("bold subset");

        assert_eq!(cache.cached_asset_count(), 2);
        assert_eq!(regular.weight(), TranslationFontWeight::Regular);
        assert_eq!(bold.weight(), TranslationFontWeight::Bold);
        assert_eq!(regular_prepared.weight(), TranslationFontWeight::Regular);
        assert_eq!(bold_prepared.weight(), TranslationFontWeight::Bold);
        assert_ne!(regular_prepared.subset_bytes, bold_prepared.subset_bytes);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn document_font_registry_is_atomic_and_identity_checked() {
        let asset = TranslationFontAsset::open(
            "ArialRegular",
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Windows Arial font");
        let mut first_plan = UnifiedTranslationFontPlan::default();
        first_plan.add_text("Registry identity one");
        let first = asset.prepare(&first_plan).expect("first prepared subset");
        let mut second_plan = UnifiedTranslationFontPlan::default();
        second_plan.add_text("Registry identity two");
        let second = asset.prepare(&second_plan).expect("second prepared subset");
        let source = fs::read(fixture_path("2305.13048v2.pdf")).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let before_objects = document.objects.len();
        let before_max_id = document.max_id;

        assert!(matches!(
            stage_document_translation_fonts(&mut document, &[&first, &first])
                .expect_err("duplicate face must fail before mutation"),
            TranslationFontError::DuplicatePreparedWeight(TranslationFontWeight::Regular)
        ));
        assert_eq!(document.objects.len(), before_objects);
        assert_eq!(document.max_id, before_max_id);

        let registry = stage_document_translation_fonts(&mut document, &[&first])
            .expect("document font registry");
        assert_eq!(registry.font_count(), 1);
        assert_eq!(document.objects.len(), before_objects + 6);
        assert_eq!(document.max_id, before_max_id + 6);
        assert!(registry.binding_for(&document, &first).is_ok());
        assert!(matches!(
            registry
                .binding_for(&document, &second)
                .expect_err("different subset identity"),
            TranslationFontError::DocumentFontIdentityMismatch(TranslationFontWeight::Regular)
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn document_font_registry_stages_against_lazy_source_and_validates_overlay() {
        let asset = TranslationFontAsset::open(
            "ArialRegular",
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Windows Arial font");
        let mut first_plan = UnifiedTranslationFontPlan::default();
        first_plan.add_text("Lazy registry identity one");
        let first = asset.prepare(&first_plan).expect("first prepared subset");
        let mut second_plan = UnifiedTranslationFontPlan::default();
        second_plan.add_text("Lazy registry identity two");
        let second = asset.prepare(&second_plan).expect("second prepared subset");
        let source = PdfSourceObjectStore::open(fixture_path("2305.13048v2.pdf"))
            .expect("lazy source object store");
        let source_maximum = source.maximum_object_number();

        let staged = stage_document_translation_font_registry(&source, &[&first])
            .expect("lazy document font registry");
        assert_eq!(staged.object_delta.object_count(), 6);
        assert_eq!(source.maximum_object_number(), source_maximum);
        assert_eq!(
            staged.object_delta.maximum_object_number(),
            source_maximum + 6
        );
        assert_eq!(
            source
                .cache_stats()
                .expect("source cache stats")
                .source_loads,
            0
        );

        let overlay = PdfObjectOverlay::new(&source, &staged.object_delta);
        assert!(staged.registry.binding_for(&overlay, &first).is_ok());
        assert_eq!(
            source
                .cache_stats()
                .expect("source cache stats")
                .source_loads,
            0
        );
        assert!(matches!(
            staged
                .registry
                .binding_for(&overlay, &second)
                .expect_err("different subset identity"),
            TranslationFontError::DocumentFontIdentityMismatch(TranslationFontWeight::Regular)
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows Source Han Sans CJK subset and render probe"]
    fn manual_windows_source_han_subset_render_probe() {
        let _guard = pdfium_test_lock();
        let default_path = PathBuf::from(
            r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Regular.ttf",
        );
        let font_path = env::var("ROSETTA_PDF_V3_FONT_PATH")
            .map(PathBuf::from)
            .unwrap_or(default_path);
        let load_started = Instant::now();
        let asset = TranslationFontAsset::open("SourceHanSansCNRegular", &font_path, 0)
            .expect("Source Han Sans font");
        let load_ms = load_started.elapsed().as_millis();
        let text = "统一译文字体：性能、体积与视觉保真。Rosetta PDF v3";
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text(text);
        let subset_started = Instant::now();
        let prepared = asset.prepare(&plan).expect("prepared CJK subset");
        let subset_ms = subset_started.elapsed().as_millis();
        let stress_text = (0x4E00..0x4E00 + 1000)
            .filter_map(char::from_u32)
            .collect::<String>();
        let mut stress_plan = UnifiedTranslationFontPlan::default();
        stress_plan.add_text(&stress_text);
        let stress_started = Instant::now();
        let stress_prepared = asset.prepare(&stress_plan).expect("prepared stress subset");
        let stress_ms = stress_started.elapsed().as_millis();
        let source =
            fs::read(fixture_path("002-trivial-libre-office-writer.pdf")).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let page_id = document.get_pages()[&1];
        let staged = prepared
            .stage(&document, b"RosettaTranslationRegular".to_vec())
            .expect("staged font");
        let resource_name = staged.resource_name.clone();
        let font_id = staged.type0_font_id;
        staged.commit(&mut document);
        attach_translation_font_to_page(&mut document, page_id, &resource_name, font_id)
            .expect("attach font");
        append_translation_text(
            &mut document,
            page_id,
            &prepared,
            &resource_name,
            text,
            72.0,
            500.0,
            14.0,
        )
        .expect("append CJK text");
        let mut output = Vec::new();
        document.save_to(&mut output).expect("save CJK output");
        let pdfium = shared_pdfium();
        let output_document = pdfium
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output");
        let extracted = output_document
            .pages()
            .get(0)
            .expect("output page")
            .text()
            .expect("output text")
            .all();
        assert!(extracted.contains(text));
        println!(
            "pdf-v3 unified-font source={} subset={} glyphs={} output={} load={}ms subset={}ms stress_glyphs={} stress_subset={} stress={}ms",
            prepared.source_byte_count,
            prepared.subset_bytes.len(),
            prepared.glyph_count(),
            output.len(),
            load_ms,
            subset_ms,
            stress_prepared.glyph_count(),
            stress_prepared.subset_bytes.len(),
            stress_ms
        );
        if let Ok(path) = env::var("ROSETTA_PDF_V3_FONT_OUTPUT") {
            fs::write(path, &output).expect("write font output");
        }
    }
}
