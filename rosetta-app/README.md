# Rosetta App

This is the Tauri desktop application for Rosetta, a local-first long-form document translation workbench.

## Stack

- Tauri v2 for local desktop capabilities
- React, TypeScript, Vite
- Tailwind CSS and shadcn/ui
- Zustand for app state
- React Router for navigation
- `@tanstack/react-virtual` for long document previews

## Product Boundary

Rosetta is not a chat product or cloud translation service. Keep work focused on local document translation, privacy-sensitive files, long-text structure preservation, and batch translation through a local or explicitly configured RWKV translation API.

## Current v1 Format Scope

v1 needs TXT, Markdown, and text-based PDF support.

PDF support means importing extractable PDF text into the existing Rosetta document pipeline:

```txt
PDF -> RosettaDocument -> RosettaBlock[] -> Segment[] -> translation file -> preview/export
```

High-fidelity PDF format restoration is a nice-to-have enhancement if it can be isolated behind the PDF importer/exporter boundary and still falls back to the baseline text pipeline. v1 does not require OCR, arbitrary perfect layout recreation, default PDF write-back, document Q&A, summarization, or cloud parsing.

## Directory Guide

```txt
src/
  app/          app shell, router, navigation
  components/   shared and shadcn/ui components
  features/     user-facing feature pages and workflows
  lib/          Tauri command wrappers and shared utilities
  store/        Zustand store
  styles/       global CSS
  types/        shared domain types

src-tauri/src/
  lib.rs
  main.rs
  rosetta_jobs/
    mod.rs             Tauri command facade
    model.rs           durable DTOs and cache constants
    import.rs          single-file and directory import orchestration
    export.rs          translation and bilingual export rendering
    store.rs           JSON index and job bundle persistence
    translation_files.rs
    revisions.rs
    formats/
      mod.rs           source format detection and parser dispatch
      txt.rs
      markdown.rs
  rwkv_api.rs          external RWKV translation API connector
  rwkv_runtime.rs      parked runtime-management experiment
```

New format work should go through `rosetta_jobs/formats/` and the existing importer/exporter boundary instead of adding parsing logic to the command facade.

## Validation

Run targeted validation when relevant:

```powershell
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Do not run dev servers or production builds unless a task explicitly asks for runtime UI verification or release packaging.
