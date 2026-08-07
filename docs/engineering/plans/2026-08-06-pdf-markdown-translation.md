# PDF Markdown Translation Implementation Plan

Date: 2026-08-06

Status: Checkpoint 0 Go; Checkpoint 1 model and migration implemented

Decision authority: [ADR 0078](../decisions/0078-pdf-markdown-pymupdf4llm-layout.md)

## 1. Outcome

After importing one PDF, the user can choose the translated output in the
right workbench:

- `PDF`: current visual translation, current page selection, current preview,
  current `pdf2zh` execution and `.pdf` export.
- `Markdown`: whole-document structured extraction, block translation,
  side-by-side structured preview and `.md` plus local image assets export.

Both outputs can coexist for the same PDF and target language. Switching the
control changes the active output artifact; it does not convert, delete or
invalidate the other artifact.

## 2. V1 Boundaries

### Included

- Non-OCR PDFs with a usable text layer.
- English and Simplified Chinese translation directions already supported by
  Rosetta's translation runtime.
- Headings, paragraphs, lists, captions, footnotes, tables, pictures and
  formulas.
- Complete-document extraction with windowed recovery.
- Local-only processing and local asset export.
- Existing selection retranslation after extraction, using normalized blocks.

### Not included

- OCR or scanned/image-only PDF conversion.
- Partial-page-range Markdown documents.
- Equation translation, LaTeX reconstruction or code-language detection.
- Reproducing PDF pagination, typography or exact visual layout in Markdown.
- Calling a language model to repair reading order, infer Markdown syntax or
  regenerate omitted source content.
- Bilingual Markdown export in v1.
- Replacing or upgrading the production `pdf2zh` execution path.

When no usable text layer is found, return a typed `ocr-required` state and
explain that Markdown conversion is unavailable for that PDF in v1. Do not
silently export an empty or image-only Markdown file.

## 3. Product Behavior

### Output selector

Add a compact `PDF | Markdown` segmented control to `WorkspaceTopbar` for PDF
sources. Persist the selected output per `(jobId, sourceFileId)`. Default to
`PDF` for old and newly imported jobs, so current behavior is unchanged.

`PDF` mode restores the current page navigation, page-range selection, force
retranslation, PDF progress, visual preview and PDF export controls.

`Markdown` mode replaces the page controls with whole-document extraction and
segment status. It uses the existing virtualized two-pane block preview after
extraction. The export command is `导出 Markdown`; the save dialog defaults to
`<source>.<targetLang>.md`.

### First use

Selecting `Markdown` must not start a download without an explicit user
action. The mode shows one of these states:

```text
component-not-installed -> installing -> component-ready
                                      -> install-failed

extraction-absent -> extracting -> ready
                               -> failed
                               -> cancelled
ready + policy/source mismatch -> stale -> extracting
```

After the managed component is ready, `开始解析` extracts the complete PDF.
Extraction progress reports committed pages, for example `已解析 16 / 80 页`.
Once normalized blocks are ready, the ordinary translation action becomes
available.

### Concurrent work and switching

- Switching output modes does not cancel an active run.
- The active run remains visible in its mode when the user returns.
- V1 allows at most one mutating run per job: PDF translation, Markdown
  extraction or Markdown translation. A second start action is disabled with
  the current operation named in its tooltip.
- Cancellation of Markdown extraction kills and recreates only the Markdown
  worker. Previously committed page shards remain resumable.
- Changing target language selects or creates another Markdown translation
  file over the same source IR; it does not re-extract the PDF.

## 4. Data Model and Compatibility

### Translation output identity

Add this shared type in Rust and TypeScript:

```ts
type RosettaOutputFormat = "txt" | "markdown" | "pdf";
```

Add required `outputFormat` to `RosettaTranslationFile`. Change translation
file lookup and creation from `(sourceFileId, targetLang)` to
`(sourceFileId, targetLang, outputFormat)`.

Backward-compatible decode rules:

| Existing source format | Missing `outputFormat` infers | Existing ID |
| --- | --- | --- |
| `pdf` | `pdf` | preserved |
| `markdown` | `markdown` | preserved |
| `txt` | `txt` | preserved |

ID rules:

- Preserve `tr-{source}-{target}` for the inferred/native output. This avoids
  renaming existing translation segment files.
