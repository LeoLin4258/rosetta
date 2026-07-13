# PDF Long Job Path Scratch Fix

Date: 2026-07-13

## Summary

Fixed PDF translation failing before unit collection when a long source
filename produced a long job ID on Windows.

The reported 217-page `The Wealth Ladder` job failed three times while
preparing pages 1-10. Every run ended in 0.6-0.9 seconds with no provider
requests and the same PyMuPDF error:

```txt
FzErrorSystem: code=2: cannot open file
```

The job-local scratch path reached 263 characters after Rosetta appended the
run directory, chunk directory, engine UUID, and `-prepared.pdf`. The bundled
PyMuPDF build reproduced the same failure at that length. A 230-character
prepared path from a shorter job completed successfully in the same app
session with the same provider.

## Change

- PDF page outputs remain in `.tmp/pdf-runs/<runId>/chunk-XXXX` until commit.
- PDF engine prepared-window files now use the short app-local
  `pdf-engine-scratch/<process-timestamp-sequence>` path.
- The scratch path does not include the job ID or source filename.
- `cleanupScratchDir` is enabled so normal worker disposal removes the
  prepared window.
- A Rust-owned drop guard provides cleanup when prepare fails before the
  Python engine registers a prepared run, and when later stages exit early.
- Added a unit test covering unique scratch allocation and cleanup on drop.

No job IDs, page-state schemas, run-state schemas, or translated artifact
paths changed. Existing long-name jobs are fixed without migration or rename.

## Validation

```powershell
cd rosetta-app\src-tauri
cargo fmt --check
cargo test pdf_engine_scratch_dirs_are_unique_and_removed_on_drop
cargo check
cargo test rosetta_jobs
```

Results:

- `pnpm typecheck`: passed.
- `cargo fmt --check`: passed.
- `cargo check`: passed.
- `cargo test rosetta_jobs`: 67 passed, 0 failed.
- Scratch allocation/drop regression: passed.

The installed Windows PDF engine also prepared page 1 of the original failing
217-page job using the new scratch layout. The prepared path was 143
characters, the prepared PDF existed, and `disposeRun` removed the scratch
directory. The smoke test did not invoke the translation provider or record
document text.
