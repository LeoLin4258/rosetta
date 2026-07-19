# ADR 0073: PDF v3 Trusted Native Export Command

Date: 2026-07-19

Status: Accepted

Refines ADR 0060 and ADR 0072.

## Context

The durable PDF v3 export coordinator existed below the Tauri boundary, while
the workbench still had no public way to export a selected native run. The old
command exports a legacy `translated-pages/` artifact and accepts target
language input, so routing v3 through it would reintroduce a second authority.

## Decision

Expose one narrow command:

```text
export_rosetta_pdf_v3_run(jobId, runId, targetPath)
```

The command accepts no source path, fingerprint, PageSet, language, revision,
font, provider, renderer policy, owner or session identity. Native code resolves
all of those values from the job-local source, completed scheduler, immutable
runtime manifest, PageGraph/Patch stores and managed font resolver.

Only a terminal `completed` run with no active leases is exportable. The command
walks the bounded scheduler windows and validates every requested page before
starting the atomic coordinator. Completed pages must match their extraction
and patch authorities; preserved pages remain source content and need no fake
TranslationPatch. If every requested page is preserved, the coordinator uses
the verified atomic source-copy path.

The command runs in blocking workers and returns a typed result containing only
run/language, bounded counts and export byte/object metrics. Errors are mapped
to fixed user-facing categories and never include paths, text, endpoints,
credentials or provider responses. The workbench shows export only for the
selected completed v3 run and never falls back to the legacy PDF artifact.

## Consequences

- v3 export is derived from one immutable native run and remains resumable and
  auditable through existing durable stores.
- Preserved-only runs export without manufacturing translation authority or
  adding bytes to the PDF.
- Export validates all selected-page identities before destination replacement,
  but still keeps rendering page-local inside the coordinator.
- A real complex 500/1,000-page Windows AMD export stress run remains a beta
  gate; the command is intentionally blocking and currently has no progress
  stream.

## Rejected Alternatives

- Reuse `export_rosetta_translated_pdf` or `translated-pages/`.
- Accept frontend-supplied source, language, PageSet or runtime identities.
- Treat preserved pages as empty or synthetic translation patches.
- Build a complete translated PDF document in memory before commit.