- Use `tr-{source}-{target}-{output}` for a non-native sibling output, such as
  Markdown generated from PDF.
- Reject duplicate triples during load; never choose one by array order.
- Do not eagerly rewrite `translation_files.json`. Persist the inferred field
  on the next normal mutation.

Update `ensure_rosetta_translation_file` to require `outputFormat`. Validate
the matrix: PDF sources allow `pdf` and `markdown`; Markdown sources allow
`markdown`; TXT sources allow `txt`.

### Active selection

Replace the frontend's translation selection key with
`jobId:sourceFileId:outputFormat`. Add persisted
`activeOutputFormatBySourceKey`, defaulting PDF sources to `pdf`. A mode switch
loads the exact triple instead of the first translation file for a source.

### Source IR

After successful Markdown extraction:

- Keep `document.format` and the source file's `format` as `pdf`.
- Atomically replace only that PDF file's normalized `document.blocks`,
  `blockIds` and source `segments` from committed extraction shards.
- Generate stable IDs from extraction policy, one-based page number, box
  ordinal and table-cell coordinates. Re-extracting unchanged bytes with the
  same policy must produce identical IDs.
- Keep `document.extractionStatus` out of the new state machine. Its old
  Docling-oriented comment and behavior should be deprecated separately;
  `pdf-markdown/manifest.json` is the extraction authority.

The current PDF page pipeline must continue to ignore `document.blocks` and
`segments`. Specifically:

- PDF progress remains derived from durable page/run state.
- Markdown translation progress remains derived from the format-qualified
  translation file.
- `syncJobWithTranslationFile` must not project a secondary Markdown output
  into the legacy PDF source/job segment counters.
- Deleting the job removes both output families through the existing job-root
  deletion boundary. Switching modes never deletes either family.

Update `docs/engineering/conventions/data-models.md` in the first model-change
commit, after the decoder and migration tests pass.

## 5. Extraction Contract

### Pinned engine call

Use an isolated worker with:

```python
pymupdf4llm.to_json(
    source_pdf,
    pages=zero_based_page_indexes,
    use_ocr=False,
    force_text=False,
    write_images=True,
    image_path=window_temp_image_dir,
)
```

Pin and report exact versions:

- `pymupdf4llm == 1.28.0`
- `pymupdf-layout == 1.28.0`
- `PyMuPDF == 1.28.0`
- CPU ONNX Runtime supplied by the managed PDF component

Do not use `to_markdown()` as a storage or export boundary. Do not pass
`header=False` or `footer=False` as the only filter: the experiment confirmed
that a `page-header` box can still be returned. Filter by `boxclass` in the
normalizer.

### Windowing and commit

- Default window size: 8 pages; hard bound: 10 pages.
- One worker request handles one window and writes only under
  `pdf-markdown/.tmp/<runId>/`.
- Validate schema, page numbers, coordinates, image paths, byte limits and
  engine versions before commit.
- Commit one gzip JSON shard per page and canonicalized images by atomic
  rename, then update `manifest.json` atomically.
- Resume from the first missing or invalid page shard. A completed shard is
  immutable for one `(source fingerprint, policy version)`.
- On cancellation, terminate the Markdown worker process group. Do not expect
  cooperative cancellation while `to_json()` is blocked.
- Never write source or translated text to normal diagnostics. Debug text
  diagnostics require the existing explicit PDF diagnostics opt-in.

### Manifest

Use a versioned manifest containing at least:

```json
{
  "schema": "rosetta-pdf-markdown-extraction/1",
  "sourceFingerprint": "sha256:...",
  "pageCount": 80,
  "engine": {
    "pymupdf4llm": "1.28.0",
    "pymupdfLayout": "1.28.0",
    "pymupdf": "1.28.0"
  },
  "policyVersion": "rosetta-pdf-markdown-normalizer/1",
  "useOcr": false,
  "forceText": false,
  "writeImages": true,
  "committedPages": [1, 2]
}
```

Do not store absolute source paths, credentials, provider endpoints, source
text or translated text in the manifest.

## 6. Deterministic Normalization

Map PyMuPDF4LLM box classes as follows:

