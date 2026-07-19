# PDF v3 Trusted Native Export

Date: 2026-07-19

## Summary

Added the public native export boundary for completed PDF v3 runs and
reconnected the workbench export action.

## Implementation

- added `export_rosetta_pdf_v3_run(jobId, runId, targetPath)`;
- resolved source, PageSet, runtime manifest, patch authorities and managed
  fonts natively instead of accepting frontend identity input;
- rejected nonterminal runs, active leases, source drift and store/runtime
  mismatches before destination replacement;
- streamed only completed-page patches through the existing atomic coordinator;
- exported preserved-only runs through verified atomic source copy;
- removed the workbench's legacy PDF export call and exposed export only for a
  selected completed v3 run;
- added typed frontend result/wrapper and compact export loading state.

## Validation

- `pnpm typecheck`;
- `cargo fmt --all -- --check`;
- `cargo check`;
- `cargo test rosetta_jobs` (126 passed);
- `cargo test empty_patch_page_set_is_a_verified_byte_exact_source_copy`;
- `git diff --check`.

Tauri dev compiled and launched successfully at `http://localhost:1420/` with
the native desktop process running. The save-dialog/export click path was not
automated: a normal browser has no Tauri IPC, and the transparent Mica WebView
did not expose usable rendered content through Windows capture.

## Remaining Boundary

The native command still needs real complex 500/1,000-page Windows AMD
translation/export acceptance evidence and a future progress/cancellation
surface if export duration warrants it.
