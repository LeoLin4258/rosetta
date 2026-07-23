# PDF v3 Atom-to-Operand Mapping Spike

Date: 2026-07-17

## Summary

Added the first PDF v3 mapping layer between PDFium text objects/page-text atoms
and Rosetta's low-level content-stream text-show provenance. Added a strict
source-only ToUnicode decoder to cover valid fixture CMaps that `lopdf 0.34`
cannot parse.

The implementation remains isolated under `pdf_v3` and is not connected to the
old PDF worker, jobs, UI, translation, persistence, preview cache or export.

## Implementation

Added:

- stable cross-stream text-show indexes and IDs;
- selected-page font resource inspection;
- font subtype, encoding, embedding, ToUnicode and capability classification;
- strict one- and two-byte fixture ToUnicode decoding with `bfchar`, `bfrange`,
  ligatures and UTF-16 surrogate pairs;
- PDFium text-object to content-show ordinal pairing;
- independent decoded-text, whitespace, font and atom-coverage checks;
- exact, whitespace-equivalent and mismatch states;
- explicit Form XObject and unsupported-decoder preservation reasons;
- hash/count/code-point diagnostics without serialized source text;
- stable mapping ID and no-text-payload tests.

`translated_text_reuse_allowed` remains `false` for every inspected source font.
ToUnicode is used only to decode and validate source bytes.

## Windows AMD Results

Page 1 of `2305.13048v2.pdf` produced:

- 242 PDFium text objects and 242 text-show operations;
- 242 ordinal pairs, with no unmatched object or show;
- 120 exact mappings;
- 107 whitespace-equivalent mappings caused by PDFium positioning whitespace;
- 15 Unicode mismatches, all PDFium U+0002 versus ToUnicode U+002D;
- 5 / 5 source-decodable page fonts;
- 0 / 5 fonts approved for translated-text reuse;
- 1 Form XObject requiring recursive mapping;
- about 500 ms elapsed in the unoptimized probe.

The first pages of five additional fixtures preserve object/show count equality:

- `simple-one-page.pdf`: 4 / 4;
- `pdflatex-image.pdf`: 10 / 10;
- `multicolumn.pdf`: 74 / 74;
- `google-doc-document.pdf`: 1,045 / 1,045;
- `GeoTopo.pdf`: 3 / 3.

The corpus also confirms that count equality alone is insufficient. The Google
Docs fixture exposes font and atom-coverage mismatches, and `GeoTopo.pdf`
contains a Form XObject. These remain conservative fallback states.

## Decision

The ordinal mapping direction is accepted only as a candidate-pair mechanism.
Production mapping must retain all independent validation checks. PageGraph
text will reconcile PDFium geometry with validated ToUnicode source text and
will represent synthetic whitespace separately from encoded glyph operands.

Source decode, source round-trip encode and translated-font capability remain
separate. Translated Unicode insertion is still blocked on font embedding,
subsetting, fitting and export validation.

The current probe separately loads the complete PDF through PDFium and `lopdf`.
This remains acceptable only for the spike; reusable handles and bounded-memory
long-document processing are still production gates.

## Validation

- `cargo fmt -- --check`;
- `cargo check`;
- `cargo test pdf_v3` (`25` passed, `5` manual probes ignored);
- `cargo test rosetta_jobs` (`78` passed);
- explicit Windows real-page mapping probe;
- first-page fixture mapping matrix;
- source-text payload exclusion test;
- `git diff --check`.
