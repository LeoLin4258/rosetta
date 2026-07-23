# 2026-07-20 PDF v3 Component Prewarm and Resolution Diagnostics

## Summary

- Split trusted PDF v3 component resolution into privacy-safe timing stages for
  managed-runtime binding, PDF asset lookup, sidecar/model digesting, font
  loading, manifest construction and runtime identity recheck.
- Record those stages in native logs and PDF v3 run-creation diagnostics,
  including process-local cache-hit flags without paths or document text.
- Probe and warm the native component in the App shell after the selected local
  translation runtime becomes ready, keyed by runtime profile and target
  language.
- Replace the PDF header's legacy pdf2zh worker badge with the actual v3
  component waiting/warming/ready/failed state.
- Stop automatically prewarming the unused legacy Python/doclayout worker at
  App startup.
- Classify native renderer failures by font planning, font assets, geometry,
  style, ownership, transaction, patch, content and cache stage.
- Preserve only affected legacy text-show entries when a unified translation
  font lacks a returned glyph instead of failing the complete page.
- Reuse the managed install manifest's verified model SHA-256 after a bounded
  file-kind and byte-count check, removing the 501 MB GGUF re-hash from normal
  App startup.

## Windows AMD Evidence

Fresh process, Simplified Chinese target:

```text
total=15238ms
runtime=3ms
sidecar digest=1ms (cache miss)
model digest=14636ms (cache miss)
fonts=596ms (cache miss)
manifest=0ms
```

Second resolution in the same App process:

```text
total=3ms
runtime=2ms
sidecar digest=0ms (cache hit)
model digest=0ms (cache hit)
fonts=0ms (cache hit)
```

The first-click delay was therefore dominated by the first complete SHA-256 of
the 501 MB managed GGUF on this historical Windows run, not by PDF parsing or
font loading. Normal startup now trusts the installer/update/repair receipt
after bounded file-kind and byte-count checks, so restarting the App no longer
re-reads the GGUF. Background prewarm still resolves fonts and the component
manifest before a translate click.

The Drylab page-2 retry reached the renderer and was classified as
`pdf-v3-renderer-font-missing-glyphs`. Missing prepared glyphs are now a typed
entry-preservation reason (`translation-font-glyph-unavailable`) so one unusual
model character cannot fail the page. This is a conservative compatibility
measure for the legacy object renderer, not the visual-quality solution in ADR
0076.

After the change, the same page completed with one translated-page patch and
zero failed pages. Its warm run creation took 108 ms: component resolution 4
ms, source preparation 41 ms, authority commit 49 ms and status sync 16 ms.
The rendered result still exhibits fragment-level bilingual output and broken
paragraph flow, confirming the need for ADR 0076 rather than further legacy
fit tuning.

The removed legacy worker separately measured about 5.7 seconds of Python and
doclayout warmup on the same start. Native v3 did not consume it.

## Validation

```text
pnpm typecheck
cargo check
cargo test rosetta_jobs::formats::pdf::v3_component --lib
cargo test rosetta_jobs::formats::pdf::v3_processor --lib
cargo test rosetta_jobs
```

The final Windows run passed 6 PDF v3 processor tests and 128 `rosetta_jobs`
tests.
