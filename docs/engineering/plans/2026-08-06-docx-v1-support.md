# DOCX v1 Support Plan

> Status: Active planning and implementation authority for DOCX v1.
> Created: 2026-08-06.
> Scope: single-file `.docx` import, structural translation preview, and translated `.docx` export.
> This plan is authoritative for unfinished DOCX work. Once a checkpoint lands,
> current code, the DOCX ADR, data-model conventions, and its change-log entry
> become authoritative for that checkpoint.

## 1. Executive Decision

DOCX is worth building next.

It is directly aligned with Rosetta's narrow product promise: private, local,
long-form document translation with structure preservation. It also reaches a
common office-document workflow without introducing a cloud service, account
system, chat surface, or a separate translation engine.

The implementation should proceed, but only with a loss-minimizing package
patching architecture:

```text
source.docx
  -> bounded ZIP/OOXML inspection
  -> body paragraphs, headings, lists, and table cells -> Rosetta IR
  -> existing Segment translation pipeline
  -> copy the original OOXML package
  -> replace mapped text in word/document.xml only
  -> atomically write translated.docx
```

Do not deserialize a DOCX into a partial typed model and regenerate the whole
package. That approach makes Rosetta responsible for every Word feature the
library does not understand and creates avoidable loss of images, numbering,
styles, relationships, and extension parts.

The expected effort is 12 to 18 focused engineering days, excluding translation
model quality tuning. The first 2 days are a go/no-go compatibility spike. If
the spike cannot produce Word- and LibreOffice-readable output without repair
prompts on the agreed fixture corpus, implementation stops before product UI
work.

## 2. Product Scope

### v1 commitments

- Import one `.docx` file as one Rosetta job.
- Keep all processing and artifacts local.
- Extract main-document body paragraphs in stable reading order.
- Recognize headings from paragraph style/outline metadata.
- Recognize numbered and bulleted list items from numbering properties.
- Extract paragraphs inside table cells with row and column provenance.
- Reuse the existing Rosetta block, segment, translation-file, revision, and
  batch translation workflow.
- Show a block-virtualized structural preview, not a Word page renderer.
- Export a translated `.docx` while preserving all unchanged OOXML package
  parts, including images, styles, numbering, themes, section/page setup,
  headers, footers, relationships, charts, and unknown extension parts.
- Preserve paragraph properties and apply translated text using a deterministic,
  documented run policy.
- Refuse unsafe or unsupported packages clearly instead of silently producing a
  damaged document.

### v1 non-goals

- Exact Word pagination or a WYSIWYG Word preview.
- Pixel-perfect line wrapping after translation.
- Translating headers, footers, footnotes, endnotes, comments, text boxes,
  shapes, equations, or chart labels.
- Perfect preservation of mixed inline bold, italic, color, and font changes
  after word order changes.
- Source DOCX editing inside Rosetta.
- Bilingual DOCX export.
- Folder import containing DOCX files.
- `.doc`, `.docm`, encrypted Office packages, or password recovery.
- Accepting/rejecting tracked changes or modifying document review history.
- Fetching external relationships, linked images, templates, or other remote
  resources.

### explicit v1 user-visible behavior

- The file picker and empty-workbench copy list DOCX after backend support is
  present.
- A DOCX job uses the normal document workbench and translation controls. It
  does not require the PDF runtime.
- DOCX preview represents headings, list depth, and table coordinates. It does
  not claim to reproduce pages.
- DOCX exposes translated export only. The bilingual action is hidden or
  disabled for DOCX with format-specific copy.
- Unsupported active/review content produces an actionable import error. It
  must not become an empty job or a partially translated export.

## 3. Current Rosetta Fit

The current core IR is already sufficient for DOCX v1:

- `RosettaDocument` owns the imported file and ordered blocks.
- `RosettaBlock` already supports `heading`, `paragraph`, `list_item`, and
  `table_cell`.
- `RosettaBlock.path` can hold a stable diagnostic OOXML location.
- `RosettaBlock.style` can hold optional DOCX provenance without adding new
  top-level core fields.