| `boxclass` | Rosetta representation | Translation/export policy |
| --- | --- | --- |
| `title` | `heading`, level 1 | translate |
| `section-header` | `heading`, initial level 2 | translate |
| `text` | `paragraph` | translate |
| `list-item` | `list_item` | translate; preserve list metadata |
| `caption` | `caption` | translate; associate with nearest picture/table when unambiguous |
| `footnote` | `footnote` | translate |
| `table` | row-major `table_cell` blocks | translate cell text only |
| `picture` | `metadata` image reference | do not translate; export canonical image |
| `formula` | `code` with formula metadata | preserve original in v1 |
| `page-header` / `page-footer` | no normalized block | omit |

Every normalized block `style` uses a versioned payload containing page
number, source box class, bounding box and only the fields needed to render or
associate structure. Table cells also carry table ID, zero-based row/column,
row/column span and header status. Picture blocks carry a job-relative image
path, dimensions and optional caption block ID; never base64 image data.

Normalization rules:

- Preserve the vendor page/box reading order. Do not re-sort globally by x/y.
- Collapse only whitespace that is unambiguously layout-generated. Do not add
  language-specific word-joining heuristics in v1.
- `force_text=False` prevents recognized text inside a picture from entering
  body paragraphs. An image with no extracted text is valid.
- Rectangular tables without spans render as GFM pipe tables. Tables with
  spans or an unsafe grid render as deterministic inline HTML tables; never
  flatten them into guessed prose.
- Missing formula text or an unsafe table is preserved with an explicit
  source placeholder/structure; it is not regenerated by the translation
  model.
- Reject a page shard if box/page identity is inconsistent, a referenced image
  escapes the temp root, decompressed JSON exceeds its bound, or normalized
  block/character counts exceed defensive per-page limits.

Rosetta's segmenter receives plain textual payloads only. Markdown markers,
image links, table delimiters and heading prefixes are generated at preview or
export time, after translated text is selected.

## 7. Worker and Managed Component

Create a separate `managed_pdf_markdown` boundary modeled on the useful
lifecycle and process-safety patterns in `managed_pdf2zh`, without sharing its
worker state.

### Worker protocol

Use line-delimited JSON with bounded messages:

- request: `hello`, `extractWindow`, `shutdown`
- response: `ready`, `windowProgress`, `windowResult`, `error`

`hello` returns protocol and exact package versions. `extractWindow` accepts
only the internally resolved source path, zero-based page indexes and an
internally resolved temp directory. Tauri derives all paths from a checked job
root; the frontend never supplies filesystem roots.

The Windows profile may use the existing managed PDF pack's CPython executable
with the Markdown overlay first on `PYTHONPATH`; this is the configuration
already validated by the spike. Sanitize inherited Python variables. Starting
the ordinary `pdf2zh` worker without that overlay must continue to resolve its
existing PyMuPDF version.

Do not assume the macOS and Linux PDF packs expose a reusable interpreter.
Their release spike must select an explicit profile-owned Python host or a
self-contained Markdown worker while preserving the same process and version
isolation. Cross-platform archive sizes are unknown until that spike passes.

### Packaging strategy and gates

The current Windows CPython 3.12 overlay experiment is the correctness
baseline:

- 97.32 MiB unpacked.
- 60.3 MiB ZIP.
- Reuses the existing pack's Python, NumPy, ONNX Runtime, NetworkX and YAML.
- Runs PyMuPDF 1.28.0 in the Markdown worker while the unchanged `pdf2zh`
  worker still resolves PyMuPDF 1.25.2.

It is not a release artifact. It still contains about 5.55 MiB of
`__pycache__` and 6.43 MiB of `mupdf-devel` files, and the current Windows PDF
pack plus overlay is 429,302,670 bytes (429.3 MB, 409.4 MiB) compressed.

Produce deterministic Windows x64, macOS arm64 and Linux x64 overlays. Strip
only files proven unnecessary by a clean-machine preflight; do not delete
vendor model variants based on filename guesses. Record archive SHA-256,
compressed/unpacked bytes and file count in managed profiles.

Release gates:

- Main Tauri installer increase: at most 5 MiB per platform.
- No Torch, Transformers, CUDA runtime or OCR model dependency.
- Windows cumulative compressed managed PDF components: at most 400 MiB.
- Record Markdown worker peak RSS on the release corpus for capacity planning;
  the 400 MiB release gate applies to cumulative downloaded managed PDF
  components, not runtime memory.
