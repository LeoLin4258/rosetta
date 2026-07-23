# 2026-07-21 PDF Production Refactor Closeout

## Status

The production PDF refactor is functionally complete and has passed the user's
ten-page App acceptance test. PDF resource-pack publication is intentionally
deferred while unrelated UI/UX work continues.

This document is the current handoff authority for the refactor. It supersedes
the unresolved status in
`2026-07-20-pdf-v3-ten-page-benchmark-regression-handoff.md` without deleting
that document's historical failure evidence.

The completed scope is:

- restore the proven pdf2zh extraction and visual-rendering path as production
  authority;
- retain Rosetta's bounded scheduling, state, cache, recovery, preview, export,
  and artifact lifecycle infrastructure;
- preserve the established RWKV request plan and translation throughput;
- reduce warm cache-miss PDF preparse time substantially;
- keep translated-page storage bounded and compressed;
- avoid the native region renderer's source-preservation regression.

The refactor is ready to leave active PDF development. It is not yet a shipped
PDF component release.

## Final Architecture

Production responsibilities are divided as follows:

- PDFMathTranslate/pdf2zh owns document preparation, layout inference,
  translation-unit collection, and visually authoritative page rendering.
- Rosetta owns local RWKV translation, document-wide batching, job/run state,
  cancellation, recovery, cache identity, page artifact commit, background
  compression, preview, and export.
- The retained PDF v3 stores and scheduler infrastructure remain isolated from
  production visual rendering through the bounded legacy adapter.
- Translation units remain the established pdf2zh units. The failed native
  visual-paragraph grouping and region reflow path is not the production
  execution authority.

This preserves the useful parts of the native rewrite without requiring its
renderer to reproduce pdf2zh's mature PDF content-stream behavior. The durable
decision is recorded in
`docs/engineering/decisions/0077-pdf-production-execution-rollback.md`.

## What Was Retained From The Native Rewrite

The earlier rewrite was not discarded in full. The retained work includes:

- bounded page/run scheduling and explicit page authority;
- cancellation, leases, stale-run recovery, and shutdown behavior;
- durable compressed patch/store primitives;
- source identity and cache identity validation;
- page-local preview and full-document export infrastructure;
- bounded status projections and privacy-safe diagnostics;
- font and artifact lifecycle work;
- the adapter boundary that allows the proven renderer to coexist with the
  newer orchestration and persistence infrastructure.

The rejected production pieces are the native region renderer, visual
paragraphs as provider execution units, page-serial provider scheduling, and
source-preserving container completion semantics. Their failure remains
documented in the superseded regression handoff.

## Exact Acceptance Fixture

Primary fixture:

```text
C:\Users\Leo\Desktop\pdf-set-1\2604.17278v1.pdf
```

Properties:

- 10 pages;
- source fingerprint used by historical comparisons:
  `sha256:5db8200931a2d4104cf435a70701e80d47849c201000ed86ca645ab25d454da2`;
- 105 collected units;
- 96 translatable units;
- 41,083 translatable source characters in the serialized pdf2zh authority;
- serialized unit payload SHA-256:
  `1112ac7b7b4509d7a447c31df8ea8b5a4c964e5ae69b304ca80a93ef7c128c42`.

Do not replace this fixture with a smaller synthetic PDF when validating a
future PDF execution or packaging change.

## Performance Result

### Controlled Engine Benchmark

| Measurement | Before | Final | Change |
| --- | ---: | ---: | ---: |
| Warm cache-miss `prepareRun`, median | 7,473.7 ms | 2,529.2 ms | -66.2% |
| Ten-page layout analysis | 2,923.0 ms | 1,388.9 ms | -52.5% |

The final Windows path uses ONNX Runtime DirectML with a maximum five-page
layout batch. Pages are grouped by tensor shape and restored to source order
before unit collection. Initialization or inference failure falls back to the
CPU provider.

### Final Full-App Manual Runs

The user imported and translated the exact ten-page fixture twice after the
final component changes:

| Measurement | Run 1 | Run 2 |
| --- | ---: | ---: |
| Cache-miss preparse | 3,006 ms | 2,841 ms |
| Complete translation | 107,136 ms | 104,220 ms |
| RWKV request time | 106,948 ms | 104,031 ms |
| RWKV requests | 20 | 20 |
| Failed RWKV requests | 0 | 0 |
| RWKV input characters | 39,841 | 39,841 |
| RWKV output characters | 13,576 | 13,709 |
| Compressed translated pages | 10 | 10 |
| Final translated-page storage | 4.29 MiB | 4.28 MiB |

Both translation profiles reported one prepared-cache hit during translation,
confirming that translation reused the import-time preparse instead of running
layout analysis again. The user confirmed that translation speed felt normal
and that visual fill-back matched the pre-refactor behavior.

RWKV accounts for nearly the complete 104-107 second translation duration.
PDF rendering accounts for approximately 1.0-1.2 seconds. Further PDF-side
optimization therefore cannot materially reduce complete translation time
without changing the model path.

## Correctness And Visual Result

The controlled CPU and DirectML paths preserved:

- identical translation-unit count, source-character count, and serialized
  payload hash;
- array-identical layout masks on all ten pages;
- identical source-substitution artifact byte totals;
- 10/10 pixel-identical Poppler PNG page pairs;
- exact sparse-page behavior for pages 2, 5, and 10 before and after durable
  layout-cache restore.

Pages 1 and 10 were inspected directly during automated regression work. The
user subsequently inspected the complete translated document in the App and
accepted the visual result.

## Storage And Memory Bounds

All ten final translated pages reported `artifactCompression=compressed` and
occupied approximately 4.3 MiB in both accepted App runs.

DirectML changes the fixed PDF worker memory tradeoff:

| Provider | Approximate peak worker RSS |
| --- | ---: |
| CPU | 497 MiB |
| DirectML, batch 5 | 1,367 MiB |

The batch is capped at five pages, so layout inference memory does not increase
linearly with document page count. This is a fixed higher peak, not permission
to retain complete long-document translations or rendered pages in memory.
Rosetta must continue committing and releasing page state incrementally.

## Component Footprint

The currently installed and already-used Windows development component occupies
987,771,713 bytes, or approximately 942 MiB:

| Installed area | Approximate size |
| --- | ---: |
| Python runtime and packages | 744.3 MiB |
| Models | 143.7 MiB |
| Assets | 34.4 MiB |
| Cache | 19.6 MiB |

The models directory contains the 71.8 MiB source ONNX model and a 71.8 MiB
ONNX Runtime optimized copy generated during use. The Windows build script
removes the optimized copy before creating the release archive, so it is an
installed-runtime cost rather than a download cost.

The currently pinned old release archive is 349,587,199 bytes, or approximately
333.4 MiB. The relevant compressed dependency changed from the 13,358,769-byte
CPU ONNX Runtime wheel to the 25,111,930-byte DirectML wheel. If all other
archive inputs remain equal, the new archive is expected to increase by about
11.2 MiB, or 3.4%. Treat this only as a planning estimate; the clean release
build's final size and SHA-256 are authoritative.

## Automated Validation

Final source validation completed successfully:

```text
cd rosetta-app
pnpm typecheck
  passed

cd rosetta-app/src-tauri
cargo check
  passed

cargo test rosetta_jobs
  132 passed; 0 failed

PDFMathTranslate:
uv run pytest test/test_doclayout.py test/test_rosetta_engine.py test/test_converter.py -q
  25 passed

Rosetta patch suite:
python rosetta-app/src-tauri/scripts/test-pdf2zh-patches.py -q
  33 passed

git diff --check
  no whitespace errors; CRLF conversion warnings only
```

These tests supplement rather than replace the user's full-App acceptance.
That manual acceptance is now complete for the ten-page fixture.

## Current Source And Component State

### Rosetta

The Rosetta worktree contains the PDF rollback, bounded adapter, performance,
packaging, diagnostics, and related UI integration changes together with other
ongoing work. Do not revert unrelated existing modifications when isolating the
PDF changes.

