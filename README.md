# Rosetta

Rosetta is a local-first desktop app for long-form document translation. It is built with Tauri v2, React, TypeScript, and a local RWKV translation service.

## Project Layout

```txt
docs/                       Product and engineering planning
docs/engineering/           Engineering decisions, change logs, and conventions
rosetta-app/                Tauri desktop application
```

## Development

Install dependencies when setting up the workspace:

```bash
cd rosetta-app
pnpm install
```

Useful validation commands:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Do not run dev servers or production builds unless the current task explicitly calls for UI runtime verification or release packaging.

## Current Focus

The project is moving from single-contributor prototyping to multi-contributor development. The current v1 milestone is a local document translation loop with TXT, Markdown, and text-based PDF import:

```txt
import file -> parse blocks -> segment text -> batch translate -> preview -> export
```

PDF support in v1 means text-based PDF import into the same Rosetta IR pipeline. High-fidelity PDF format restoration is a nice-to-have enhancement if it can be delivered without destabilizing the baseline pipeline. It does not mean OCR, cloud parsing, chat with PDF, or making arbitrary PDF layout restoration a v1 acceptance gate.

## Engineering Records

Major decisions and large changes are tracked in `docs/engineering/`.

- `docs/engineering/decisions/` records architectural decisions.
- `docs/engineering/change-log/` records large project changes.
- `docs/engineering/conventions/` records implementation conventions.
- `docs/engineering/plans/2026-05-12-pdf-v1-support.md` records the current PDF v1 scope and contributor boundary.
