# ADR 0023: PDF v3 Unified Translation Fonts

Date: 2026-07-17

Status: Accepted

## Context

PDF v3 source extraction records original font provenance, but source decoding
does not prove that an embedded source subset can encode translated text. Trying
to preserve every source typeface would require per-font Unicode coverage,
encoding, licensing, embedding and fitting logic. It would also create many
font resources and subsets in one output document.

The product requirement is visual fidelity of layout, protected content,
color, weight and geometry. A translation is intentionally a different
language; matching the original typeface family is not required when one
controlled translation family provides better performance, smaller output and
more predictable glyph coverage.

The existing managed PDF component pack already distributes offline Source Han
Sans CN Regular/Bold and GoNotoKurrent assets. Windows system fonts are useful
for development probes but cannot be a production dependency.

## Decision

PDF v3 uses a Rosetta-selected unified translation font family rather than
reusing source PDF fonts.

Initial family policy:

- Simplified Chinese uses Source Han Sans CN Regular.
- Source Han Sans CN Bold is loaded only when validated translated style spans
  require a real bold face.
- Other target languages use GoNotoKurrent as the broad-coverage family
  candidate.
- A missing glyph is a typed fallback; the renderer does not silently switch to
  an operating-system font.

Typeface family is normalized, while page geometry, font size, color, opacity,
weight intent and protected spans remain renderer inputs. Source font metadata
continues to support extraction and style analysis, not translated encoding.

### Asset ownership

Translation fonts are versioned component assets with hashes and license
metadata. The runtime reads each selected font file once into a reusable
`TranslationFontAssetCache`; documents and pages share the same immutable byte
snapshot. Production does not resolve `C:/Windows/Fonts` or platform font APIs.

Before use, the native font layer validates:

- OpenType parsing and face index;
- outline embedding permission;
- subsetting permission;
- TrueType `glyf` outlines for the current writer;
- complete glyph coverage for the planned translated character set.

### Document-wide subset

The renderer collects one sorted character set for the complete document and
face. It creates one deterministic PDF-specific subset with `subsetter` and
reuses it across every page. Input page order does not affect glyph IDs, subset
bytes or subset name.

The PDF font representation uses:

- one embedded subset stream;
- one explicit CID-to-GID map;
- one `FontDescriptor`;
- one `CIDFontType2` descendant;
- one ToUnicode CMap;
- one Type0 font using `Identity-H`.

CIDs are assigned deterministically by Unicode scalar order. They are separate
from remapped subset GIDs, allowing the PDF ToUnicode map to preserve extraction
identity. Widths and font metrics are normalized to 1000 units.

All six font objects are staged before commit. The page resource layer can
attach the same Type0 object to any number of pages without duplicating font
bytes.

### Shaping boundary

The current proof supports Simplified Chinese and direct-cmap Latin text.
Glyph coverage alone is not permission to render Arabic, Indic or other scripts
that require shaping. Those scripts remain unsupported until a shaping engine
produces positioned glyph runs and the renderer validates extraction and visual
output.

## Evidence

Windows/AMD Source Han Sans CN probe:

- source font: 10,397,552 bytes;
- 29 used characters plus `.notdef`: 7,064-byte subset;
- source fixture PDF: 12,609 bytes;
- output PDF with embedded Chinese font and ToUnicode: 19,290 bytes;
- output growth: 6,681 bytes;
- PDFium re-extracted the complete Chinese/Latin string exactly;
- Poppler rendered Chinese, full-width punctuation and Latin without missing
  glyphs or layout defects.

A 1000-CJK-character stress plan produced:

- 1,001 glyphs including `.notdef`;
- 255,624 subset bytes;
- about 14 ms subset time in the Windows debug test;
- 2.46% of the full Source Han font size.

Cold font read and validation measured about 304 ms on this development run.
The asset cache removes that cost from page-level and repeated-document work.

An automated Windows test also proves:

- identical character sets produce byte-identical subsets and names;
- staging six objects does not mutate `Document.max_id`;
- one Type0 font object is attached to pages 1 and 2;
- PDFium extracts inserted text correctly from both pages.

## Consequences

### Positive

- Translated encoding is independent of arbitrary source font subsets.
- Font parsing and cold I/O happen once instead of once per page or source font.
- One document-wide subset prevents repeated font embedding.
- CJK subset creation is fast relative to extraction and translation.
- Output size scales with used glyphs rather than the 10-15 MB source font.
- ToUnicode preserves search, copy and re-extraction of translated text.
- Regular and real bold can share one controlled family.

### Costs

- Translation typeface may differ visibly from the source family.
- Document export must know the complete glyph set before final font commit.
- A later translation revision that introduces new glyphs rebuilds the subset.
- Current embedding supports TrueType `glyf`; CFF/CFF2 require another path.
- Complex scripts require shaping before they can use the broad-coverage font.
- Font asset licenses and hashes become component manifest requirements.
- Actual source-region replacement, fitting and line breaking remain separate.

## Rejected Alternatives

- Reuse each original PDF font whenever source decoding succeeds.
- Embed the complete 10-15 MB translation font in every output.
- Create one font subset per page.
- Resolve fonts from the operating system at render time.
- Silently fall back to a second font for missing glyphs.
- Treat Unicode coverage as sufficient for complex-script rendering.