### PDFMathTranslate

At closeout time, the sibling fork is based on:

```text
990bed055d372772f5cec8ef4a982a8f767d64a4
```

with uncommitted changes in:

```text
pdf2zh/converter.py
pdf2zh/doclayout.py
pdf2zh/rosetta_engine.py
test/test_doclayout.py
test/test_rosetta_engine.py
```

The Windows pack builder reads the sibling checkout through
`Pdf2zhSourcePath`; it does not reconstruct these changes from the Rosetta
repository. Before building a release resource pack, commit the fork changes
and record the resulting exact commit in the release handoff and pack manifest.
`990bed0 plus local changes` is not a reproducible release identity.

### Installed Development Pack

The installed Windows component under:

```text
C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64
```

was patched in place for development acceptance. Its installed manifest still
identifies the old `pdf-layout-pack-windows-x64-v2026.07.16.1` archive. Do not
publish or copy this mutable directory as a release artifact.

The checked-in Windows profile also intentionally remains pinned to the old
archive until release work resumes.

## Deferred Release Checklist

When PDF component release work resumes:

1. Commit and pin the PDFMathTranslate fork changes.
2. Build a clean Windows amd64 resource pack with
   `build-pdf2zh-pack-windows-amd64.ps1`.
3. Require the build smoke test to report `DmlExecutionProvider` with CPU
   fallback available.
4. Rerun the focused 25 upstream tests and 33 Rosetta patch tests.
5. Run the exact ten-page prepare/render smoke test from the clean pack.
6. Record the archive byte size and SHA-256.
7. Upload the archive under a new immutable
   `pdf-layout-pack-windows-x64-vYYYY.MM.DD.N` release tag.
8. Update `managed_pdf2zh/profile.rs` with the new size, hash, mirror URL, and
   GitHub fallback URL.
9. Test both upgrade from the old pack and a completely fresh component
   installation.
10. Perform one final full-App translation before shipping the App version.

Do not overwrite the old release asset. Existing App versions pin its size and
hash.

## Known Non-Blocking Follow-Up

### Batched Layout Timing Attribution

The worker timing wrapper instruments `build_layout_mask`, while DirectML uses
`build_layout_masks`. Consequently, final App diagnostics report `layout=0`
and attribute batched inference time to `other`. Total preparse time remains
correct. Fix the timing wrapper before relying on per-stage layout telemetry in
future performance work.

### Long-PDF Product Soak

The five-page inference cap, incremental page commits, translation-state
release, and artifact compression provide bounded implementation behavior.
However, a real 500/1,000-page full-App soak has not been completed for this
final combination. Run it before claiming long-PDF release certification. It
is not required before unrelated UI/UX work proceeds.

### Platform Scope

DirectML is Windows-only. macOS and Linux retain their existing providers and
must not receive new resource-pack identities without platform-specific build
and regression testing.

## Performance Boundary

The low-risk large PDF-side gains are complete. Additional work may still save
fractions of a second in unit collection or cold startup, but changing layout
resolution, model behavior, extraction units, or renderer authority risks the
correctness that this refactor restored.

Do not reopen the native renderer or alter RWKV batching to pursue a small PDF
preparse gain. Any future execution-path change must first preserve the exact
fixture authority, visual result, 20-request translation plan, bounded
long-document behavior, and current storage result.

## Supporting Documents

- `docs/engineering/decisions/0077-pdf-production-execution-rollback.md`
- `docs/engineering/change-log/2026-07-20-pdf-production-execution-rollback.md`
- `docs/engineering/change-log/2026-07-21-pdf-v3-bounded-legacy-unit-adapter.md`
- `docs/engineering/change-log/2026-07-21-pdf2zh-production-performance.md`
- `docs/engineering/benchmarks/2026-07-21-pdf2zh-resource-manager-reuse.md`
- `docs/engineering/plans/2026-07-20-pdf-v3-ten-page-benchmark-regression-handoff.md`
