# Rosetta

Rosetta is a local-first desktop app for long-form document translation. It is built with Tauri v2, React, TypeScript, and a local RWKV translation service.

## Project Layout

```txt
docs/                       Product and engineering planning
docs/engineering/           Engineering decisions, change logs, and conventions
rosetta-app/                Tauri desktop application
```

## Development

```bash
cd rosetta-app
corepack pnpm install
corepack pnpm dev
corepack pnpm tauri dev
```

## Current Focus

The project is in infrastructure setup. The first engineering milestone is a TXT and Markdown translation loop:

```txt
import file -> parse blocks -> segment text -> batch translate -> preview -> export
```

## Engineering Records

Major decisions and large changes are tracked in `docs/engineering/`.

- `docs/engineering/decisions/` records architectural decisions.
- `docs/engineering/change-log/` records large project changes.
- `docs/engineering/conventions/` records implementation conventions.