- Clean-machine install, repair, cancellation and offline restart pass on all
  supported release platforms.
- Ordinary PDF visual translation passes its existing regression suite with
  the Markdown overlay installed and uninstalled.

If the 400 MiB cumulative Windows gate is not met, stop before default rollout.
Acceptable next work is a reproducible audit/rebuild of the shared base PDF
pack; silently merging the two Python environments or changing the production
worker's PyMuPDF version is not acceptable.

## 8. Export Contract

Markdown export takes the active Markdown translation-file ID, not only the
source format. Validate `outputFormat === "markdown"` in Rust.

Destination layout:

```text
document.zh-CN.md
document.zh-CN.assets/
  page-0001-picture-01.png
```

Use relative links such as
`![translated caption](document.zh-CN.assets/page-0001-picture-01.png)`.
Deduplicate image bytes within one export by content hash while retaining
stable logical names in the generated Markdown.

Export is a staged multi-file transaction:

1. Render Markdown and copy referenced images into a sibling temp directory.
2. Verify every relative link resolves inside the staged asset directory.
3. Flush and atomically replace the destination Markdown file.
4. Replace the asset directory using a recoverable sibling rename strategy.
5. On failure, keep the previous successful export intact and remove staging
   best effort.

The export result reports aggregate bytes and files written. It must not expose
job-internal cache paths. Images without captions still export with empty alt
text; captions are not duplicated as a second paragraph unless the source
structure actually contains both.

## 9. Implementation Checkpoints

### Checkpoint 0: Release-quality spike and corpus

Deliverables:

- Promote the current extraction/overlay commands into reproducible scripts
  under `rosetta-app/src-tauri/scripts/`.
- Create a local, non-redistributed manifest for a 24-document acceptance
  corpus: single column, multi-column, reports, papers, manuals, CJK, tables,
  figures/captions, footnotes, formulas and long documents.
- Capture cold start, warm seconds/page, peak RSS, output structure defects and
  archive sizes.
- Verify the decisive complex-figure sample with `force_text=False` and
  `write_images=True`.

Go criteria:

- No critical body-text omission, duplication or cross-column interleaving on
  the golden pages.
- Figure-internal text does not pollute body text on the decisive sample.
- Warm extraction median is at most 0.6 seconds/page and p95 at most
  1.5 seconds/page on the test machine.
- Packaging gates in section 7 either pass or have a measured, bounded pack
  rebuild checkpoint approved before product wiring proceeds.

#### Checkpoint 0 execution record (2026-08-06)

Checkpoint 0 tooling is now reproducible under `rosetta-app/src-tauri/scripts/`:

- `pdf_markdown_checkpoint0.py` validates the 24-document manifest, exact
  package versions and extraction policy, then records cold start, warm
  seconds/page, process-tree peak RSS and structure-review flags. It persists
  only sanitized document IDs and job-relative image names in benchmark JSON.
- `pdf-markdown-corpus-manifest.json` fixes 24 non-redistributed PDFs by root
  alias, relative path, byte count, page count and SHA-256. The local corpus is
  240 pages / 26,958,964 bytes and covers the categories required above.
- `build-pdf-markdown-overlay.py` downloads exact target wheels, constructs a
  deterministic platform overlay, removes bytecode/development files and only
  model variants outside the pinned `DocumentLayoutAnalyzer` default runtime
  closure, then records archive identity and size. The Windows preflight runs
  a real PDF after pruning and verifies CPU-only ONNX execution.
- `requirements-pdf-markdown-overlay.txt` pins `pymupdf4llm 1.28.0`,
  `pymupdf-layout 1.28.0`, `PyMuPDF 1.28.0` and `tabulate 0.10.0`.

The 8-page recovery/commit window remains unchanged. To stay within the RSS
gate, one worker request invokes `to_json()` once per page inside that window,
releases page-local memory, then combines the structured page results before
the atomic window write. This remains a `to_json()` integration boundary and
does not use `to_markdown()` as storage or export authority.

Windows x64 result, measured with the installed production PDF pack's CPython
3.12 host and the trimmed overlay first on `PYTHONPATH`:

