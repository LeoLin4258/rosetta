# ADR 0075: PDF v3 Versioned Patch Store and Failed Run Terminal State

Date: 2026-07-20

Status: Accepted

Refines ADR 0034, ADR 0049, ADR 0062, ADR 0067 and ADR 0068.

## Context

TranslationPatch store schema 2 changed page payloads from plain
`.patch.json` to gzip `.patch.json.gz`, but the target-language directory was
still derived only from the language hash. A beta schema-1 store and a
schema-2 process therefore addressed the same directory. Repair rejected the
old shard entries, and new page translations failed during patch persistence.

The scheduler also had page-level `failed` records but no run-level failed
terminal state. Once every requested page failed, completion remained false,
the run stayed `running`, native workers polled forever, owner heartbeat kept
renewing and the manifest generation continued to grow even though no work
was claimable.

## Decision

Patch-store directory identity includes its storage schema. Schema 2 uses:

```text
translations/language-v2-<target-language-sha256>/
```

The prior beta `language-<sha256>` directory is not migrated or read. It is
left untouched until ordinary job deletion removes the job tree. This is an
intentional beta reset of derived translation authority, not a source-document
migration.

Add `failed` to `PdfV3RunState`. A run reconciles to `failed` when at least one
requested page is failed, every requested page is completed, preserved or
failed, and no page is pending, extracting, extracted or translating. This
reconciliation runs after page commits, stale-owner recovery and scheduler
open, so old all-failed `running` manifests converge without a migration pass.

`failed` is terminal for native workers, owner heartbeat, active UI polling,
page-selection locking and export. Export continues to accept only
`completed`. Pause, resume and cancellation do not reinterpret `failed`.

Exact-page retry accepts a `failed` run only when the selected page has
`retryable=true`. The page returns to pending or extracted authority and the
run changes to `running` before the supervisor is attached. A foreign failed
run may first take over an expired owner lease; takeover alone retains
`failed` and does not start a worker.

## Evidence

Automated tests cover:

- schema-2 directory isolation while a legacy language directory exists;
- all-page and mixed completed/preserved/failed terminal convergence;
- scheduler-open normalization of an all-failed running manifest;
- terminal owner-renewal and worker shutdown behavior;
- retryable failed-run restart and non-retryable rejection;
- the complete PDF v3 Rust suite and TypeScript state projection.

## Consequences

- Store-format changes cannot silently share incompatible manifests or page
  filenames with an earlier schema.
- A fully failed run stops disk churn and is truthfully visible to the UI.
- Retrying remains page-exact and does not require creating a second state
  authority.
- Existing beta translation patches are recomputed on demand; source PDFs,
  PageGraphs and user files are not deleted or modified.
- Future patch-store schema changes must change directory identity in the same
  code change and ADR.

## Rejected Alternatives

- Parse and migrate schema-1 patch files in place.
- Delete legacy stores during startup.
- Keep all-failed runs `running` and let workers poll for manual retries.
- Treat any failed page as `completed` or allow export from a failed run.
- Encode schema only inside the manifest while reusing the same directory.
