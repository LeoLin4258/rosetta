# PDF v3 Windows AMD Extraction Benchmark

Date: 2026-07-18

## Scope

This diagnostic isolates PDF v3 selected-page extraction, content operand
mapping and PageGraph reconciliation. It does not include translation, render
cache population, final PDF export or UI work.

Environment:

- Windows on the current AMD development machine;
- Rust test profile (`unoptimized + debuginfo`);
- `pdfium-render` 0.9.1 with PDFium 7763 bindings;
- fixture `fixtures/pdf/2305.13048v2.pdf`;
- source size 1,590,242 bytes, 30 pages;
- selected pages 1 through 10, producing 39,783 atoms.

Command:

```powershell
cargo test pdf_v3::reconcile::tests::manual_windows_ten_page_reconciliation_benchmark --lib -- --ignored --exact --nocapture
```

Each recorded sample starts a new test process and opens a new shared document
handle. Windows file-system caching is uncontrolled, so these are repeatable
local diagnostic runs rather than strict cold-disk measurements.

## Results

| Implementation | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Repeated text-page open and per-object character scan | 4,692 ms | 4,791 ms | 4,729 ms |
| Shared text page, per-object character scan | 1,613 ms | 1,516 ms | 1,650 ms |
| Shared text page, single-pass object identity | 808 ms | 874 ms | 784 ms |

Final-stage ranges across the three single-pass runs:

| Stage | Range |
| --- | ---: |
| Document open | 84-94 ms |
| Extraction total | 242-257 ms |
| PDFium object snapshot | 82-85 ms |
| Object text retrieval | 81.2-82.7 ms |
| Single-pass object identity queries | 2.2-2.5 ms |
| Character geometry/style/identity loop | 101-113 ms |
| Content operand mapping | 432-498 ms |
| PageGraph reconciliation | 72-79 ms |

## Conclusion

Reusing `PdfPageText::for_object()` removed repeated text-page loads. Replacing
`chars_for_object()` with one exact identity query per character removed the
remaining object-count-by-character-count scan. The ten-page debug diagnostic
is now about 5.7 times faster than the stable pre-change runs.

Content operand mapping is the new largest stage. Future performance work
should profile that stage without weakening ToUnicode correction, Form
invocation provenance or atomic reconciliation.

The result is strong evidence for the current fixture only. The full PDF v3
acceptance matrix still requires release-profile corpus measurements, malformed
and duplicate-layer cases, and 100/500/1,000-page memory and throughput runs.

## Follow-Up: Content Mapping

The next pass retained parsed immutable content streams for one page mapping,
indexed lopdf page IDs once per document and removed per-byte formatting
allocations from SHA-256 ID encoding. It did not retain parsed content across
pages.

Three follow-up debug runs measured:

| Measurement | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Ten-page total | 767 ms | 717 ms | 797 ms |
| Content operand mapping | 400 ms | 373 ms | 415 ms |
| Aggregate page lookup | 29 us | 30 us | 30 us |
| Page-local stream cache hits | 219 | 219 | 219 |

Compared by three-run median, content mapping moved from 440 ms in the first
single-pass result to 400 ms in the follow-up, about 9% faster. Total median
moved from 808 ms to 767 ms. Run-to-run noise remains material at this scale.

## Follow-Up: Bounded Lazy Source Mapping

The extraction/mapping handle then replaced its complete lopdf object graph and
all-page object ID vector with the existing mmap-backed `PdfSourceObjectStore`.
Each mapping resolves a one-page index and inherited resource context on demand.

Three debug runs measured:

| Measurement | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Ten-page total | 807 ms | 761 ms | 742 ms |
| Content operand mapping | 434 ms | 416 ms | 404 ms |
| Aggregate page index/context lookup | 5,142 us | 4,598 us | 4,749 us |
| Source object loads | 167 | 167 | 167 |
| Source object cache hits | 998 | 998 | 998 |
| Final resident source objects | 167 | 167 | 167 |
| Final estimated resident bytes | 524,541 | 524,541 | 524,541 |
| Page-local parsed stream cache hits | 219 | 219 | 219 |

The total median is 761 ms versus 767 ms before the migration. Mapping median
is 416 ms versus 400 ms. The selected page-tree lookup cost is now visible but
small, and the complete source object graph is no longer retained. The measured
working set is well below the fixed 512-object / 16 MiB LRU limits; active
decompressed stream allocations remain outside this retained-cache figure.