| Metric | Result | Gate |
| --- | ---: | ---: |
| Corpus | 24 documents / 240 pages | 24 documents |
| Cold worker ready | 1.973 s | recorded |
| Cold first-page extraction | 2.268 s | recorded |
| Warm median | 0.333 s/page | <= 0.6 s/page |
| Warm p95 | 0.623 s/page | <= 1.5 s/page |
| Peak process-tree RSS | 407,248,896 bytes / 388.4 MiB | recorded |
| Windows overlay | 29,985,992 bytes / 28.6 MiB | measured |
| Windows base + overlay | 396,059,375 bytes / 377.7 MiB | <= 400 MiB |

The original 63,229,287-byte overlay became a 29,985,992-byte deterministic
archive after removing 43,758,368 unpacked bytes. The retained layout resource
closure is `layout_rf2.4.1+imf1`, `feature_imf1` and
`table_grid_model_v4_ep`, matching the pinned package's default configuration.
Two independent Windows builds produced SHA-256
`f2e01a2df1a4c5aaa74114dbb49f1473b2082104f1aee23eeb3407ded13ac2fc`.

Isolation preflight passed:

- overlay worker: PyMuPDF/PyMuPDF4LLM/Layout `1.28.0`;
- ONNX provider selected by the layout model: `CPUExecutionProvider` only;
- production worker before and after overlay execution: PyMuPDF `1.25.2`;
- extraction policy: `use_ocr=False`, `force_text=False`,
  `write_images=True`.

Cross-platform archive construction from the exact target wheels produced:

| Platform | Overlay archive | Existing base + overlay | Native preflight |
| --- | ---: | ---: | --- |
| Windows x64 | 29,985,992 bytes / 28.6 MiB | 396,059,375 bytes / 377.7 MiB | passed |
| macOS arm64 | 34,622,507 bytes / 33.0 MiB | 429,985,090 bytes / 410.1 MiB | passed on native host |
| Linux x64 | 36,480,503 bytes / 34.8 MiB | 511,686,286 bytes / 488.0 MiB | passed on native host |

The macOS archive SHA-256 is
`9a362d58227f6cb1159b8fa1520c23cc3ead951ae4d5f9abcf5153d9171fb6a9`;
the Linux archive SHA-256 is
`fa2ca9e5e66e2f1930cbb200a2b4a9001d5e8f4e2c256d45844462e0cdab447e`.
Native builds on macOS arm64 and Linux x64 reproduced those exact byte counts
and SHA-256 identities. Each build used the installed platform PDF pack's
profile-owned CPython 3.12 host, selected `CPUExecutionProvider` only, loaded
overlay PyMuPDF `1.28.0`, ran a real-PDF `to_json()` preflight and left the
production worker on PyMuPDF `1.25.2` before and after execution. The macOS
build used an offline wheelhouse after the host's direct PyPI transfer stalled;
the builder and resulting archive were otherwise unchanged.

Native 24-document / 240-page corpus result:

| Metric | macOS arm64 | Linux x64 | Gate |
| --- | ---: | ---: | ---: |
| Cold worker ready | 1.926 s | 1.462 s | recorded |
| Cold first-page extraction | 0.405 s | 1.141 s | recorded |
| Warm median | 0.139 s/page | 0.275 s/page | <= 0.6 s/page |
| Warm p95 | 0.300 s/page | 0.584 s/page | <= 1.5 s/page |
| Peak process-tree RSS | 1,148,796,928 bytes / 1,095.6 MiB | 472,555,520 bytes / 450.7 MiB | recorded |

Both native reports reproduced the Windows structure counts: one adjacent
duplicate-body flag, six empty-body edge pages, one reviewed picture/body
overlap, zero invalid page identities and zero unknown box classes. The same
seven sanitized MuPDF color-space warnings appeared on `GeoTopo`; extraction
completed.

The elevated RSS is reproducible outside the full corpus. On macOS, a fresh
single-page decisive-sample process reached 539,525,120 bytes / 514.5 MiB RSS;
on Linux it reached 285,655,040 bytes / 272.4 MiB. Limiting OpenMP, OpenBLAS
and Accelerate threads to one did not materially change either result. The
pinned layout package already disables the ONNX CPU memory arena, so that
setting is not an available further reduction. These measurements remain a
capacity and optimization risk, not a Checkpoint 0 release-size failure.

Structure review result:

- The decisive `2206.01062.pdf` page 1 has one picture plus one caption and
  zero body boxes overlapping the picture. Figure-internal labels do not enter
  body order with `force_text=False`.
