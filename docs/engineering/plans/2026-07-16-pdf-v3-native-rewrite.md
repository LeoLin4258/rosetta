# PDF v3 Native Rewrite Plan

Date: 2026-07-16

Status: In progress

Branch: `codex/pdf-v3-rewrite`

Current progress:

- Phase 0 core contracts started: `PageSet`, `PageGraph`, `TranslationPatch`, and `PageResult` are represented as versioned Rust types.
- Phase 1 PDFium extraction spike started on Windows/AMD with exact PageSet random access and character-level geometry/style extraction.
- Page-object-order provenance is stable across repeated PDFium extraction for the same source fingerprint.
- PDFium save-only round trips are pixel/text exact on the tested pages, but same-text `set_text()` replacement changes pixels and corrupts extracted text on a real paper page. PDFium remains an extraction candidate but is not accepted as the v3 replacement renderer.
- PyMuPDF/MuPDF save-only round trips are also pixel/text exact, but its high-level redact-and-reinsert path changes 12.04% of pixels and changes extracted text from 3,851 to 3,308 characters on the real paper page. One of five page font resources cannot be replayed through the high-level API at all.
- PDFium is retained as the selected extraction/preview candidate. Neither PDFium `set_text()` nor PyMuPDF high-level delete/reinsert is accepted as the v3 replacement renderer.
- The Rust content-stream identity spike now provides stable `stream object + operator index + operand path` provenance. Rewriting all 779 encoded text operands on the real paper page preserves 3,909 / 3,909 extracted characters with zero changed pixels in both PDFium and independent Poppler rendering.
- The selected renderer direction is a Rosetta-owned low-level content-stream patch layer, with PDFium retained for extraction, preview, and validation. Identity, translated-text encoding/fitting and multi-target copy-on-write are now proven on the current conservative target set; paragraph reflow and bounded-memory export remain pending.
- Phase 2 atom-to-operand mapping has started. Stable cross-stream text-show IDs pair every top-level PDFium text object with every `Tj`/`TJ` show by verified ordinal order on the current fixture pages, including 242 / 242 pairs on the real paper and 1,045 / 1,045 on the Google Docs fixture.
- Rosetta now has a strict source-only ToUnicode decoder for one- and two-byte fixture CMaps, `bfchar`, `bfrange`, ligatures, and UTF-16 surrogate pairs. Source decoding is explicitly separate from translation encoding; no tested source font is approved for translated-text reuse.
- The real paper mapping classifies 120 / 242 pairs as exact, 107 as whitespace-equivalent because PDFium synthesizes spacing from positioning, and 15 as Unicode mismatches where PDFium exposes U+0002 but ToUnicode maps the source code to U+002D. PageGraph must therefore combine PDFium geometry with validated ToUnicode text correction rather than treating either view as universally authoritative.
- PageGraph schema v2 now reconciles PDFium atoms with decoded content operands. Every mapped atom can carry stable `mapping + text-show + operand + encoded byte range + source-unit character index` provenance without copying encoded source bytes into the IR.
- Reconciliation updates a text object atomically. It distinguishes PDFium-verified atoms, ToUnicode-corrected atoms, PDFium synthetic whitespace, source whitespace with no PDFium geometry, and preserved unmapped atoms. A failed object does not leave partially rewritten PageGraph state.
- On the real paper page, reconciliation maps 242 / 242 top-level text objects and classifies 3,238 atoms as verified, 15 as ToUnicode-corrected, 602 as synthetic whitespace, 2 source whitespace characters as having no PDFium atom, and 56 Form XObject atoms as preserved. The page remains correctly `partial` until Form XObject recursion exists.
- A reusable read-only `DocumentHandle` now owns one source fingerprint, one `lopdf::Document`, and one PDFium document built from the same immutable source byte snapshot. It verifies cross-engine page-count identity and lets sparse extraction, mapping, and reconciliation reuse the same document lifetime instead of reopening the source for each stage or page.
- On the 1.59 MB / 30-page real paper fixture, a Windows debug probe measured about 101 ms to open the handle, then about 658 ms and 691 ms to reconcile pages 1 and 3 through that same handle. Per-page debug timings varied between runs. Repeated document initialization is removed, but page-local PDFium traversal and source mapping remain the dominant unoptimized costs.
- Extraction and mapping now share one short-lived, non-serializable `PdfiumPageSnapshot`. The combined reconciliation path no longer repeats PDFium page text, text-object character mapping, font-name lookup or Form-object counting for the same page.
- After the shared snapshot change, three Windows debug runs measured page 1 at 429-477 ms and page 3 at 452-555 ms through an already-open handle. PageGraph atoms, provenance and conservative fallback results remained unchanged.
- PDFium and source mapping now recursively traverse Form XObjects in invocation order. Form resource dictionaries take priority with parent-context fallback, text-show IDs include the invocation path, and validated shared Form operands retain structured provenance for invocation-local copy-on-write.
- On the real paper page, recursive traversal aligns 258 / 258 text objects/shows across 27 Form invocations and 5 unique Form streams. The original 242 top-level objects remain mapped; 16 Type3 Form objects are explicitly preserved because no safe source decoder exists.
- The identity renderer now performs a read-only recursive Form discovery pass and rewrites every unique page/Form content stream exactly once. Shared Form invocation paths are reported separately from underlying stream ownership.
- On the real paper page, recursive identity covers 7 unique streams, 27 Form invocations, 1,360 operations, 258 text shows and 800 / 800 text operands. PDFium reports exact text and zero changed pixels; independent Poppler renders byte-identical page PNGs.
- A provenance-addressed operand-range executor now validates page/stream/operation/operand identity, complete operand length and SHA-256, byte bounds and non-overlap. It stages every affected stream and commits only after the complete patch set succeeds.
- Shared page streams and shared Form invocations are detected before mutation. Cross-page Form ownership uses a conservative `/Resources/XObject` reachability index, avoiding decompression of unselected page content streams; one logical target can now enter the copy-on-write path.
- On the 30-page real paper fixture, a one-byte identity patch against unique Form stream `24 0` fell from about 1,418 ms with all-page content parsing to 28-30 ms with resource reachability. PDFium text and pixels remain exact.
- PageGraph schema v3 now carries every Form invocation as structured parent stream, `Do` operation and child Form provenance instead of renderer-parsed strings.
- The patch executor can copy-on-write one logical shared target. It clones the leaf and every invocation ancestor, materializes effective resources, rewires only the selected page, and commits the new object chain atomically.
- Real-paper cross-page Form COW preserves PDFium and Poppler identity on selected page 1 and unselected page 2. The 30-page output grows by 7,761 bytes (about 0.52%), not by a full page or document copy.
- Valid uncompressed content streams now use the same `get_plain_content()` path as filtered streams.
- Translated text now targets one Rosetta-controlled font family instead of attempting to reuse each source PDF font. Simplified Chinese selects Source Han Sans CN Regular and loads Bold only for validated bold spans; production never depends on operating-system fonts.
- The native font layer caches one immutable asset, validates embedding/subsetting rights and glyph coverage, builds one deterministic document-wide subset, and stages one reusable Type0/CIDFont with explicit CID-to-GID and ToUnicode maps.
- A 29-character Chinese/Latin probe reduced the 10,397,552-byte Source Han font to 7,064 bytes and produced a 19,290-byte searchable PDF. A 1000-CJK-character subset was 255,624 bytes and took about 14 ms in the Windows debug probe.
- PageGraph schema v4 now carries source stream/operation, unqualified font resource, `Tf` size and `Tz` scaling required for renderer state validation.
- The first real replacement path atomically switches one unique top-level text-show to the unified font, writes translated CIDs, restores source font state and rejects stale provenance, later shows or unreadable overflow.
- A Source Han probe replaced one English source line with `统一字体安全回填` in about 3 ms. PDFium extracts the new Chinese text and Poppler shows the original baseline/size with all following lines undisturbed; the output is 16,483 bytes.
- Single-show fitting no longer accepts a caller-provided width. A typed layout
  gate resolves one reconciled PageGraph source object, includes synthetic
  whitespace, projects its bounds along the character-matrix baseline and
  converts page advance back to text space. The LibreOffice fixture derives
  453.68 units and keeps the Source Han replacement at fit scale 1.0.
