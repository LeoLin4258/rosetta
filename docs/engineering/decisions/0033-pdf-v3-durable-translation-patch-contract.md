# ADR 0033: PDF v3 Durable TranslationPatch Contract

Date: 2026-07-17

Status: Accepted

Amends ADR 0015.

## Context

PDF v3 needs a durable translation authority that is independent from rendered
page PDFs. The existing placeholder `TranslationPatch` type did not identify a
translation revision or provider, did not bind entries to exact source atoms,
and represented fit state as an optional scalar. It could not safely detect a
stale PageGraph, distinguish untranslated work from a renderer preservation
decision, or prove that protected citations and symbols survived translation.

Keeping full source text in every patch would also duplicate PageGraph data and
increase sensitive text exposure. Keeping full translated page PDFs as the
authority would repeat fonts and resources for every page, recreating the disk
growth PDF v3 is intended to remove.

## Decision

`TranslationPatch.schemaVersion = 1` is the canonical, page-addressed durable
translation contract.

A patch records:

- deterministic `patchId`, 1-based page number and source page hash;
- target language, positive translation revision, provider ID and model ID;
- renderer version and source-ordered translation entries;
- deterministic entry IDs, ordered atom IDs and a SHA-256 of each complete
  source `PageAtom`;
- translated UTF-8 text and one validated PageGraph style ID;
- protected span ID, kind, exact value and fixed-width `u32` translated UTF-8
  byte range;
- renderer state as `pending`, `fitted` with an explicit strategy and scale, or
  `preserved` with a stable reason code.

The builder canonicalizes atoms and entries by PageGraph order. One atom may
belong to at most one patch entry. Unmapped atoms, mixed or absent styles,
partial protected spans, missing protected span placements, overlapping byte
ranges and placements whose bytes differ from the exact protected value are
typed failures.

PageGraph protected spans must themselves be canonical before a patch can be
built: IDs are unique, atom IDs exist and are unique in strictly increasing
source order, and concatenating those atoms equals `exactText`. Protected
regions must therefore be split into explicit PageGraph atoms rather than
recovered later with substring heuristics.

`entryId` is derived from the source page hash and canonical atom IDs.
`patchId` is the SHA-256 identity of the complete canonical patch with its own
ID field cleared. Translation metadata and renderer decisions are part of this
identity. Decode rebuilds the canonical translation data against the current
PageGraph, checks source atom hashes and validates the final patch identity.

Compact JSON is the initial encoding. Build, encode and decode all enforce a
16 MiB page-patch limit; one entry additionally has an 8 MiB translated-text
limit and one patch has at most 100,000 entries. A patch stores no source text
except the exact values that must be protected in translated output.

This ADR fixes the data contract only. Atomic file replacement, revisioned
paths, patch manifests, compression, bounded render cache and streaming export
remain separate Phase 4 work.

## Consequences

### Positive

- A patch can be rejected deterministically when its page, atoms, ordering,
  protected values, renderer state or identity is stale.
- Translation retries and provider/model changes produce explicit revisions
  instead of silently replacing ambiguous page state.
- Renderer preservation is durable and distinguishable from work that has not
  yet been attempted.
- Patch storage scales primarily with translated text and references, without
  embedding page PDF resources or ordinary source text.
- UTF-8 byte ranges can be checked exactly without language-dependent character
  indexing.

### Costs

- Translation planning must split protected regions into dedicated ordered
  PageGraph atoms.
- Any material contract change requires a new schema version; beta v1/v2 PDF
  artifacts are not migrated.
- Renderer updates must recalculate `patchId` before the patch is committed.
- JSON is not the final compression decision and may later be wrapped in a
  versioned compressed container without changing the logical schema.

## Rejected Alternatives

- Store complete translated page PDFs as the durable translation authority.
- Persist only translated strings and recover their source targets by text
  search.
- Keep optional `fitScale` without a typed pending/fitted/preserved state.
- Store all source atom text in each patch.
- Accept partially covered citations and attempt substring repair during
  rendering.