- Automated review reported one adjacent repeated body box and one
  picture/body overlap. Visual/source review showed that the repetition is
  present in the source fixture and the overlap is a designed IBM sidebar with
  valid list text, not duplicated extraction or figure-label pollution.
- Six pages have no normalized body boxes. One is blank; the others are
  picture/header-only edge pages, including an RTL sample outside v1. None is a
  golden-page body omission in the supported English/Simplified-Chinese scope.
- Golden pages show no critical body omission, extraction-added duplication or
  cross-column interleaving. Non-critical defects remain: word-join noise on
  several tightly typeset scientific/table pages and one punctuation-only
  heading in the manual sample. Formula content remains source-preserved and
  table JSON contains row/column/cell structure.
- MuPDF emitted seven color-space parse warnings while processing the 117-page
  `GeoTopo` fixture; extraction completed with valid page identity. This stays
  a hardening risk for later worker diagnostics because normal logs must not
  include source paths or content.

Checkpoint 0 acceptance:

| Condition | Result |
| --- | --- |
| Golden-page critical quality | pass on Windows, macOS and Linux corpus |
| Decisive complex-figure isolation | pass |
| Warm median and p95 | pass on Windows, macOS and Linux |
| Worker peak RSS | recorded on all platforms; not a 400 MiB package gate |
| Windows cumulative 400 MiB gate | pass |
| Production `pdf2zh` PyMuPDF isolation | pass on Windows, macOS and Linux |
| Deterministic archive identity | pass on Windows, macOS and Linux |
| macOS/Linux isolated native preflight | pass |
| Managed install, repair, cancellation and offline restart | pending lifecycle verification |
| Ordinary PDF visual-translation regression with overlay present/absent | pending lifecycle verification |

Decision: **Go for Checkpoint 1.** Native archive, isolation, quality, speed
and the Windows 400 MiB cumulative download-size checks pass. Peak RSS is
recorded but is not the package-size hard gate. Managed install/repair,
cancellation, offline restart and ordinary-PDF regression remain release
verification for the later worker/managed-component checkpoint.

Checkpoint 1 may proceed with legacy PDF/Markdown/TXT migration fixtures before
any UI work. Track ONNX session tuning and worker recycling as a bounded memory
optimization follow-up, without treating 400 MiB RSS as the release-size gate.

### Checkpoint 1: Model and migration

Primary files:

- `rosetta-app/src-tauri/src/rosetta_jobs/model.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/path.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/translation_files.rs`
- `rosetta-app/src/types/rosetta.ts`
- `rosetta-app/src/store/useRosettaStore.ts`
- `docs/engineering/conventions/data-models.md`

Implement `outputFormat`, triple uniqueness, legacy inference, stable IDs and
format-qualified frontend selection. Add fixtures for old PDF, Markdown and
TXT jobs. Confirm a load/save cycle does not rename legacy translation files
or change the PDF default mode.

#### Checkpoint 1 execution record (2026-08-06)

- Added required `outputFormat` to Rust and TypeScript translation-file models.
- Existing records without the field infer the native source format and are
  atomically rewritten without changing their translation ID.
- Native PDF/Markdown/TXT outputs retain `tr-{source}-{target}`; a PDF
  Markdown sibling uses `tr-{source}-{target}-markdown`.
- The narrow ensure command validates the source/output matrix and resolves
  the full `(sourceFileId, targetLang, outputFormat)` identity.
- Legacy source/job counters select only the native output, so Markdown
  segment progress cannot overwrite PDF page progress.
- The Zustand store persists output choice per job/source and keys the selected
  translation ID by job/source/output, with a legacy pair-key read fallback.
- Existing frontend call sites now pass their current native output format;
  no output selector or mode-switch UI was added.

### Checkpoint 2: Managed component and isolated worker

Primary files:

- new `rosetta-app/src-tauri/src/managed_pdf_markdown/`
- `rosetta-app/src-tauri/src/lib.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/layout.rs` only where a read-only
  base-runtime locator is needed
- new worker and pack scripts under `rosetta-app/src-tauri/scripts/`

Implement profiles, install/repair/status, worker supervision, process-group
cancellation, version preflight and bounded protocol decoding. Prove by test
that the two workers resolve their respective PyMuPDF versions concurrently.