- Horizontal/vertical scale, 90/180/270-degree direction, reverse baselines,
  arbitrary-angle preservation and stale-PageGraph zero-mutation now have
  automated coverage. Arbitrary angles remain preserved because PDFium exposes
  axis-aligned character boxes rather than exact glyph quads.
- Single-show replacement now resolves one PageGraph style, replays validated
  device-color/render state from the content stream and requires a matching
  Regular or Bold prepared translation face. Italic, mixed, stroked, clipping
  and unsupported paint states preserve the source.
- A Source Han Bold probe completed at fit scale 1.0 in about 4 ms and produced
  a 16,101-byte searchable output. PDFium verified text, font face and color;
  Poppler verified baseline and later-line stability.
- Real-paper and Google Docs Bold areas commonly use `Td`-anchored shows in one
  `BT`/`ET`. The renderer now recognizes validated `Tm/Td/TD/T*` and quote
  anchors, plans multiple same-face shows against the unchanged source stream
  and commits them as one transaction. Unanchored consecutive shows preserve
  the source.
- A two-show Source Han Bold real-paper probe completed at fit scale 1.0 in
  about 15 ms. The 1,508,982-byte output was 81,260 bytes smaller than source;
  Poppler changes were confined to both targets and unselected page 2 was
  pixel-exact.
