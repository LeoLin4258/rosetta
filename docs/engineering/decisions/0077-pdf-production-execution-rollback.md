# ADR 0077: PDF Production Execution Rollback

Date: 2026-07-20

Status: Accepted

Supersedes the production-routing portions of ADR 0072, ADR 0073 and ADR 0076.
The native PDF v3 extraction, scheduler, store, recovery, font and cache
contracts remain available for further development.

## Context

The native region execution path failed the established ten-page benchmark.
The same source completed in 119-137 seconds before the rewrite and required
about 262 seconds through the native v3 path. Only 5 of 157 flow containers
were visibly reflowed; the remaining 152 retained source content while their
pages were still reported as completed.

The failure is concentrated in the production provider/render execution path:
page-local provider scheduling prevents the former cross-page wide batches,
and conservative region ownership and fit checks discard most translated
output after provider work has completed.

## Decision

Restore the existing pdf2zh-backed PDF workbench as the production execution
path.

- PDF preparse, translation, progress, page preview and export use the existing
  `preparse_rosetta_pdf_pages`, `translate_rosetta_pdf_pages`, legacy translated
  page preview and translated PDF export commands.
- Provider work is again collected across the selected page window before
  batching, rather than constrained by one native v3 page claim at a time.
- The existing pdf2zh renderer remains responsible for translated page
  artifacts and visual restoration.
- Native PDF v3 extraction, PageGraph authority, bounded scheduler, patch
  storage, cancellation/recovery primitives, fonts, preview caches and tests
  remain in the repository. They are not the default production workbench path.
- No native v3 region renderer result is treated as production acceptance until
  it independently passes the exact ten-page coverage and throughput gates.

## Persistence

This rollback adds no persistent data format and requires no migration. Legacy
page translation state remains backward-compatible. Existing native v3 run
artifacts remain isolated under `pdf-v3/` and may be inspected or removed by
existing local-data cleanup behavior.

## Acceptance

Automated validation must pass frontend typechecking, Rust compilation, focused
PDF v3 tests and `rosetta_jobs` tests. Product acceptance remains manual and
requires the user to run the exact ten-page benchmark, confirm materially
restored translation coverage and layout, and compare elapsed time with the
119-137 second historical range. Automated tests alone do not mark this
rollback accepted.

## Consequences

- The known product path regains cross-page batching and the previously proven
  visual renderer without deleting the safer native infrastructure.
- Native v3 pause/resume/recovery UI is temporarily removed from the production
  workbench; the legacy PDF run controls are restored.
- Future native renderer work must remain behind explicit development wiring
  until it passes real-document acceptance before another production switch.
