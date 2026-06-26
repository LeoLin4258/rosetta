# 2026-06-26 PDF Timeline Diagnostics

## Summary

Added a job-local PDF lifecycle timeline at:

```txt
<job>/diagnostics/pdf-timeline.jsonl
```

The timeline records ordered timestamps for PDF import and page-translation
runs, including run start/end, chunk start/end, per-page artifact commits,
durations, provider IDs, page counts, file sizes, and aggregate RWKV timings.
It also records persistent-worker internal `worker.stage` events for PDF
preprocessing, YOLO layout inference, pdfminer page processing, patch
application, single-page PDF saves, and page event emission.

The persistent PDF worker startup prewarm now also performs one synthetic
blank-page YOLO prediction before it reports `ready`, so the first user-visible
translated page no longer pays as much of YOLO's first predict-time setup.
The ready log records the prewarm status, duration, device, and failure reason
when applicable.

`page.processPage` is now expanded into lower-level worker stages for
pdfminer/page-device work (`beginPage`, `renderStreams`, `endPage`,
`receiveLayout`, and `patchStreams`) plus per-page
`translateRequest` timings. The request timings record only counts, character
lengths, durations, and error types; they do not record source text, translated
text, prompts, or model responses.

The default PDF OpenAI-shim paragraph batch width, which also drives the
persistent worker's pdf2zh `thread` count, was raised from 4 to 8. This is an
instrumented performance experiment aimed at reducing first-page
TextConverter translation waves; the effective thread count and per-request
wave pattern are visible in `pdf-timeline.jsonl`.

See also:

- `docs/engineering/change-log/2026-06-26-pdf-first-page-latency-investigation.md`
  for the measured first-page latency findings, fixes, and remaining
  optimization candidates.

## Motivation

PDF performance investigations previously required stitching together several
separate sources:

- app stderr logs;
- optional `rwkv-io-debug.jsonl`;
- per-run translation profiles;
- page state files;
- page artifact modification times.

That made it hard to answer basic questions such as "how long from import to
first translated page?" or "which chunk delayed the first visible output?".
The new timeline gives each PDF job one ordered, local diagnostic trail.

## Privacy Boundary

Timeline events are diagnostics only. They must not contain source text,
translated text, prompts, model responses, or document content. Events are
limited to IDs, timestamps, counts, durations, page numbers, file sizes,
provider identifiers, and aggregate RWKV timing counters.

## Files Changed

- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/diagnostics.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/import.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/mod.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/worker.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/rosetta_pdf2zh_worker.py`
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs`
- `docs/engineering/pdf-pipeline.md`

## Validation

Run:

```powershell
cd rosetta-app/src-tauri
cargo test rosetta_jobs
```