- Anchored transactions now select a prepared face per show from validated
  PageGraph style and can stage Regular and Bold together. Object IDs are
  reserved without collision, page resources are materialized once and all
  font/stream/page mutations remain one commit.
- A mixed-face Source Han real-paper probe completed both replacements at fit
  scale 1.0 in about 13 ms. The 1,511,382-byte output was 78,860 bytes smaller
  than source; Poppler changes were confined to the two targets and page 2 was
  pixel-exact.
- Shared-stream mapping now applies decode, font and atom-coverage gates before
  classifying `SharedContentStream`; sharing is a renderer capability marker,
  not an unconditional reconciliation fallback.
- Translated replacement now commits through three explicit ownership paths:
  unique top-level streams update in place, cross-page shared top-level streams
  clone and rewire only the selected page, and all Form targets use
  invocation-local copy-on-write with effective leaf resources.
- A two-invocation Form probe translated only the selected invocation, cloned
  two streams in about 4 ms and produced a 16,564-byte searchable PDF. A
  cross-page shared-stream probe rewired only page 1; page 2 remained
  pixel-exact and retained the source text.
- The low-level patch executor now merges multiple invocation targets into one
  deterministic clone tree. Nodes are keyed by root page stream and structured
  path prefix, so every common Form ancestor is cloned once and multiple page
  `/Contents` roots can be rewired in one atomic page commit.
- A nested identity probe targeting two invocations cloned root + parent + two
  leaves (4 streams, instead of two independent 3-stream chains). The output
  grew by 2,610 bytes and its Poppler PNG SHA-256 remained pixel-exact.
- Translated replacement now plans multiple stream/invocation targets against
  one unchanged source page, unions required font faces and commits all fonts,
  clone roots and page rewiring atomically. A one-target compatibility wrapper
  retains the existing transaction contract.
- A Source Han two-invocation translated probe reused one six-object font
  subset, cloned one root plus two leaves in about 4 ms and produced a
  searchable 17,044-byte PDF from a 13,129-byte source. A mixed Form/top-level
  test rewired both `/Contents` roots in one commit and preserved every source
  stream.
- Logical targets in distinct `BT`/`ET` objects can now share one physical
  stream/path. They validate independently against the unchanged source, then
  merge into one descending splice, encode and ownership commit. Top-level and
  Form COW tests prove one physical rewrite/leaf regardless of logical target
  count.
- A Source Han same-stream probe replaced two text objects in about 4 ms and
  produced a searchable 16,488-byte PDF from a 13,473-byte source. Poppler
  changes were confined to the two original text rows.
- Phase 4 patch-first persistence has started with a durable
  `TranslationPatch` schema v1. Canonical page/atom identity, translation
  revision and provider/model metadata, exact protected-span byte placement,
  typed renderer decisions, deterministic IDs and compact JSON validation are
  implemented with a 16 MiB page-patch limit.
- Patch fixtures round-trip without copying ordinary source text and reject
  stale pages/atoms, duplicate atom ownership, partial or reordered protected
  spans, invalid fit state and modified patch identity. The following store
  slice builds atomic revisioned ownership on that contract.
- Phase 4 now has an atomic sharded patch store. Immutable revision-addressed
  page patches are indexed by deterministic 64-page shards; the shard width is
  internal and does not constrain PageSet, scheduling or user-visible ranges.
  Windows-compatible temp/backup replacement, stale revision rejection,
  parallel commit serialization, interrupted-write recovery and orphan cleanup
  are implemented.
- Two 1,000-page Windows AMD debug probes completed independently synced page
  commits in 15.54-16.40 seconds. The final run used 16 shards, 323,244 logical
  index bytes and 615,572 patch payload bytes. A rejected whole-manifest design
  took 51.54 seconds, so it was removed before becoming a persistent contract.
- Phase 4 now also has an isolated bounded render cache. Cache keys bind source,
  page, patch/revision, renderer and output options; content-addressed PNG/PDF
  artifacts use a 384 MiB / 4,096-entry default policy, deterministic LRU,
  active leases, atomic writes and page-local repair across 64 hash shards.
- A 1,000-page Windows AMD test retained only the configured 128 entries,
  stayed below its 128 KiB artifact quota and kept logical index bytes below
  1 MiB. The cache remains isolated from legacy PDF state.
- PageGraph schema v5 now carries the exact text-show operator and operand
  SHA-256 needed to construct replacement requests without searching source
  text. A new `TranslationPatch` renderer validates complete source-object
  coverage, groups entries by stream/Form path/`BT`/`ET`, preflights every
  target against the unchanged document, resolves all entry decisions and
  applies safe targets through one existing atomic page batch.
- Unsupported or incomplete entries now receive stable preservation reasons
  while safe sibling entries still render. Stale operator/operand identity is
  fatal and leaves all PDF objects and `max_id` unchanged. The patch store now
  accepts only fully resolved patches; pending patches are ephemeral renderer
  drafts and never become disk authority.
