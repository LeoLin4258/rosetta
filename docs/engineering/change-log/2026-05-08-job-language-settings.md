# 2026-05-08 Job Language Settings

## Summary

Added job-level source and target language configuration to the Tasks page.

Users can now choose the source language and target language for the active translation job before starting translation. The selected direction is saved into the local job cache and used by the RWKV `/v1/chat/completions` prompt builder.

## Changes

- Added a Rust command:
  - `update_rosetta_job_languages`
- Persisted language changes to:
  - `document.json`
  - `segments.json`
  - `index.json`
- Updated all segments with the selected `sourceLang` and `targetLang`.
- Reset existing translatable segment translations when the language direction changes, so old translations are not shown under a new target language.
- Added language selectors to `/jobs/:jobId`:
  - source language
  - target language
- Extended RWKV API translation request with optional `sourceLang` and `targetLang`.
- Updated the RWKV prompt builder from a hard-coded `English -> Chinese` prompt to language-label based prompts:

```txt
<SourceLabel>: <source text>

<TargetLabel>:
```

## Boundaries

- The confirmed and tested model path remains English -> Chinese.
- Other language directions are now represented in UI, job data, and prompt construction, but model quality/stability still needs empirical testing.
- API probe remains English -> Chinese because it is meant to verify the currently confirmed RWKV engineer endpoint.
- No dev server or build command was run.

## Validation

Executed:

```txt
cargo fmt
cargo test rwkv_api
cargo test rosetta_jobs
cargo check
corepack pnpm typecheck
```