### Checkpoint 3: Extraction store and normalizer

Primary files:

- new `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf_markdown/`
- `rosetta-app/src-tauri/src/rosetta_jobs/document.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/store.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/mod.rs`

Implement the manifest, page shards, image canonicalization, resume, stale
detection, normalization and atomic IR projection. Add narrow Tauri commands
for status, start and cancel; emit content-free progress events.

### Checkpoint 4: Translation and Markdown rendering

Primary files:

- `rosetta-app/src-tauri/src/rosetta_jobs/translation_files.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/export.rs`
- `rosetta-app/src/lib/translationSegments.ts`
- focused renderer tests in `rosetta_jobs`

Reuse the ordinary segment translation runner for Markdown output. Add a
single deterministic renderer shared by workbench preview and export semantics
for headings, lists, captions, footnotes, images and simple/complex tables.
Ensure formulas and image references are skipped by translation scheduling.

### Checkpoint 5: Workbench integration

Primary files:

- `rosetta-app/src/features/workspace/WorkspacePage.tsx`
- `rosetta-app/src/features/workspace/WorkspaceTopbar.tsx`
- `rosetta-app/src/features/preview/DocumentPreview.tsx`
- `rosetta-app/src/lib/rosettaExport.ts`
- `rosetta-app/src/lib/rosettaJobs.ts`
- `rosetta-app/src/store/useRosettaStore.ts`

Add the output selector and explicit install/extract/failed/ready states.
Branch on `(source format, output format)`, not only `sourceFile.format ===
"pdf"`. Keep `PdfDocumentPreview` for PDF mode and route Markdown mode through
the existing virtualized block preview. Verify mode switching during every
active state and after app restart.

### Checkpoint 6: Multi-file export and hardening

Implement `.md` plus asset export, destination rollback, cleanup, deletion and
repair behavior. Run the full corpus and packaging matrix. Update one aggregate
change log for the feature and record final benchmark numbers before enabling
the mode by default.

## 10. Validation Matrix

### Rust and persistence

- Legacy translation-file inference for PDF/Markdown/TXT.
- Triple uniqueness and format validation.
- Stable IDs across resume and same-policy re-extraction.
- Source fingerprint/policy/version mismatch invalidates only derivatives.
- Interrupted window never appears committed.
- Corrupt/truncated gzip shard is re-extracted.
- Image path traversal and oversized JSON are rejected.
- Markdown progress never changes PDF page authority or legacy PDF counters.
- Job/file deletion removes extraction shards and images after workers stop.
- Export rollback preserves the last successful `.md` and assets.

### Frontend

- PDF is the default for old/new PDF jobs.
- Output choice persists per source file.
- Each target language resolves the exact output-qualified translation file.
- Page controls appear only in PDF mode.
- Extraction controls/status appear only in Markdown mode.
- Mode switch does not clear the other mode's selection, progress or preview.
- Long previews remain virtualized; do not render all blocks.
- Translation/retranslation/export buttons use active-output readiness.

### Regression

Run when relevant:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Also run focused worker protocol, managed-component installer and pack preflight
tests on Windows x64, macOS arm64 and Linux x64. Do not mark the feature ready
from automated tests alone; inspect rendered Markdown and exported assets for
the acceptance corpus.

## 11. Stop Conditions

Pause rollout and return to a measured design decision if any condition holds:

- The release artifact exceeds the 400 MiB cumulative Windows PDF-component
  budget.
- The Markdown overlay changes the PyMuPDF version or behavior seen by the
  production `pdf2zh` worker.
- Complex figures again inject internal labels into body reading order.
- Resume can mix page shards from different source bytes or policy versions.
- Markdown syntax must be generated or repaired by the translation model to
  pass the corpus.
- More than a small, deterministic normalization layer is required to correct
  general reading order; that would recreate the abandoned self-built layout
  engine.

## 12. Completion Definition

The feature is complete only when:

- One PDF exposes working `PDF` and `Markdown` output modes without changing
  existing PDF behavior.
- Markdown extraction, translation, restart/resume and export pass the corpus.
- Old jobs load without migration loss and default to PDF output.
- Packaging, memory and performance gates pass on release artifacts.
- Data-model conventions, one aggregate change log and final benchmark results
  reflect the shipped implementation.