- A Windows AMD manual probe replaced one LibreOffice row with `Unified patch
  renderer`. Independent Poppler rendering confined all 6,846 changed pixels
  to the original first-row band (0.3145% of a 1241x1754 page), and independent
  `pypdf` extraction found the replacement text in the output.
- The patch renderer is now connected to the bounded render cache through a
  resolved-patch-only bridge. Fully resolved patches can be deterministically
  re-preflighted and rerendered after a cache miss; a stored decision drift or
  renderer contract-version mismatch fails before document mutation.
- `translatedPagePdf` generation consumes one working document, removes every
  unselected page plus document navigation, prunes unreachable objects,
  renumbers/compresses and validates an exactly-one-page artifact before cache
  insertion. Cache insert remains separate from resolved patch ownership.
- On the 30-page / 1,590,242-byte real-paper fixture, the cached page artifact
  was 104,857 bytes. Independent Poppler changed 2,718 pixels (0.1249%) only in
  the target footer row; page geometry, 26 annotations and the external link
  remained intact, and `pypdf` extracted the replacement text.
- Resolved single-page artifacts now feed an isolated PDFium preview rasterizer.
  Exact 200..=1,800 pixel widths produce deterministic PNGs whose cache keys
  bind both the patch renderer and preview-rasterizer contracts. The artifact
  owns its cache identity, width variants cannot collide, and corrupt cached
  PNG bodies become rebuildable misses.
- The Windows AMD real-paper probe produced a complete 1,200x1,697 PDFium PNG
  in 1,054,528 bytes. Visual inspection showed the full page without clipping,
  blank regions or layout movement outside the translated footer. Independent
  Poppler rendered the same single-page PDF at 1,200x1,698, the expected
  one-pixel height rounding difference between raster engines.
- A document-wide translation-font registry now stages each prepared face once
  and lets consecutive page-patch renders reuse the same Type0 object. Registry
  binding validates weight, asset/fingerprint/subset identity and the live PDF
  object before any page mutation; duplicate faces fail atomically.
- A 30-page Windows AMD probe translated pages 1 and 2 with one 27,568-byte
  Arial subset. Both page renders staged zero font objects, the document held
  exactly one matching Type0 font, and the complete output was 1,521,952 bytes
  versus the 1,590,242-byte source. Poppler changes stayed within the two target
  footer rows and page 3 remained pixel-exact; page count, annotations and
  metadata were retained.
- Final document export now appends an explicit `PdfObjectDelta` to the
  immutable source instead of serializing a complete replacement document or
  comparing complete source/rendered object graphs.
- A bounded `PdfSourceObjectStore` now memory-maps the source, resolves xref
  tables/streams and object streams on demand, and exposes a 16 MiB / 512-entry
  object LRU plus an immutable delta overlay.
- Document-wide font allocation and registry identity validation are the first
  renderer operations migrated from `&lopdf::Document` to `PdfObjectView`.
  The real two-page export stages its six font objects directly against the
  lazy source and validates them through the overlay without loading a source
  object.
- TranslationPatch page staging now separates its immutable source traversal
  document from the accumulated object view. Font, page and copy-on-write
  deltas stay only in `PdfObjectOverlay`; each later page allocates above the
  complete accumulated maximum without applying earlier deltas to the source
  document. The real multi-page proof also removed its complete working-document
  clone, leaving one read-only source object graph plus the bounded delta.
- Selected-page identity now comes from a reusable `PdfPageIndex` over the lazy
  source view. It records only the explicit `PageSet`, skips unrelated page-tree
  subtrees by `/Count`, and supplies page/content-root IDs to replacement
  preflight and staging without `Document::get_pages()`.
- Selected-page dictionaries and inherited resources now come from an owned
  `PdfPageObjectContext`, while target identity, preflight decode and staged
  content-stream reads use the immutable lazy source view. The real two-page
  proof loads 12 source objects and retains 28,712 estimated bytes.
- Form invocation validation and COW clone-tree staging now use owned effective
  resource contexts over the lazy source view. A nested repeated-Form proof
  stages four clones with 8 source loads and about 11 KiB resident.
- The current incremental two-page proof remains 1,617,258 bytes from a
  1,590,242-byte source: 27,016 appended bytes and 10 delta objects. Page 1 and
  2 changes remain confined to their translated footer rows and page 3 is
  pixel-exact. Cross-page stream/Form ownership now uses one reusable bounded
  lazy index; production page staging no longer accepts a complete document.

## Purpose

Rosetta beta 的现有 PDF 链路已经通过持久 worker、页面窗口、缓存和压缩补丁积累了足够多的经验，但核心边界仍然不适合作为长期架构。PDF v3 彻底放弃旧 PDF v1/v2 的实现、artifact 和协议兼容，重新建立一个由 Rosetta 控制的 native、page-addressable、patch-first PDF 平台。

