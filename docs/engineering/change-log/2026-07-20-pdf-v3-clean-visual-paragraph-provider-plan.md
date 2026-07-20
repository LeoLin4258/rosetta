# PDF v3 Clean Visual-Paragraph Provider Plan

Date: 2026-07-20

## Summary

Added the provider-side contract required by ADR 0076 so PDF translation no
longer has to treat producer-specific text shows as semantic model inputs.
The production processor is intentionally not switched yet because the current
durable patch and renderer still operate on one text show at a time.

## Changes

- Added a page-local visual-paragraph plan derived from PageGraph line,
  paragraph and flow-container groups.
- Reconstructs natural paragraph text across source objects and style changes,
  collapses abnormal PDF whitespace, joins visual lines and handles mapped
  line-end hyphenation.
- Keeps citations, numbers, URLs, formulas and symbols identity-bound through
  exact protected tokens and validates missing, duplicate, unknown or reordered
  tokens before accepting provider output.
- Treats flow containers atomically: an unsafe paragraph preserves the complete
  container instead of allowing partially translated bilingual corruption.
- Rejects empty, implausibly short/long, apparently untranslated and fragmented
  mixed-language Chinese-target output before it can become patch authority.
- Added a provider adapter that batches visual paragraphs through the existing
  local RWKV transport without exposing document text in diagnostics.

## Drylab Evidence

Manual Windows AMD probing used the same three-page Drylab PDF that produced
the reported mixed-language regression. No source or translated text was
logged.

| Page | Legacy text-show units | Visual paragraph units | Preserved containers |
| --- | ---: | ---: | ---: |
| 1 | 123 | 8 | 0 |
| 2 | 169 | 7 | 0 |
| 3 | 95 | 8 | 0 |
| Total | 387 | 23 | 0 |

Visual grouping took approximately 2.6-4.6 ms per page in the probe. The full
three-page extraction, reconciliation, grouping and clean-plan probe completed
in approximately 0.41 seconds after test binary startup.

## Validation

```powershell
cd rosetta-app/src-tauri
cargo test paragraph_translation_plan --lib
cargo test visual_paragraph --lib
$env:ROSETTA_PDF_V3_GROUPING_PROBE='<Drylab source.pdf>'
cargo test manual_windows_external_visual_grouping_probe --lib -- --ignored --nocapture
```

The focused tests cover cross-object paragraph reconstruction, whitespace
normalization, provider batching, protected-span restoration, fragmented
mixed-language rejection and complete-container preservation.

## Next Boundary

Introduce durable region TranslationPatch entries and a flow-container renderer
that neutralizes all owned source text shows atomically and lays out translated
paragraphs once in the container. Do not split translated paragraph text back
across legacy source-object entries.