## Follow-Up: Reusable PageSet Index and Long Documents

The one-page compatibility path above would repeatedly traverse page-tree
prefixes during a sequential long-document run. Multi-page work now resolves
one source-bound mapping index for the caller's explicit `PageSet` and reuses it
while page snapshots, parsed streams and PageGraphs remain page-local.

A valid generated fixture deliberately uses a flat `/Pages/Kids` array, one
independent compressed stream per page and one exact text show. Each probe ran
in a separate test process:

```powershell
cargo test --locked manual_windows_hundred_page_long_document_probe --lib -- --ignored --nocapture
cargo test --locked manual_windows_five_hundred_page_long_document_probe --lib -- --ignored --nocapture
cargo test --locked manual_windows_thousand_page_long_document_probe --lib -- --ignored --nocapture
```

| Pages | Source | Open | Index | Main loop | Extraction | Mapping | Reconciliation | Atoms |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 100 | 23,807 B | 7 ms | 3 ms | 51 ms | 18,119 us | 27,541 us | 5,041 us | 3,300 |
| 500 | 118,221 B | 10 ms | 16 ms | 286 ms | 81,560 us | 172,404 us | 26,194 us | 16,500 |
| 1,000 | 238,223 B | 16 ms | 34 ms | 681 ms | 181,747 us | 425,263 us | 60,096 us | 33,000 |

| Pages | Peak working set | Final private bytes | Source loads / hits | Final source cache |
| ---: | ---: | ---: | ---: | ---: |
| 100 | 20,086,784 B | 4,206,592 B | 204 / 499 | 204 entries / 171,334 B |
| 500 | 22,069,248 B | 5,812,224 B | 1,494 / 2,009 | 512 entries / 461,630 B |
| 1,000 | 23,728,128 B | 7,733,248 B | 3,005 / 3,998 | 512 entries / 521,630 B |

Every page reconciled as `Complete` with one exact mapped text show. The source
cache reached its 512-entry ceiling and remained far below its 16 MiB byte
ceiling.

A separate 500-page comparison resolved one all-page index in 16,914 us, then
resolved 500 individual one-page indexes in 756,893 us with a warm source
cache. Repeated construction was 44.75 times slower on this worst-case tree.

These results close the synthetic retained-memory and structural-complexity
gate for native extraction/mapping. They do not cover translation, persistence,
final export, UI work or complex real-world long-document corpus behavior.

## Follow-Up: Durable PageGraph Extraction Worker

The scheduler extraction worker now commits each reconciled PageGraph to its
compressed artifact store before it commits scheduler extraction authority.
The manual probe runs real-paper pages 1-10 through the complete durable path:

```powershell
cargo test --locked manual_windows_real_ten_page_extraction_pipeline_probe --lib -- --ignored --nocapture
```

| Stage | Time |
| --- | ---: |
| Complete worker batch | 3,432 ms |
| Scheduler claims | 161,345 us |
| Native reconciliation | 722,923 us |
| PageGraph store | 2,357,095 us |
| Streaming JSON + gzip | 2,116,120 us |
| Scheduler commits | 159,069 us |

| Disk measurement | Bytes |
| --- | ---: |
| Logical uncompressed PageGraphs | 37,855,795 |
| Compressed artifact payloads | 3,323,703 |
| Complete PageGraph store | 3,327,910 |
| Scheduler directory | 4,034 |

Fast deterministic gzip retains 8.78% of the raw JSON size. The measured native
reconciliation remains below one second; durable serialization/compression is
now the largest stage. Gzip level 6 was rejected after it reduced payloads to
2,010,386 bytes but increased the batch to 5,314 ms.

The current result is still a debug diagnostic with synchronous file durability.
A compact PageGraph disk schema should be evaluated only if release-profile
corpus measurements show that the 2.36-second store stage remains material.

## Follow-Up: Durable 500-Page Translation and Export

An ignored Windows acceptance test now exercises the complete deterministic
path without using AppData or the local model service. It repeats the
renderable page from `002-trivial-libre-office-writer.pdf` 500 times in a
temporary source PDF. The repeated pages deliberately share the template's
content streams, fonts and resources, so export must apply cross-page ownership
and copy-on-write rules rather than relying on independent synthetic streams.