本计划不把当前 `pdf2zh` 链路逐步改造成 v3。当前 beta 的 PDF 派生状态可以丢弃，源 PDF 保留并重新生成全部派生数据。

## Product Decisions

已确认的产品决策：

- 首要目标是视觉保真，而不是可编辑文本或重新排版。
- 无法安全回填的复杂区域保留原文，不生成猜测性译文。
- 普通文字 PDF 走 native fast path；复杂页面使用明确的 fallback，不让 fallback 反向污染核心模型。
- 允许采用 MuPDF 等更强的 PDF 引擎，并承担必要的授权成本。
- 翻译结果以 page patch 为权威数据，完整译文 PDF 只在预览缓存或导出时生成。
- PDF 输出保留可保留的页面对象、链接、书签、注释和元数据；数字签名在译文导出后明确视为失效的新文件。
- 基础 native 组件和高级复杂页面组件分离安装。
- 长 PDF 使用流式、页级、可恢复调度，用户不感知固定 10 页分片。
- v3 不迁移旧版 PDF 派生 artifact，beta 用户从源 PDF 重新生成。

## Goals

- 提供显式的 `PageSet` API，支持任意页提取、检查、翻译、渲染和导出。
- 对标准 Unicode text run 实现快速、准确、可解释的 extraction。
- 基于 glyph/run 和原始 PDF object provenance 实现局部回填。
- 保留图片、矢量图形、表格线、背景、非翻译文字和页面几何结构。
- 通过 citation、URL、编号、公式和样式 span 的稳定 ID 避免字符串猜测回填。
- 让数百页 PDF 在有界内存和有界磁盘缓存内完成，支持崩溃恢复和局部重试。
- 让组件安装、能力、健康、版本和运行操作完全由 Rosetta 管理。
- 让 extraction、translation、render 和 export 具备独立版本、诊断和测试边界。

## Non-goals

- v3 不承诺扫描 PDF OCR。
- v3 不强行翻译公式、转曲文字、不可解释的 Type3 文本或无法安全定位的视觉区域。
- v3 不把 PDF 转换成通用 Markdown/DOCX 中间格式。
- v3 不通过增加启发式字符串规则来覆盖未知 PDF。
- v3 不保留旧 v1/v2 page artifact、旧 worker 协议或旧 page state 的迁移兼容层。

## Target Architecture

```text
Native PDF Core
  ├─ DocumentHandle / PageIndex
  ├─ PageSet random access
  ├─ glyph/run/object extraction
  ├─ PageGraph construction
  ├─ object-preserving patch renderer
  └─ page/export validation

Rosetta PDF Orchestrator
  ├─ page task scheduler
  ├─ translation unit planner
  ├─ patch store
  ├─ bounded render cache
  ├─ long-job recovery
  └─ component lifecycle manager

Local Translation Provider
  └─ receives protected, versioned translation spans

React Workbench
  └─ consumes typed snapshots and page events, never owns PDF business state
```

### Native PDF Core

The core must expose a narrow, versioned interface independent of any specific PDF library:

- `openDocument(source)`;
- `inspectPageSet(document, pageSet)`;
- `extractPageSet(document, pageSet, options)`;
- `renderPageSet(document, pageSet, patches, options)`;
- `exportDocument(document, patches, options)`;
- `releaseDocument(documentHandle)`.

PDFium remains a candidate because it is already packaged, but its glyph, font, color and content-stream capabilities must be verified. MuPDF is an allowed alternative when object-level extraction or patch rendering requires it. The selected engine must be isolated behind this interface.

### PageSet

`PageSet` is a typed, canonical set of 1-based page numbers. It must support single pages, ranges, deduplication, sorting, validation against the source page count and a stable hash. UI text input is only one serialization of this type.

### PageGraph

PageGraph is the canonical extraction model. It contains:

- page boxes, rotation, crop and media dimensions;
- ordered text atoms and glyph/run provenance;
- Unicode text and source hashes;
- bbox/quad, baseline, transform and writing mode;
- font resource, size, color, alpha and style references;
- z-order, clipping, layer and object identity;
- line, paragraph, column, table/cell and caption groups;
- protected spans for citations, URLs, numbers, formulas and symbols;
- confidence and explicit fallback reasons.

PageGraph IDs must be deterministic for the same source fingerprint, engine version and extraction schema. The model must not expose library-specific classes to the frontend or persistent job APIs.

### TranslationPatch

TranslationPatch is the durable translation result for a page. It references PageGraph atom/span IDs and stores:

- source page hash;
- source atom hashes;
- translated spans;
- protected span values;
- style references;
- local fit/reflow decisions;
- renderer and patch schema versions;
- translation revision and provider/model identity.

