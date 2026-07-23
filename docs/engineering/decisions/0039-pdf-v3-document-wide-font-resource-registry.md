# ADR 0039: PDF v3 Document-Wide Font Resource Registry

Date: 2026-07-18

Status: Accepted

Amends ADR 0023 and ADR 0028.

## Context

PDF v3 already prepares deterministic translation-font subsets and reuses one
face across all replacements inside a single page transaction. Consecutive
page renders still staged six new PDF font objects per used face, however.

A naive whole-document loop would therefore embed the same subset once per
translated page. That recreates the excessive export size the patch-first
rewrite is intended to remove. It also prevents a later incremental writer
from knowing which font objects are document authority.

The existing single-page cache artifact remains intentionally self-contained.
Its PDF bytes cannot depend on a mutable document export session, so page-cache
rendering and final-document resource ownership need separate explicit paths.

## Decision

PDF v3 introduces `DocumentTranslationFontRegistry` for multi-page and final
document rendering.

Before page mutation, the export planner must collect the complete translated
character set for every required face and prepare each deterministic subset.
Registry staging:

- rejects duplicate prepared weights before document mutation;
- sorts faces by `TranslationFontWeight` regardless of caller order;
- stages all faces and object IDs before committing any of them;
- commits exactly one six-object Type0/CIDFont set per used face;
- records its resource name and Type0 object ID with the prepared font identity.

A registry binding is valid only when weight, asset ID, source-font SHA-256 and
deterministic subset name match the requesting `PreparedTranslationFont`. The
referenced PDF object must still be a Type0 font whose `/BaseFont` equals the
recorded subset name. Missing, replaced or mismatched objects are typed errors
before page mutation.

The registry-aware patch renderer performs all existing source, geometry,
style, fit and decision checks. When a replacement is committed, it attaches
the registered resource name and Type0 object ID to the selected page or
copy-on-write Form resources. It stages zero new font objects per page.

The existing renderer entry point continues staging self-contained page-local
fonts. This preserves the deterministic single-page PDF and render-cache byte
contract. Callers must choose the document registry entry point explicitly.

This ADR does not declare final export bounded-memory. `lopdf::Document` still
loads the complete source object graph. `lopdf::IncrementalDocument` can append
changed objects without rewriting source bytes, but the current implementation
also retains the source bytes and parsed object graph in memory. The final
exporter therefore still requires a lazy source-object reader and an
incremental delta writer or equivalent lower-level implementation.

## Evidence

Automated Windows tests prove:

- duplicate Regular faces fail with unchanged object count and `max_id`;
- one staged registry adds exactly six objects;
- a different prepared subset with the same weight is rejected;
- pages 1 and 2 of the real-paper fixture render consecutively through one
  registry;
- both page results report `stagedFontObjectCount = 0`;
- the complete document contains exactly one matching Type0 subset;
- PDFium re-extracts the expected translated text from both pages.

The 30-page Windows AMD probe measured:

- source PDF: 1,590,242 bytes;
- one shared Arial subset: 27,568 bytes;
- complete output: 1,521,952 bytes;
- page 1 Poppler difference: 2,559 pixels (0.1176%), bounds `(245, 1592)` to
  `(550, 1610)`;
- page 2 Poppler difference: 2,059 pixels (0.0946%), bounds `(671, 1592)` to
  `(898, 1610)`;
- page 3: zero changed pixels;
- source/output annotation counts on pages 1-3: `26, 31, 7` in both;
- 30 pages and source metadata retained;
- independent `pypdf` extraction found each translation only on its target
  page.

Visual inspection found no clipping, overlap or unrelated movement on either
translated page.

## Consequences

### Positive

- Translation-font payload no longer grows once per translated page.
- Regular and Bold can each have one document-owned deterministic subset.
- Page and Form copy-on-write paths share one explicit resource authority.
- Single-page cache rendering remains reproducible and isolated.
- The later incremental writer has a stable resource registry to serialize.

### Costs

- Export must know all translated characters before creating the registry.
- A translation revision introducing a new character rebuilds the document
  subset and export.
- Registry identity and object validity are checked on every page commit.
- The current multi-page proof still loads the complete source PDF in memory.

## Rejected Alternatives

- Stage one translation subset per page.
- Search page resource dictionaries and guess whether an existing font matches.
- Make single-page cache artifacts depend on a document export session.
- Call the current full-`Document` loop a bounded-memory streaming exporter.
- Use `lopdf::IncrementalDocument` without accounting for its retained source
  bytes and parsed previous object graph.
