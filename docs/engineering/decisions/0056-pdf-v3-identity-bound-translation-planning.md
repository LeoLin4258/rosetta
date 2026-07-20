# ADR 0056: PDF v3 Identity-Bound Translation Planning

Date: 2026-07-18

Status: Superseded by ADR 0076

Refines ADR 0034, ADR 0036, ADR 0055 and the PDF v3 data-model conventions.

## Context

The durable translation worker accepts a provider-neutral page processor, but
there was no canonical path from a reconciled PageGraph to provider input or
from provider output to a pending TranslationPatch. Reusing source strings as
the correspondence key would make duplicate text ambiguous and would recreate
the substring-search failure mode that PDF v3 explicitly rejects.

Protected citations, numbers, URLs and formulas also need to survive provider
translation exactly. A missing, duplicated or reordered protected value must
not be accepted and guessed back into a PDF content stream.

## Decision

Add an in-memory, page-owned `TranslationPagePlan` contract. The plan is not a
durable authority and is rebuilt from the exact PageGraph when a page is
processed. It contains a bounded ordered list of translation units plus typed
source-preservation diagnostics.

The initial planner is deliberately aligned with the current renderer:

- one unit completely owns the provenance-bearing atoms of one source text
  object;
- all unit atoms must share one style and one exact text-show identity;
- unmapped, mixed-style, mixed-text-show and otherwise unsupported objects are
  retained as explicit preserved regions;
- a protected span crossing source-object ownership preserves the affected
  objects instead of producing a partial patch;
- an object containing only protected content does not create a no-op unit.

Each `unitId` is a SHA-256 identity over the plan contract version, source page
hash, page number and ordered PageGraph atom IDs. Source text equality is never
used for identity. Duplicate source strings on the same page therefore remain
independent units.

Protected spans are replaced in provider input with deterministic `{vN}`
tokens that do not occur in the source unit. Existing source text containing
that token syntax is conservatively preserved until the provider adapter owns
a disjoint escaping contract. Reassembly requires every planned token exactly
once and in source order, rejects any unknown token, restores the exact source
value and records its UTF-8 byte placement for TranslationPatch validation.

Provider results are keyed by `unitId`. Result array order has no authority;
unknown, duplicate, missing or extra results are rejected. The reassembler
rebuilds the expected plan from the current PageGraph before accepting output,
then emits a pending `TranslationPatchDraft` that still must pass the existing
PageGraph-aware patch builder and renderer resolution.

The plan is bounded to 100,000 units, 1 MiB source text per unit and 16 MiB
accepted source text per page. It never spans pages and is released with the
active PageGraph. A plan with no safe units cannot produce an empty patch; its
caller must commit the page as explicitly preserved.

No Tauri command, provider request type, persistent schema or frontend protocol
is added in this slice.

## Evidence

Automated tests cover:

- exact citation token restoration and UTF-8 patch placement;
- stable distinct unit identity for duplicate source strings;
- out-of-order provider results with canonical patch order;
- missing, duplicate and unknown result rejection;
- missing, duplicate, reordered and unknown protected token rejection;
- mixed-style and cross-object protected-span preservation;
- stale or tampered plan rejection;
- per-unit byte limits and source placeholder collision preservation;
- rejection of an empty patch when no safe unit exists.

## Consequences

### Positive

- Provider output can only target PageGraph atoms through deterministic IDs.
- Protected content is restored and located without Unicode substring guesses.
- Planner diagnostics expose why individual source objects remain original.
- Translation planning remains page-local and bounded for long documents.
- TranslationPatch remains the only durable translated-text authority.

### Costs

- Unit granularity is currently source text object, not paragraph or semantic
  group. This favors safe visual replacement over translation context.
- PageGraph protected-span production is still pending; the planner only
  consumes and validates spans already present in the graph.
- The concrete async local-provider bridge and renderer-owning page processor
  remain separate work above this contract.
- Existing source `{vN}` syntax causes conservative preservation until a
  provider-level escaping contract is implemented.

## Rejected Alternatives

- Match provider results back by source text or result order.
- Persist translation plans or duplicate their source text in patch storage.
- Allow protected spans to be partially covered by separate patch entries.
- Accept missing or reordered protected tokens and search for equivalent text.
- Create an empty successful patch for a page with no safe translation units.