A patch is not a PDF and must be much smaller than a complete translated page artifact.

### Renderer

The renderer preserves all original objects unless a patch explicitly targets them. It must:

- erase only the exact translated glyph/run regions;
- reuse original geometry and style where valid;
- share document-level fonts and resources;
- use one controlled translation font family and one deterministic subset per used face;
- preserve graphics, fills, lines, images, links and annotations where possible;
- reject missing or reordered protected spans;
- report overflow, overlap and unsupported object reasons;
- return `translated`, `preserved` or `failed` page results.

Unsupported or low-confidence regions remain original. A renderer failure must not produce a blank successful page.

## Persistent Data Model

The v3 job layout is intentionally independent from existing PDF state:

```text
<jobId>/
  source.pdf
  source-manifest.json
  pdf-v3/
    extraction/
      page-0001.ir.zst
    translations/
      zh-CN/
        manifest.json
        page-0001.patch
    runs/
      <runId>.json
    render-cache/
      bounded LRU entries
  exports/
    translated.pdf
```

Extraction IR is disposable derived data. Translation patches and their source hashes are the durable translation authority. Render cache is disposable and quota-bound. Complete per-page PDFs are not the canonical state.

All writes use temporary files plus atomic replace. Content-addressed or revisioned patch names prevent stale renderer output from overwriting a newer translation.

## Long PDF Scheduler

- Keep one read-only document handle and a bounded page queue.
- Process pages independently; cross-page translation context is an input to a task, never the ownership boundary of a task.
- Batch translation units across ready pages without retaining completed page render state.
- Commit each page patch transactionally before releasing its memory.
- Persist queue cursor, page states, run lease, engine version and cancellation state.
- Resume only missing or invalid pages after restart.
- Use backpressure between extraction, translation and rendering.
- Keep preview rasterization on-demand and bounded by byte quota.
- Stream final export directly from source plus patches.

No user-facing behavior or persisted state may depend on a fixed 10-page chunk size.

## Component Manager

The component manifest must include:

- component id, version, contract schema and build hash;
- platform and architecture;
- package files, sizes and hashes;
- engine capabilities;
- font/model asset capabilities;
- license metadata;
- minimum supported app version.

Lifecycle states are separate for installation, health and active operation:

```text
absent → downloading → verifying → unpacking → self-test → ready
ready ↔ busy
ready → degraded → repairing/failed
```

The manager owns install, verify, repair, update, remove, start, stop, self-test and diagnostics. A capability negotiation failure is explicit and typed. The frontend never infers readiness from a cache marker or worker event alone.

## Visual Fidelity Policy

- Standard text runs: translate and reinsert with object-level provenance.
- Citations, URLs, page numbers, formula tokens and protected symbols: preserve exactly.
- Tables: extract and translate cells only when cell geometry is explainable.
- Formulas, algorithms, dense visual boxes and uncertain regions: preserve original.
- Mixed color/weight: restore span styles only when the translated span mapping validates; otherwise use the safe source-preserving fallback.
- Typeface family: translated text may use the Rosetta-selected unified family; matching the source typeface is not required. Preserve validated weight intent with the family bold face only when needed.
- Text expansion: apply a deterministic fit policy; if it cannot fit without damaging the page, preserve the source region and report the reason.
- Page signatures: source signature remains valid only on source; any translated export is a new unsigned document.

## Disk and Performance Budgets

Initial targets, to be confirmed by the native engine spike:

- ordinary 10-page text PDF cold extraction: p50 below 2 seconds, p95 below 4 seconds;
- selected-page extraction must not run layout work on unselected pages;
- first visible page should be renderable independently of the rest of the document;
- long runs must keep active extraction/render memory bounded;
- render cache must have a configurable hard byte limit, with a conservative default in the 256–512 MB range;
- the selected render-cache default is 384 MiB plus bounded index metadata, with a 4,096-entry default limit;
- translation storage must be patch-based and scale with text, not with one full PDF per page;
- final PDF should embed shared font/resource sets rather than page-local copies.

## Testing Strategy

Before implementation, create a fixture corpus covering:

- single and multi-column papers;
- mixed colors, weights, superscripts and citations;
- tables, formulas, algorithms and captions;
- duplicate text layers;
- rotated pages and mixed page sizes;
- CJK, RTL, vertical writing, ligatures and combining marks;
- links, bookmarks, annotations, forms, metadata and encrypted files;
- Type3, damaged and outline-like text;
- 100, 500 and 1000+ page stress documents.

Required test layers:

- deterministic PageGraph extraction tests;
- identity render round trips;
- protected span and citation invariants;
- visual regression tests;
- text re-extraction from exported PDFs;
- long-run memory, disk and backpressure tests;
- crash, cancel, resume and partial export tests;
- malformed-PDF and fuzz tests;
- component install/repair/version negotiation tests;
- provider/model and renderer version reproducibility tests.

