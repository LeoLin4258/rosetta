# ADR 0017: PDF v3 Source Text Provenance and Font Capability Separation

Date: 2026-07-17

Status: Accepted

## Context

ADR 0016 selected PDFium for extraction and a Rosetta-owned content-stream
patch layer for rendering. The next requirement is to map PDFium PageGraph
atoms to the encoded operands that the renderer can patch.

The two representations do not expose identical text:

- PDFium page text includes whitespace inferred from glyph positioning and
  `TJ` advances even when no whitespace exists in an encoded string operand;
- on the real paper fixture, PDFium exposes U+0002 for 15 glyphs whose
  authoritative ToUnicode mapping is U+002D;
- `lopdf 0.34` cannot parse valid one-byte ToUnicode CMaps used by the tested
  LibreOffice and TeX PDFs;
- decoding source bytes does not prove that a source font can encode arbitrary
  translated Unicode or that its embedded subset contains the required glyphs.

Treating PDFium text, `lopdf` decoding, ordinal order, or the source font as a
single authority would create silent corruption in PageGraph or export.

## Decision

PDF v3 separates source provenance and font capabilities into explicit checks:

- PDFium remains authoritative for page geometry, object order, atom geometry,
  style, preview and validation;
- a Rosetta-owned source decoder reads ToUnicode CMaps for semantic source text;
- top-level PDFium text objects and content-stream text-show operations may be
  paired by ordinal order only after their counts match for the inspected page;
- each pair independently records decoded text, font-name, atom-coverage and
  whitespace-equivalence checks;
- exact, whitespace-equivalent and mismatched mappings remain distinct states;
- Form XObjects, missing decoders, count mismatches, font mismatches, incomplete
  atom coverage and Unicode mismatches remain explicit preservation reasons;
- source decoding, source round-trip encoding and translated-text font support
  are three separate capabilities;
- translated text may not reuse a source font unless a future font subsystem
  proves Unicode coverage, encoding, embedding and output validation.

The current source CMap decoder supports the bounded ToUnicode subset observed
in the fixture corpus: one- and two-byte code spaces, `bfchar`, sequential and
array `bfrange` mappings, multi-character Unicode destinations and UTF-16
surrogate pairs. It rejects malformed ranges, unmapped source codes, duplicate
mappings, mappings outside the declared code space and inherited `usecmap`
resources it cannot resolve.

The mapping diagnostic persists hashes, counts, code points and provenance, but
does not serialize source text payloads.

## Evidence

On page 1 of `2305.13048v2.pdf`:

- 244 top-level page objects;
- 242 PDFium text objects;
- 242 content-stream text-show operations across 2 streams;
- 242 ordinal pairs and no unmatched text objects or shows;
- 120 exact text mappings;
- 107 whitespace-equivalent mappings;
- 15 Unicode mismatches, all PDFium U+0002 versus ToUnicode U+002D;
- 5 font resources, all source-decodable after the Rosetta CMap decoder;
- 0 fonts approved for translated-text reuse;
- 1 Form XObject retained as an explicit recursive-mapping gap;
- about 500 ms for the current unoptimized probe, including separate whole-file
  loads by PDFium and `lopdf`.

The first-page fixture corpus also preserves object/show count alignment:

- `simple-one-page.pdf`: 4 / 4, all exact;
- `pdflatex-image.pdf`: 10 / 10;
- `multicolumn.pdf`: 74 / 74;
- `google-doc-document.pdf`: 1,045 / 1,045;
- `GeoTopo.pdf`: 3 / 3, with a Form XObject gap.

Count alignment is evidence for stable traversal order, not sufficient evidence
for a safe patch. The independent checks remain mandatory.

## Consequences

### Positive

- PageGraph can repair known PDFium Unicode defects without giving up PDFium's
  geometry and performance advantages.
- Synthetic visual whitespace is not falsely attributed to encoded operands.
- Unsupported mappings preserve source instead of receiving heuristic patches.
- Font embedding and translated-text encoding can be designed independently of
  source extraction.
- Mapping diagnostics remain inspectable without persisting document text.

### Costs

- PageGraph construction needs an explicit reconciliation stage between PDFium
  atoms and source operands.
- Form XObjects and inherited CMaps require recursive resource resolution.
- The current probe loads the whole document twice and is not the production
  `DocumentHandle` or long-document memory architecture.
- A new font subsystem is still required before translated Unicode insertion.

## Rejected Alternatives

- Trust PDFium object or page text as the only Unicode authority.
- Trust `lopdf::get_font_encoding()` as complete PDF font support.
- Treat ordinal count equality as proof of an exact mapping.
- Strip whitespace globally and classify every remaining pair as exact.
- Reuse embedded source fonts for translation because they decode source text.
