# PDF Rosetta Batch Backend Start

Date: 2026-07-02

## Summary

Started the default-beta implementation of the Rosetta-specific PDF batch
backend described in
`docs/engineering/plans/2026-07-02-pdfmathtranslate-rosetta-batch-backend-plan.md`.

This change moves the Rosetta-specific PDF batch backend toward the default
beta path. The persistent pdf2zh worker now asks the PDF component for
`service="rosetta-batch"` unless `ROSETTA_PDF_FORCE_OPENAI_SHIM=1` is set as an
explicit engineering escape hatch. Beta users are expected to update to the new
Rosetta PDF component; the old OpenAI-compatible PDF shim is no longer treated
as the normal product path.

## Scope

- Added `/v1/rosetta/batch-translations` to the local PDF shim server.
- Added a worker-side handoff to the PDF component's native
  `rosetta-batch` translator. The PDF component owns per-item cache semantics,
  ordered results, and its `translate_many(...)` implementation.
- Updated the one-shot CLI path to use the same `rosetta-batch` service by
  default. `ROSETTA_PDF_FORCE_OPENAI_SHIM=1` remains available only for local
  diagnosis while the new component is being validated.
- Moved the Rosetta-managed PDF worker toward the new component contract:
  the bundled layout model is now expected to be
  `models/doclayout_yolo_docstructbench_imgsz1024.onnx`, worker warmup uses the
  PDFMathTranslate ONNX layout entry point, and old Torch/doclayout_yolo worker
  prewarm code was removed.
- Passed `ROSETTA_BATCH_JOB_ID` through both the persistent worker and one-shot
  CLI paths so the native PDF component's `rosetta-batch` translator can tag
  Rosetta batch requests consistently.
- Did not change the durable PDF page state model, ordinary TXT/Markdown
  scheduling, or installer profile. Pack URLs still need to be updated after the
  new Rosetta PDF component artifact is uploaded.
- Built a local Windows dogfood PDF component pack from the patched
  PDFMathTranslate checkout:
  `rosetta-app/dist/pdf-layout/rosetta-pdf2zh-windows-amd64.zip`.
  The pack uses Python 3.12.13, PDFMathTranslate 1.9.11 source, ONNX Runtime,
  and `models/doclayout_yolo_docstructbench_imgsz1024.onnx`; it does not include
  Torch or `doclayout_yolo` as importable packages.
- Recorded local artifact metadata:
  size `320275049` bytes, SHA256
  `c36133090f1221542b1d4a53f8d576e2b80e5f96ec31129c1ea1c74318f49542`, layout
  model SHA256
  `fece9af02f618b603ff7921ccec6861d13e7e1f9830e091dfb7e8ad9311e5b21`.
- Fixed local dogfood installation: when `packUrl` or
  `ROSETTA_PDF2ZH_PACK_URL` points to a custom PDF component archive, the
  installer no longer reuses the built-in profile's old SHA256/size. Custom
  archives may still opt into explicit verification with `packSha256` and
  `packSizeBytes`.
- Fixed two Rosetta worker compatibility issues found during local dogfood:
  PDFMathTranslate 1.9.11 no longer exports `pdf2zh.pdf2zh.setup_log`, and the
  newer `pdfminer.six` page objects used by the pack may not expose `pageno`.
  The worker now configures logging through a local compatibility helper and
  reattaches Rosetta's explicit zero-based page index before passing pages into
  pdf2zh conversion.
- Fixed a follow-up Rosetta worker crash in target-language layout:
  `AttributeError: 'NoneType' object has no attribute 'char_lengths'`. The
  streaming worker now loads the same target Noto font as PDFMathTranslate
  1.9.11's high-level path, injects it into page resources, and passes
  `noto_name`/`noto` into `TranslateConverter`.
- Added a Lightning-only direct path for `/v1/rosetta/batch-translations`.
  Rosetta batch requests now bypass the legacy PDF shim queue when the active
  provider is Lightning, call `translate_batch_via_lightning(...)` directly,
  preserve item order, split only over-budget items, and reassemble split
  outputs before returning to the PDF component.
- Kept the legacy `PendingTranslation` queue path for non-Lightning PDF
  providers and OpenAI-compatible fallback behavior.
- Added privacy-safe worker diagnostics for native `translate_many(...)` calls:
  `page.processPage.translateBatch` stage events plus batch count, item count,
  failed batch count, and batch duration totals in receive-layout summaries.
- Started Phase 7 cross-page batching inside the persistent worker for
  `service="rosetta-batch"`. The worker now has a chunk-local two-pass path:
  first collect text units with a deferred translator, then call the real
  Rosetta batch translator once for the collected units, then replay the pages
  with a pretranslated translator to produce the usual page-level PDF
  artifacts.
- Scoped cross-page batching to Rosetta's native PDF translator. Local
  diagnosis can disable it with `ROSETTA_PDF_CROSS_PAGE_BATCH=0` or
  `ROSETTA_PDF_DISABLE_CROSS_PAGE_BATCH=1`.
- Added timeline stages for `crossPageBatch.collect`,
  `page.collectTranslationUnits`, and `crossPageBatch.translate` so model time
  and extraction/replay time can be separated without logging source or
  translated text.

## Validation

Validation completed for this implementation pass:

```bash
cd rosetta-app
.\node_modules\.bin\tsc.cmd --noEmit --pretty false
cd src-tauri
cargo check
cargo test managed_pdf2zh
cargo test rosetta_jobs
```

Additional syntax check:

```bash
C:\Users\Leo\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe -m py_compile rosetta-app\src-tauri\src\managed_pdf2zh\rosetta_pdf2zh_worker.py
```

Runtime PDF benchmarking remains a follow-up because the new path requires the
updated Rosetta PDF component plus a local RWKV provider.

Additional pack validation completed locally:

- The Windows pack build script completed successfully.
- The script-level import smoke test passed before and after pruning caches.
- A separate extract-and-import smoke test confirmed `pdf2zh 1.9.11`,
  `RosettaBatchTranslator.name == "rosetta-batch"`, batch support enabled, and
  no importable `torch` or `doclayout_yolo` package in the packed runtime.
- The published zip does not contain
  `doclayout_yolo_docstructbench_imgsz1024.onnx.optimized`; ONNX Runtime may
  generate that machine-local cache after first model initialization.
- A source-worker smoke test using the installed dogfood pack's Python and ONNX
  model now reaches the `ready` event.
- A one-page worker smoke test now emits a `page` event and writes
  `page-0001.pdf` with the installed dogfood pack.
- A smoke test against the real imported `2604.17278v1.pdf` source with a fake
  local `rosetta-batch` endpoint now emits a `page` event and writes page 1.
  The page produced one native batch request containing 8 text items and 1788
  source characters, confirming the current implementation has page-local
  batching but still needs wider cross-page aggregation for full RWKV batch
  utilization.
- A Phase 7 fake-backend smoke test against pages 1-2 of the same PDF now
  writes `page-0001.pdf` and `page-0002.pdf` while sending exactly one
  `/v1/rosetta/batch-translations` request containing 18 text items and 6,974
  input characters. This validates the worker collect / global translate /
  replay chain before real Lightning benchmarking.
- A real Lightning forced-retranslate benchmark on the same 10-page PDF now
  confirms Phase 7's main throughput win:
  - run `run-pdf-1782975492135`
  - total wall `16.760s`
  - `pagesTranslated=10`, `pagesFailed=0`
  - `rwkv.requestCount=1`
  - `rwkv.averageBatchSize=132.0`
  - `rwkv.totalRequestMs=3.931s`
  - `rwkv.totalInputChars=36,381`
  - `rwkv.totalOutputChars=13,355`
- Compared with the previous direct-path run, provider requests dropped from
  `10` to `1`, average batch size rose from `13.2` to `132.0`, summed RWKV
  request time dropped from about `19.626s` to `3.931s`, and total wall time
  dropped from `27.943s` to `16.760s`.
- Timeline diagnostics now show the next bottlenecks: the two-pass Phase 7
  implementation repeats layout inference (`page.yoloPredict=20` events,
  `4.700s` total for 10 pages) and single-page PDF saving is now a large share
  of wall time (`page.saveSinglePdf=10` events, `3.385s` total).
- Added the next Phase 7 worker optimization: the replay pass now reuses the
  collect-pass layout mask and cached pdfminer `LTPage` tree. In a fake-backend
  2-page smoke, replay no longer emitted `page.processPage.renderStreams`
  events, while `page.yoloPredict` dropped to one event per page and page PDFs
  were still produced normally.
- Split `page.saveSinglePdf` diagnostics into `insertPage` and `writeFile`.
  The same fake-backend smoke showed the old save bottleneck was
  `writeFile`, not page insertion.
- Switched single-page PDF artifact saving to speed-first local cache output
  (`deflate=0`) with `ROSETTA_PDF_SINGLE_PAGE_DEFLATE=1` as a local escape
  hatch. In the fake-backend 2-page smoke, save time dropped from about
  `667ms` total to about `15ms` total, but page artifacts grew to roughly
  `14MB` each. This tradeoff must be checked on the real 10-page and 18-page
  PDFs before treating it as final product behavior.
- A real 10-page forced-retranslate benchmark after replay reuse and
  speed-first page artifact saving completed in `8.636s` on
  `run-pdf-1782979020192`. The run preserved one Lightning request with batch
  size `132` and `rwkv.totalRequestMs=3.689s`. Compared with the previous
  Phase 7 run (`16.760s`), `page.yoloPredict` dropped from `20` events /
  `4.700s` to `10` events / `2.373s`, `page.saveSinglePdf` dropped from
  `3.385s` to `62ms`, and replay used `page.processPage.replayLayout` instead
  of repeating pdfminer stream rendering. The 10 committed translated page
  artifacts totaled about `137.8MB`, so the speed/disk-space tradeoff remains
  an explicit follow-up.
- A real 18-page forced-retranslate benchmark on
  `job-1782930613972-2605-14926v2--1` completed in `14.663s` on
  `run-pdf-1782979570988`, compared with the old same-job baseline
  `42.059s`. Provider requests dropped from `20` to `1`, average provider
  batch size rose from `10.55` to `212.0`, and summed RWKV request time dropped
  from `29.404s` to `4.618s`. The 18 committed translated page artifacts
  totaled about `257.8MB` under the speed-first page artifact setting.