- `Segment` remains the translation scheduling and cache unit.
- The preview already virtualizes blocks and reconstructs translated block text
  from ordered segments.
- Translation files and revisions already separate target-language output from
  source segments.

The required integration is format-specific rather than a new product flow.
The main current mismatches are:

- Type vocabulary mentions `docx`, but `RosettaSourceDocumentFormat` excludes
  it.
- Rust format detection and the file picker accept only TXT, Markdown, and PDF.
- normal import reads UTF-8 strings and enforces a 5 MB text-file limit.
- the generic exporter renders UTF-8 text, while DOCX needs binary package
  output.
- the preview renders non-Markdown blocks as plain paragraphs and needs small
  structural treatments for heading/list/table-cell blocks.
- folder import is deliberately limited to TXT/Markdown and should remain so in
  DOCX v1.

DOCX must not reuse the PDF page pipeline. PDF is visual and page-artifact
oriented; DOCX is an editable structured package and fits the normal block and
segment pipeline.

## 4. Technology Research

Research snapshot: 2026-08-06.

### Option A: `docx-rs` typed read and write

[`docx-rs`](https://crates.io/crates/docx-rs) is active and useful for creating
new documents. Crate `0.4.22` was released on 2026-07-21, the repository had 545
stars and was pushed on 2026-07-28, and crates.io reported 3,031,168 total
downloads at research time.

It is not suitable as Rosetta's authoritative round-trip exporter. Its open
issues demonstrate the exact class of risks Rosetta must avoid:

- [#597](https://github.com/bokuweb/docx-rs/issues/597): reading then writing
  shortens a document.
- [#717](https://github.com/bokuweb/docx-rs/issues/717): images disappear after
  opening and saving.
- [#759](https://github.com/bokuweb/docx-rs/issues/759): numbering changes during
  parsing/round trip.
- [#873](https://github.com/bokuweb/docx-rs/issues/873): valid DOCX variants fail
  parsing; the reported corpus includes Strict OOXML and table variants.

The library zipper builds a known set of package parts. Rosetta instead needs
unknown parts to survive unchanged. `docx-rs` may still be used in tests to
generate simple fixtures, but it should not read and regenerate user documents
in production.

### Option B: legacy `docx` crate

[`docx`](https://crates.io/crates/docx) `1.1.2` was last released in 2020 and is
based on an older package stack. It is not a credible production dependency for
new DOCX support.

### Option C: `docx-review-core`

[`docx-review-core`](https://crates.io/crates/docx-review-core) `0.1.1` has an
interesting extraction model for comments, tracked changes, hyperlinks,
headers, footers, footnotes, and endnotes. At research time it had only 69
crates.io downloads and explicitly focused on reading/review rather than
editing. Its design can inform fixtures and parser behavior, but it should not
become a v1 production dependency without corpus-based validation.

### Option D: package copy plus narrow XML mutation

This is the selected approach.

Rosetta already directly depends on [`zip`](https://crates.io/crates/zip),
resolved as `2.4.2`, and that version supports raw copying archive entries. The
source package can therefore be rewritten with every entry copied in its
original compressed form except `word/document.xml`.

Use [`quick-xml`](https://crates.io/crates/quick-xml) for bounded event-based
OOXML reading and writing. Version `0.39.3` is currently present transitively
through Tauri/plist, while current crates.io is `0.41.0`. DOCX work should add a
direct, pinned compatible dependency rather than relying on a transitive crate.
The implementation checkpoint must choose the exact version through normal
`cargo check` and lockfile review.

### Rejected conversion paths

- LibreOffice headless conversion adds a large external runtime, changes layout
  and OOXML independently of Rosetta, and is hard to make deterministic across
  platforms.
- Converting DOCX to Markdown/HTML and rebuilding DOCX discards package parts
  and inline structure.
- Running Office automation would make support platform-specific and require an
  installed desktop application.

## 5. Target Module Boundary

Recommended Rust layout:

```text
rosetta-app/src-tauri/src/rosetta_jobs/
  formats/
    mod.rs
    docx/
      mod.rs          # public format boundary
      package.rs      # bounded ZIP/OPC inspection and copying
      parse.rs        # document.xml -> blocks and segments
      styles.rs       # styles.xml and numbering.xml lookup
      provenance.rs   # durable source manifest
      export.rs       # mapped text patch and atomic package write
      policy.rs       # unsupported structures and quotas
```

Keep these responsibilities outside the module:

- generic job indexing and bundle loading stay in `store.rs`;
- source-format dispatch stays in `formats/mod.rs` and `import.rs`;
- translation scheduling stays unchanged;
- user-selected export dispatch stays in the existing Tauri command boundary;
- frontend preview remains in the existing preview feature.

No broad filesystem permission is required. The frontend passes a user-selected
path to the existing narrow Tauri import/export commands.

## 6. OOXML Package Contract

### accepted package profile

An accepted v1 source must:

- be a ZIP-based OPC package;
- contain `[Content_Types].xml`, `_rels/.rels`, and a valid office-document
  relationship;
- resolve the main document part inside the package, normally
  `word/document.xml`;
- declare a WordprocessingML main-document content type;
- contain well-formed XML within configured limits;
- not be encrypted or macro-enabled;
- not contain tracked-change structures covered by the fail-closed policy.

Do not hard-code `word/document.xml` as the only discoverable main part. Resolve
it through the root relationship and then store the resolved part in the
manifest. Export v1 may still require the resolved part to be a supported
WordprocessingML main document.

### package preservation rule

On export:

1. Reopen cached `source.docx`, never the user's original path.
2. Verify its SHA-256 fingerprint against `docx_source.json`.
3. Reparse the main document and validate every block anchor and source-text
   hash before mutation.
4. Raw-copy every ZIP entry other than the main document part.
5. Write one modified main document part with original non-text XML events and
   namespace declarations preserved.
6. Validate the finished package before commit.
7. Atomically replace a temporary file in the destination directory.

The exporter must not deliberately edit styles, numbering, relationships,
headers, footers, comments, metadata, thumbnails, media, charts, or embedded
custom XML in v1.

### security limits

Start with explicit, format-specific constants. Proposed defaults for the spike:

| Limit | Proposed v1 value |
| --- | ---: |
| compressed source file | 100 MiB |
| ZIP entries | 20,000 |
| total declared and actually read uncompressed bytes | 512 MiB |
| one XML part | 64 MiB |
| main-document paragraphs | 100,000 |
| extracted logical text | 20 million Unicode scalar values |
| XML nesting depth | 256 |

These are provisional until the corpus spike records real sizes. Enforcement
must use both archive metadata and counted streaming reads because ZIP metadata
alone is untrusted.

Reject:

- absolute paths, `..`, unsafe separators, NULs, duplicate entry names, and
  normalized-name collisions;
- unsupported compression or malformed central-directory records;
- DTD/DOCTYPE declarations and any entity-based external resolution attempt;
- `vbaProject.bin`, macro-enabled main content types, ActiveX parts, and `.docm`;
- encrypted/OLE compound packages presented as `.docx`;
- limits exceeded during inspection or actual decompression;
- malformed XML, missing required package relationships, and ambiguous main
  parts.

External relationships may be preserved as inert package data, but Rosetta must
never dereference or fetch them. The parser records their presence for
diagnostics without logging document text or target URLs.

## 7. Extraction and IR Mapping

### traversal boundary

Parse only the main document body in v1:

- body-level `w:p` -> paragraph or heading block;
- `w:tbl/w:tr/w:tc` paragraphs -> table-cell blocks;
- nested tables -> stable nested table coordinates;
- drawing/text-box subtrees -> preserved but not extracted;
- section properties, bookmarks, proofing markers, and drawings -> structural,
  not translation text;
- `w:instrText` and deleted text -> never translation text.

Namespace handling must work by resolved namespace/local name and must not
depend only on a single Transitional OOXML prefix. Strict OOXML is accepted only
after its fixture passes extraction and export; otherwise it receives a clear
unsupported-profile error.

### block mapping

| OOXML structure | Rosetta block | Required provenance |
| --- | --- | --- |
| normal body paragraph | `paragraph` | part, paragraph ordinal, source hash |
| outline/heading paragraph | `heading` | heading level, style id |
| paragraph with `numPr` | `list_item` | `numId`, level, list kind if resolved |
| paragraph inside table cell | `table_cell` | table path, row, column, cell paragraph |
| empty/structural paragraph | omitted or skipped metadata | deterministic rule |
| unsupported protected paragraph | skipped source block | reason flag |

Heading detection should prefer effective `w:outlineLvl` after resolving style
inheritance. Localized style display names are a fallback, not the primary
contract. List kind is derived from `numbering.xml` when available; export does
not reconstruct numbering because original paragraph properties remain intact.

### logical text

Construct paragraph text from eligible `w:t`, `w:tab`, and line-break elements
in document order. Respect `xml:space="preserve"`. Normalize only the minimum
needed for the existing segmenter; preserve intentional tabs and breaks in the
source block text.

Long paragraphs still use the existing `MAX_SEGMENT_CHARS` splitter. Export
reconstructs one translated block from its ordered translation segments using
the same target-language join rule as preview/export today.

### block provenance

Use both human-inspectable block metadata and a dedicated durable manifest.

Example block fields:

```json
{
  "path": "word/document.xml#p-000042",
  "style": {
    "docx": {
      "part": "word/document.xml",
      "paragraphIndex": 42,
      "sourceTextHash": "sha256:...",
      "paragraphStyleId": "Heading1",
      "headingLevel": 1,
      "numbering": null,
      "table": null,
      "flags": []
    }
  }
}
```

`path` is a Rosetta diagnostic identity, not an XPath evaluated against an
arbitrary file. The cached source package is immutable, so a document-order
paragraph ordinal plus source hash is sufficient to detect stale/corrupt
anchors.

## 8. Durable Artifacts and Compatibility

DOCX job directory:

```text
<job>/
  source.docx
  docx_source.json
  document.json
  segments.json
  translation_files.json
  translations/
  translation_revisions.json
  exports/
```

Proposed `docx_source.json`:

```json
{
  "schemaVersion": 1,
  "sourceFingerprint": "sha256:...",
  "sourceBytes": 1234567,
  "filename": "report.docx",
  "originalPath": "C:/Users/.../report.docx",
  "mainDocumentPart": "word/document.xml",
  "packageProfile": "transitional",
  "importedAt": "1786000000000",
  "blocks": [
    {
      "blockId": "document-job-block-1",
      "paragraphIndex": 42,
      "sourceTextHash": "sha256:...",
      "textNodeCount": 3,
      "flags": []
    }
  ]
}
```

The manifest is export authority and has its own schema version. Do not place
large arrays of XML node details into `document.json` unless a consumer needs
them outside DOCX import/export.

Compatibility policy:

- Existing TXT, Markdown, and PDF jobs remain readable without migration.
- The core schema can remain version 1 if DOCX is added only as a new format
  value and optional `style.docx` metadata. Confirm this in the implementation
  ADR.
- Old Rosetta builds not understanding `docx` will not load new DOCX jobs; new
  builds must still load every old job.
- Missing or invalid `source.docx`/`docx_source.json` makes export unavailable
  with a repair/re-import message. It must not fall back to generating a new
  document from incomplete IR.
- Implementation must update `conventions/data-models.md` with the artifact,
  fingerprint, anchor, cleanup, and compatibility rules.

## 9. Text Replacement Policy

The difficult part is mapping translated paragraph text back to run-level XML.
V1 prioritizes a valid, structurally intact document over exact inline-style
alignment.

### simple paragraph path

A simple paragraph contains eligible text runs but no fields, tracked changes,
comments, content controls, drawings/text boxes, or other protected islands.

For a simple paragraph:

1. Verify the paragraph source hash.
2. Select a carrier run using a deterministic policy: first non-empty eligible
   run, preferring the paragraph's dominant run properties when that can be
   determined without moving structural nodes.
3. Put reconstructed translated block text in the carrier `w:t`.
4. Empty the remaining eligible source `w:t` nodes without deleting surrounding
   run properties or structural siblings.
5. Set or remove `xml:space="preserve"` based on translated leading/trailing
   whitespace.
6. Convert intentional translated line breaks to `w:br` within the carrier run;
   do not create new paragraphs from model output.

This retains paragraph, list, table, style, and section structure. Mixed inline
formatting may collapse to the carrier run and is explicitly best-effort in v1.

### protected structures

- `w:instrText` is never translated.
- `w:fldSimple` and complex field ranges are preserved unchanged.
- TOC, page-number, citation, and reference field results are not authoritative
  translation text because Word may regenerate them.
- Any tracked changes (`w:ins`, `w:del`, move ranges) cause a fail-closed import
  error in v1 with guidance to accept/reject changes in the source first.
- Paragraphs containing comments, bound/locked content controls, equations,
  text boxes, or mixed field text are initially marked skipped unless the CP0
  fixture proves a bounded mutation rule.
- Hyperlink relationships and destination data are always preserved. CP0 must
  choose one of two safe behaviors for linked display text: a validated
  span-aware patch, or skipping the containing paragraph. Do not silently move
  an entire paragraph into a hyperlink or discard the visible link.

This hyperlink decision is the only intended open policy after this plan. It
must be frozen in the ADR before CP2 extraction code lands.

## 10. Preview and Frontend Integration

Reuse `DocumentPreview` and its block virtualizer.

Required changes:

- add `docx` to `RosettaSourceDocumentFormat`;
- add import filter/copy and drag/drop format validation;
- use a Word/document icon from the existing icon library;
- add `.docx` naming/filter support in `rosettaExport.ts` and the Tauri save
  dialog;
- dispatch DOCX to its binary exporter instead of `render_export_blocks`;
- render `heading` with a restrained heading treatment;
- render `list_item` with indentation and a generic marker in preview;
- render `table_cell` with subtle row/column context, not a fake paginated Word
  canvas;
- keep source/translation hover, selection, and block synchronization working;
- keep all long-document previews virtualized.

No DOCX-specific Zustand slice should be needed. Persistent package/provenance
state belongs in Rust and job artifacts, while active file/block selection stays
in the existing UI state.

## 11. Checkpoint Plan

### CP0: compatibility and mutation spike (1.5 to 2 days)

Deliverables:

- sanitized DOCX fixture corpus and provenance README;
- bounded test-only package inspector;
- prototype main-document paragraph extraction;
- prototype export that raw-copies all entries and changes one simple paragraph;
- ZIP-entry checksum report showing only the main document part changed;
- manual open results from current Microsoft Word and LibreOffice.

Acceptance:

- simple Word, LibreOffice, and Google Docs-exported fixtures open without a
  repair prompt after mutation;
- images, list numbering, table structure, styles, headers/footers, and page
  setup remain present;
- source and output have the same package-entry name set;
- hyperlink policy is chosen and written into the pending ADR;
- unsupported tracked-change and encrypted fixtures are detected, not damaged.

Stop conditions:

- Word or LibreOffice requests package repair on ordinary fixtures;
- raw-copy export loses or rewrites unrelated parts;
- paragraph anchors cannot be validated deterministically;
- the team cannot state a fail-closed policy for fields, links, and revisions.

### CP1: secure package reader and source cache (2 days)

Deliverables:

- `formats/docx/package.rs`, `policy.rs`, and `provenance.rs`;
- format-specific quotas and structured errors;
- OPC main-part discovery and content-type validation;
- `source.docx` and atomic `docx_source.json` persistence;
- source fingerprint validation and cleanup behavior;
- ADR for package-copy plus narrow XML mutation.

Acceptance:

- valid packages pass preflight before a job is indexed;
- malformed, oversized, encrypted, macro-enabled, path-traversal, duplicate-name,
  DTD, and decompression-limit fixtures fail with stable user-facing errors;
- failure leaves no phantom job or partial durable bundle;
- external relationships are never fetched and their URLs are not logged.

### CP2: main-document extraction into Rosetta IR (3 to 4 days)

Deliverables:

- streaming `document.xml`, `styles.xml`, and `numbering.xml` readers;
- paragraph, heading, list, and table-cell mapping;
- deterministic block IDs, paths, style metadata, and manifest anchors;
- paragraph logical-text construction and segment generation;
- skipped/protected block diagnostics without source-text logging.

Acceptance:

- fixture snapshots prove reading order, heading levels, list levels, and nested
  table coordinates;
- every segment references an existing block and every manifest entry references
  one DOCX block;
- all translatable block source hashes match the cached package;
- long fixtures stay within memory/time budgets recorded by a small benchmark;
- unsupported-only documents return a clear reason instead of appearing ready
  with zero segments.

### CP3: normal translation workflow and structural preview (1.5 to 2 days)

Deliverables:

- Rust and TypeScript source-format integration;
- single-file import picker and workbench labels/icons;
- existing translation-file/revision workflow exercised with DOCX blocks;
- virtualized heading/list/table-cell preview;
- DOCX-specific disabling of bilingual export.

Acceptance:

- import, language selection, whole-file translation, selection retranslation,
  retry, and app restart reuse the existing durable translation facts;
- opening a DOCX does not probe or install the PDF runtime;
- a 10,000-block synthetic document does not render all blocks at once;
- TXT, Markdown, and PDF behavior is unchanged.

### CP4: translated DOCX export (3 to 4 days)

Deliverables:

- DOCX-specific export dispatch and binary result accounting;
- source/anchor/hash validation before mutation;
- simple-paragraph run replacement and frozen protected-structure behavior;
- raw copying of unchanged ZIP entries;
- output package validation and atomic destination commit;
- `.docx` default filename and save filter.

Acceptance:

- no export occurs while required segments are pending, translating, failed, or
  empty;
- source-package fingerprint or anchor mismatch fails before writing the user
  target;
- Word and LibreOffice open every accepted output without repair prompts;
- entry-set equality holds and byte/checksum equality holds for every unchanged
  compressed package entry where the ZIP library exposes raw copying;
- headings, lists, tables, images, headers/footers, styles, numbering, links
  under the frozen policy, and page setup survive;
- cancelling or failing export does not replace an existing destination file.

### CP5: compatibility gate, documentation, and release readiness (2 to 3 days)

Deliverables:

- completed cross-producer fixture matrix;
- manual Word/LibreOffice evidence recorded in a benchmark or plan checkpoint;
- data-model convention update;
- implementation change-log entry;
- ADR status finalized;
- user-facing limitation copy reviewed;
- stale plan assumptions corrected from measured results.

Acceptance:

- required validation commands pass;
- privacy review finds no source/translation text or relationship target logging;
- no new Tauri permission is added unless separately documented;
- failure messages explain whether the user should remove encryption, accept
  tracked changes, simplify unsupported content, or re-import a damaged cache;
- release notes describe only demonstrated behavior.

## 12. Fixture and Acceptance Matrix

Fixtures must contain synthetic or redistributable text only.

| Producer/feature | Import | Translate | Export/open | Required result |
| --- | --- | --- | --- | --- |
| Word, plain paragraphs | yes | yes | Word + LibreOffice | no repair, order preserved |
| Word, Heading 1-3 | yes | yes | both | hierarchy and styles survive |
| Word, bullets/numbers/nesting | yes | yes | both | numbering definitions survive |
| Word, merged/nested tables | yes | yes | both | cell order and table XML survive |
| Word, images and captions | body only | body only | both | image bytes and placement survive |
| Word, headers/footers/page setup | body only | body only | both | unchanged parts survive |
| Word, mixed bold/italic | yes | yes | both | documented carrier-style behavior |
| Word, internal/external hyperlinks | policy | policy | both | destination preserved, never fetched |
| Word, TOC and fields | protected | no field code | both | fields preserved unchanged |
| Zotero/Word citation fields | protected | bounded body policy | both | no field corruption |
| comments | protected/skipped | no | both | no false support claim |
| tracked changes | reject | no | n/a | actionable fail-closed error |
| equation/text box/shape | skip | no | both | unchanged package content |
| LibreOffice Writer | yes | yes | both | no repair and stable structure |
| Google Docs export | yes | yes | both | no repair and stable structure |
| Strict OOXML | conditional | conditional | both | pass corpus or reject clearly |
| encrypted/OLE `.docx` | reject | no | n/a | explicit unsupported error |
| disguised `.docm`/VBA | reject | no | n/a | active-content error |
| ZIP traversal/duplicate/bomb | reject | no | n/a | bounded failure, no partial job |
| malformed XML/relationships | reject | no | n/a | bounded failure, no panic |
| 100k-paragraph boundary | bounded | sampled | no manual gate | no unbounded UI render |

At least one accepted output per producer must be opened manually in both Word
and LibreOffice. Programmatic XML validity is necessary but not sufficient;
Office's repair prompt is a release blocker.

## 13. Validation Commands

Per checkpoint, run the narrowest relevant tests plus the repository baseline:

```powershell
cd rosetta-app
pnpm typecheck

cd src-tauri
cargo check
cargo test rosetta_jobs
```

Add focused Rust tests such as:

```powershell
cargo test rosetta_jobs::formats::docx
```

Do not run a dev server or production build unless runtime UI verification or
release packaging is explicitly requested. CP3 visual verification, when
authorized, should check that virtualization remains active and that heading,
list, and table-cell treatments do not cause overlap at desktop window sizes.

## 14. Documentation Required During Implementation

Before CP2 lands:

- add the next ADR documenting package-copy/narrow-mutation authority,
  dependency choice, source fingerprinting, run replacement, and protected
  structures;
- update this plan with CP0 evidence and the hyperlink decision.

Before release:

- update `docs/engineering/conventions/data-models.md` for `source.docx`,
  `docx_source.json`, schema compatibility, repair, export, and cleanup;
- add one aggregate DOCX implementation entry under
  `docs/engineering/change-log/`;
- update product-plan wording only if measured v1 behavior differs from its
  current basic DOCX promise;
- add release notes only after Word and LibreOffice acceptance gates pass.

## 15. Risks and Mitigations

### silent round-trip loss

Mitigation: original-package authority, raw copying, changed-entry checksum
tests, package validation, and mandatory Word/LibreOffice open gates.

### run fragmentation and inline styles

Mitigation: paragraph-level translation, deterministic carrier-run policy,
explicit best-effort inline styling, and fail-closed protected structures.

### Word fields and review history

Mitigation: never translate field instructions; reject tracked changes; skip
unsupported protected paragraphs until a fixture-backed rule exists.

### ZIP/XML denial of service

Mitigation: pre-index limits, counted streaming reads, XML depth/text limits,
path/name validation, and no archive extraction to arbitrary filesystem paths.

### over-expanding product scope

Mitigation: main body only, single file only, translated DOCX only, normal
Rosetta workbench, and no Office runtime/cloud dependency.

### large document memory use

Mitigation: event-based XML parsing, bounded source text, block virtualization,
and package-entry streaming/raw copy. The implementation must not retain both a
full decompressed package and a second full package model in memory.

## 16. Final Go/No-Go Gate

Proceed from planning to full implementation only when CP0 demonstrates all of
the following:

- package-copy export opens without repair in Word and LibreOffice;
- unrelated package parts survive byte-for-byte where raw copy permits;
- paragraph source anchors and hashes are deterministic;
- the team accepts the documented v1 losses around inline formatting and
  protected structures;
- the security policy rejects active, encrypted, malformed, and unbounded input
  before durable job creation.

If any gate fails, keep DOCX in research status. Do not ship a converter that
looks successful while silently deleting Word content.
