# PDF multi-window prepare cache

Date: 2026-07-15

## Summary

Rosetta's persistent PDF worker now retains multiple prepared PDF windows in a
bounded least-recently-used (LRU) cache. Switching from PDF A to PDF B and back
to PDF A in the same app session reuses A's parsed document and layout state
instead of repeating the expensive prepare pipeline.

The cache keeps six prepared windows by default, enough for the five curated
WAIC demo documents. `ROSETTA_PDF_PREPARE_CACHE_ENTRIES` can set a capacity from
1 through 32. Once the limit is reached, the least recently used prepared run
is disposed through the existing PDF engine API.

## Reliability

- Cache hits reset the prepared run to its pristine state before translation.
- A failed reset removes and disposes the damaged entry, then performs a full
  prepare.
- A failed unit collection disposes the newly created run instead of leaking
  engine memory.
- Explicit dispose requests remove only the matching cached run.
- Older component packs without `resetRun` retain the conservative behavior:
  all cached runs are disposed and the requested PDF is prepared again.
- Windows, Linux, macOS, and local-staging pack smoke tests now reject engine
  builds that do not export `resetRun`. The Linux real-PDF smoke test also
  renders once after a reset so a nominal but broken implementation cannot be
  published.
- Stage diagnostics report cache entry count and configured capacity without
  including document content.
- The worker reports the owning job IDs for its current cache after prepare,
  eviction, disposal, and recoverable errors. Tauri keeps an in-memory snapshot
  and emits updates so the frontend never infers readiness from job history.
- The frontend subscribes before reading the initial snapshot and ignores a
  stale snapshot if an event arrives during startup.

## UI

Prepared PDFs receive a 6 px low-saturation green dot on the lower-right of
their PDF icon in the document sidebar. The indicator does not add a column,
move filenames, or replace the separate translation status. Its tooltip is
`PDF 已预解析`. Worker startup now restores the indicator for compatible
durable layout entries; opening that PDF then rebuilds the live renderer state
in the background without rerunning ONNX layout inference.

PDF, Markdown, and text jobs use dedicated monochrome format icons. Partial
translation status uses a hollow amber circle, and document rows no longer
scale down while pressed.

## Scope

The complete prepared-run LRU remains process-local. Durable layout-mask
persistence is specified separately by ADR 0014; it does not attempt to
serialize pdfminer file handles or full renderer state.