## Implementation Phases

### Phase 0 — Design lock

- Freeze the PageSet, PageGraph, TranslationPatch, PageResult and component manifest contracts.
- Record engine licensing and distribution requirements.
- Freeze the fixture corpus and performance/disk measurement method.

### Phase 1 — Native engine spike

- Compare PDFium and MuPDF on extraction, font/style provenance, random page access and object-preserving patch rendering.
- Measure cold/warm extraction, memory, output size and license obligations.
- Select the engine only after identity render results, not by API familiarity.

### Phase 2 — PageGraph and inspect mode

- Implement source fingerprinting, DocumentHandle and PageSet.
- Produce PageGraph for selected pages.
- Add a local debug representation showing source object, atom, style and fallback decisions.
- Map PDFium page-text atoms to encoded content operands with independent count, font, Unicode, atom-coverage, synthetic-whitespace, Form XObject and decoder checks.
- Keep source decode capability, source re-encode capability and translated-font capability as separate states.
- Reconcile eligible objects atomically and persist byte-range provenance only after all object-level checks pass.
- Keep `complete`, `partial` and `preserved` page states explicit; partial extraction must never be presented as fully patchable.

Current Phase 2 boundary: source fingerprinting, exact `PageSet`, reusable
`DocumentHandle`, recursive text-show mapping and atomic PageGraph
reconciliation are implemented in the isolated Rust module. Extraction and
mapping share one PDFium page snapshot. Recursive Form XObjects, inherited
resources and structured invocation provenance are implemented. Shared-stream
status is retained only after the ordinary mapping gates pass, allowing the
renderer to choose invocation-local copy-on-write. Type3 decoding remains
pending.

### Phase 3 — Identity renderer

- Apply original text through the new renderer.
- Validate page geometry, colors, text extraction and visual output.
- Do not connect translation until identity render passes the fixture corpus.

Current Phase 3 boundary: top-level and recursive Form identity rewrites pass
the fixture corpus and the Windows real-paper page in PDFium and Poppler.
The atomic operand-range executor applies source-verified patches to unique
streams and can merge multiple shared page/Form invocation paths into one
copy-on-write clone tree. Deterministic unified-font subsetting, Type0/CIDFont
embedding and searchable CJK text insertion are proven, and multiple anchored
shows in one `BT`/`ET` can now be replaced atomically using PageGraph-derived
text-space fit bounds, validated device-color state and independently selected
Regular/Bold translation faces. Page-level translated batches now span multiple
Form invocation paths and top-level content roots, reuse one font subset per
face and merge all copy-on-write paths into one atomic page commit. Each target
remains one stream/path and one `BT`/`ET`, while distinct text-object targets in
the same stream/path share one physical staged stream. The durable patch
contract is now connected through a conservative page renderer for complete
single-object entries. Unanchored consecutive shows, one-show mixed styles,
paragraph layout and arbitrary-angle geometry remain preserved.

### Phase 4 — Patch-first persistence

- Persist compact page patches and page revisions.
- Add bounded render cache and garbage collection.
- Add streaming export with shared resources and fonts.

Current Phase 4 boundary: the canonical `TranslationPatch` schema v1, builder,
compact JSON encoding and PageGraph-aware validation are implemented. The
contract records deterministic patch/entry identity, source page/atom hashes,
translation revision and provider/model identity, exact protected-span UTF-8
byte ranges and typed pending/fitted/preserved renderer decisions. Ordinary
source text is not duplicated into patches, and all persistent encode/decode
paths enforce a 16 MiB page-patch limit. Atomic revisioned disk storage is now
implemented with a stable language manifest, bounded 64-page index shards,
page-local recovery and superseded/orphan cleanup. The isolated render cache is
also implemented with source/patch/renderer-addressed keys, a configurable
384 MiB default hard artifact quota, 4,096-entry default limit, 64 bounded hash
index shards, deterministic LRU, active leases, atomic writes, integrity checks
and local repair. Pending translation drafts now resolve entirely in memory;
only fitted/preserved patches can enter the store, avoiding same-revision
identity conflicts. Resolved patches now deterministically regenerate pruned
single-page PDF artifacts and use source/patch/revision/current-renderer cache
identity for bounded insertion and lease-validated reads. Those page artifacts
now rasterize on demand to exact-width PDFium PNGs with a separately versioned
preview contract, bounded insertion and lease-validated reads. Patch
compression remains pending. Document-wide font resource reuse is implemented
and proven across consecutive page renders. A source-identity-checked
incremental delta writer now copies the immutable source with a fixed 64 KiB
buffer, appends only changed objects and a new xref/trailer, supports
cancellation before commit, and atomically replaces the destination after file
sync. The writer no longer owns source bytes or the previous object graph. The
  production page renderer now reads page trees, resources, content streams and
  cross-page ownership through lazy views and a reusable target-bounded index.
  Scheduler recovery and job-level stress validation remain pending before the
  complete app workflow is end-to-end resumable. Font registry and page renderer mutation are explicitly staged
