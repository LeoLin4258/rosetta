# PDF v3 PageGraph Reconciliation Spike

Date: 2026-07-17

## Summary

Upgraded the isolated PDF v3 PageGraph contract to schema v2 and added an
atomic reconciliation stage between PDFium page-text atoms and decoded
content-stream operands.

The implementation remains disconnected from PDF v2, jobs, UI, translation,
persistence, render cache and export.

## Implementation

Added:

- source decoder output with Unicode-to-encoded-byte ranges;
- stable operand provenance for `Tj`, `TJ`, `'` and `"` string operands;
- ligature-aware source-unit character indexes;
- PageAtom source states for verified, corrected, synthetic and preserved text;
- object-atomic PageGraph update planning;
- explicit accounting for source whitespace without PDFium geometry;
- page-level complete, partial and preserved summaries;
- conservative whole-object fallback for decoder, font, atom coverage and
  alignment failures;
- deterministic repeated-reconciliation tests;
- fixture corpus and Windows real-page probes.

Mapping diagnostics continue to omit source text and encoded byte payloads.
PageGraph itself contains local source text as required by the extraction IR and
must not be emitted through ordinary diagnostics or telemetry.

## Windows AMD Results

Page 1 of `2305.13048v2.pdf`:

- 3,911 atoms;
- 3,238 PDFium/source-verified atoms;
- 15 ToUnicode-corrected atoms;
- 602 PDFium synthetic whitespace atoms;
- 2 source whitespace characters without a PDFium atom;
- 56 preserved Form XObject atoms;
- 242 / 242 top-level text objects mapped;
- page status: `partial`;
- elapsed: about 606 ms in the unoptimized probe.

Fixture first pages:

- `simple-one-page.pdf`: complete, 80 verified, 6 synthetic, 0 preserved;
- `pdflatex-image.pdf`: complete, 505 verified, 112 synthetic, 0 preserved;
- `multicolumn.pdf`: partial, 2,891 verified, 16 corrected, 556 synthetic,
  49 preserved;
- `google-doc-document.pdf`: partial, 913 verified, 210 synthetic,
  16 preserved;
- `GeoTopo.pdf`: preserved, 4 synthetic and 82 preserved.

## Remaining Boundaries

- Form XObject content and inherited resources are not recursively reconciled.
- The current builder reloads the PDF for extraction and mapping. It is a
  correctness spike, not the final long-document DocumentHandle.
- No translated Unicode is encoded or rendered.
- PageGraph persistence and compression are not connected.
- No production job or frontend state changed.

## Validation

- `cargo test pdf_v3`: 29 passed, 0 failed, 6 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- first-page fixture reconciliation matrix;
- explicit Windows real-page reconciliation probe;
- deterministic PageGraph JSON round trip;
- mapping diagnostic payload exclusion test;
- `git diff --check`: passed (Git reported only existing LF-to-CRLF warnings);
- repository and `src-tauri` `tmp/pdfs` directories: 0 files.