The scheduler runs with the production-shaped `2 / 4 / 1` extraction,
extracted-page and translation limits. Every hundredth page is explicitly
preserved. Other pages pass through native extraction, compressed PageGraph
storage, translation planning, deterministic scripted provider results,
page-local unified-font fit resolution, sharded patch storage and shared-font
atomic incremental export. The test verifies output page count, sampled
translated text, a preserved page, source-prefix retention, destination
replacement and absence of atomic temp residue.

Command:

```powershell
cargo test --locked pdf_v3::acceptance::manual_windows_five_hundred_page_end_to_end_acceptance --lib -- --ignored --exact --nocapture
```

Environment and interpretation:

- Windows on the current AMD development machine;
- Rust debug test profile (`unoptimized + debuginfo`);
- Arial is the one acceptance translation font, representing the production
  unified-font policy without reading managed AppData resources;
- deterministic scripted translation excludes local model throughput;
- the fixture repeats one real renderable page and is a structural stress
  test, not a varied 500-page corpus quality measurement.

| Measurement | Result |
| --- | ---: |
| Pages | 500 |
| Completed / preserved pages | 495 / 5 |
| Fitted translation entries | 3,465 |
| Complete pipeline | 355,253 ms |
| Extraction worker wall time | 58,568 ms |
| Translation worker wall time | 295,703 ms |
| Export | 213,678 ms |
| Native reconciliation | 4,738,551 us |
| PageGraph store | 25,191,936 us |
| PageGraph JSON + gzip | 18,598,344 us |
| Scripted planning + fit renderer | 188,236,804 us |
| Patch store | 36,561,329 us |
| Peak process working set | 39,612,416 B |
| Final private bytes | 14,311,424 B |

| Disk measurement | Result |
| --- | ---: |
| Source PDF | 103,679 B |
| Logical PageGraphs | 310,374,392 B |
| Compressed PageGraph payloads | 23,972,299 B |
| Complete PageGraph store | 24,156,807 B |
| Logical patch payloads | 35,941,347 B |
| Complete patch store | 36,130,664 B |
| Scheduler | 219,438 B |
| Unified font subset | 23,336 B |
| Incremental PDF append | 325,926 B |
| Final translated PDF | 429,605 B |

The run closes the repeated-real-page 500-page bounded pipeline gate. Memory
remained below 40 MB despite more than 346 MB of logical PageGraph and patch
data being streamed through the process. Final PDF growth was about 652 bytes
per translated page plus one shared 23 KB font subset; it did not embed a font
or full source copy per page.

Debug throughput is not yet acceptable as a product performance claim. The
largest measured stage is page-local planning and renderer fit resolution,
followed by patch persistence and PageGraph persistence. The next performance
work should profile these boundaries in a non-production optimized benchmark
workflow and investigate a compact patch/PageGraph disk schema without
weakening durable authority. The equivalent 1,000-page harness exists but has
not yet been recorded, and a varied complex real-world 500/1,000-page corpus
remains required.

## Follow-Up: Replacement Planning De-duplication

The replacement renderer now builds one page-local operation index for all
targets in a physical content stream. The index resolves text state before
each target, `BT`/`ET` ownership and the later-position boundary in one forward
and one reverse pass. The renderer also caches decoded content streams for the
duration of one page render, keyed by stream identity and Form invocation path.
The cache is intentionally not retained across pages or jobs.

The 20-page deterministic smoke acceptance was rerun on the same Windows AMD
machine in the Rust debug profile:

| Stage | Previous baseline | After de-duplication |
| --- | ---: | ---: |
| Complete pipeline | 12,100 ms | 11,786 ms |
| Translation processor | 6,970 ms | 6,552 ms |
| Planning + patch assembly | not split | 1,053 ms |
| Font planning | not split | 1,090 ms |
| Font preparation | not split | 18 ms |
| Font staging | not split | 69 ms |
| Replacement render stage | 4,590 ms | 4,318 ms |
| Export | not split | 7,775 ms |

The measured change is a small improvement at this scale and is subject to
debug-run noise; it is not a claim about 500-page throughput. Its durable
benefit is removing repeated scans and repeated stream decoding from the
renderer hot path while retaining the existing conservative validation rules.
The full PDF v3 test suite remains green after the change.
