# AGENTS.md

## Project

Rosetta is a local-first desktop app for long-form document translation. It uses Tauri v2, React, TypeScript, and a local RWKV translation service.

The product focus is narrow:

- local translation
- privacy-sensitive documents
- long text and document structure preservation
- batch translation through a local model API

Do not turn Rosetta into a generic AI assistant, chat product, cloud translation service, or collaboration tool.

## Required Reading

Before making architectural changes, large feature changes, or data model changes, read:

- `docs/rosetta_project_plan.md`
- `docs/engineering/README.md`
- `docs/engineering/conventions/frontend.md`
- `docs/engineering/conventions/data-models.md`

For stack-level decisions, also check:

- `docs/engineering/decisions/`

## Development Rules

- Prefer the existing project structure over introducing new abstractions.
- Keep changes scoped to the user request and the current feature boundary.
- Do not add cloud upload, login, sync, telemetry, or account features unless explicitly requested and documented.
- Do not add chat, summarization, rewriting, document Q&A, or generic AI assistant behavior.
- Large project changes must update `docs/engineering/change-log/`.
- Architectural decisions must be recorded in `docs/engineering/decisions/`.
- Core data model changes must update `docs/engineering/conventions/data-models.md`.
- Keep persistent data format changes backward-compatible or document the migration path.

## Frontend

- Use React, TypeScript, Tailwind CSS, shadcn/ui, Zustand, React Router, and `@tanstack/react-virtual`.
- shadcn/ui is initialized with preset `bJMSkhvs`; the theme color is `stone`.
- Keep the UI as a desktop workbench, not a landing page or chat interface.
- Use predictable navigation, clear hierarchy, and restrained styling.
- Use virtual scrolling for long document, block, or segment previews.
- Do not render all blocks or segments of a large document at once.
- Prefer shadcn components from `rosetta-app/src/components/ui/` for common UI controls.
- Use semantic tokens such as `bg-background`, `bg-card`, `text-foreground`, `text-muted-foreground`, and `border-border`.
- Keep global CSS in `rosetta-app/src/styles/index.css`.
- Put shared domain types in `rosetta-app/src/types/`.

## Tauri

- Keep local file, system dialog, app data directory, and process access behind Tauri commands or plugins.
- Do not broaden Tauri permissions without documenting why.
- Prefer narrow command APIs over exposing broad filesystem access to the frontend.
- Treat privacy and local-only behavior as product requirements, not optional polish.

## Validation

When relevant, run:

```bash
cd rosetta-app
corepack pnpm typecheck
corepack pnpm build
cd src-tauri
cargo check
```

If a validation command cannot be run, state why in the final response.


## DO NOT
- Do not run dev or build