as merge-checked `PdfObjectDelta` values, and final multi-page export no longer
applies those deltas to its source traversal document. The incremental writer
consumes the accumulated delta directly; whole-object-graph comparison is no
longer part of the export path.

The lazy source-object foundation now opens the immutable source through a
read-only memory map, resolves classic/xref-stream and object-stream entries on
demand, converts only requested objects into the existing renderer object type,
and bounds its LRU by bytes and entries. Incremental export base construction,
document-wide font allocation, registry identity validation and page-level
object allocation already use the accumulated lazy overlay. A selected-page
`PdfPageIndex` now resolves page count, page object identity, page-tree ancestry
and direct content-stream references through `PdfObjectView`, skips unselected
subtrees by `/Count`, and is reused across the real multi-page staging proof.
`PdfPageObjectContext` resolves the exact page dictionary and materializes
inherited resources through that same immutable source view. Replacement
identity, preflight, decode and staged stream reads also use the lazy source
view, so selected-page staging no longer depends on `Document::get_pages()`,
selected-page resource helpers or complete-document source stream reads. The
real two-page proof loads 12 source objects and keeps 12 cache entries / 28,712
estimated bytes resident under explicit ceilings.

Form invocation validation and copy-on-write resource traversal use the same
immutable lazy source boundary. Form
validation and COW staging now build owned effective resource contexts from the
lazy source view, resolve each root/Form stream there, rewrite the selected page
from its page context, and allocate clones above the accumulated overlay
maximum. A nested repeated-Form proof produces the same four clones as the
`Document` adapter with 8 source loads, 11 cache hits, 8 resident entries and
11,272 estimated resident bytes.

Global cross-page stream/Form ownership now uses a transient three-state index
over explicit target stream IDs. It streams page dictionaries, avoids content
decompression, follows Form-local resource declarations conservatively, and is
reused across page stages. A 1,000-page synthetic scan retains two requested
target states; the real 30-page proof stays within a 12-entry object cache.
Production replacement and TranslationPatch staging no longer accept
`lopdf::Document`.

### Phase 5 — Translation and protected spans

- Add citation, URL, formula, number and style span protection.
- Add deterministic fit and safe preservation policies.
- Add page-level translation revisions and local retry.

### Phase 6 — Long-document scheduler

- Add bounded queues, backpressure, durable leases, cancellation and crash recovery.
- Validate 500/1000-page runs without user-visible chunk semantics.

Current Phase 6 boundary: an independent durable scheduler core now stores a
small versioned run manifest plus authoritative 64-page state shards. Typed
page states and extraction/translation leases support bounded claims, commit,
failure/retry, pause, cancellation and stale-owner recovery. Independent hard
limits cover extracting pages, extracted pages waiting for translation and
translating pages; status reads are capped at 256 records. Opening a run
recovers synced temp/backup candidates, verifies exact PageSet coverage and
rebuilds the manifest summary from shards. Recovery consumes validated
PageGraph and TranslationPatch inventories, so artifacts committed before a
crash are promoted and invalid completion state is not trusted. A 1,000-page
Windows AMD test uses 16 shards, keeps each shard at or below 64 records and
proves claim limits without ten-page scheduling semantics. Worker/Tauri/UI
integration and a real 500/1,000-page end-to-end translation remain pending.

### Phase 7 — Component control plane

- Add signed manifests, install/repair/update/remove, self-test, capabilities and typed health events.
- Move all process lifecycle ownership into the native Tauri manager.

### Phase 8 — Optional complex-page fallback

- Adapt the legacy or advanced engine behind the PageGraph/PageResult contract only where native coverage is insufficient.
- Keep fallback reasons explicit and measurable.
- Do not let fallback-specific models or state leak into the native core.

### Phase 9 — Beta validation and replacement

- Run the full fixture, stress, disk, crash and export matrix.
- Reset all beta PDF derived artifacts from source PDFs.
- Remove old PDF commands, state files, worker protocol and page-PDF authority.
- Publish the new component pack and update PDF engineering documentation.

## Definition of Done

PDF v3 is complete only when:

- the normal path no longer depends on the old Python/pdf2zh orchestration;
- arbitrary PageSet extraction works;
- identity render is stable across the fixture corpus;
- unsupported areas preserve source content instead of producing bad translations;
- long documents remain resumable and bounded in memory and disk;
- patch storage is the durable translation authority;
- export uses shared resources and passes page/text validation;
- component state is inspectable and controllable through typed APIs;
- engine, IR, patch and renderer versions are reproducible;
- old beta PDF derived data is deliberately discarded rather than migrated.
