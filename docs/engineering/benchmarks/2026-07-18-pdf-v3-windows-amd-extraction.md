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
