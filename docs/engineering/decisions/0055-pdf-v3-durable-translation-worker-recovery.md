# ADR 0055: PDF v3 Durable Translation Worker and Recovery Inventory

Date: 2026-07-18

Status: Accepted

Refines ADR 0034, ADR 0036, ADR 0049 and ADR 0054.

## Context

PDF v3 had independent PageGraph, TranslationPatch and scheduler authorities,
but no orchestrator joined their translation lifecycles. A caller could claim a
translation lease without proving that the scheduler extraction authority still
matched the loadable PageGraph, and there was no production assembly path for
the complete independently validated recovery inventory required by ADR 0049.

Connecting the local provider directly inside the native PDF module would also
couple PDF persistence to one async API and make renderer/provider replacement
harder. The durable boundary only needs a page processor that returns either a
fully resolved TranslationPatch, an explicit source-preservation decision or a
stable typed failure.

## Decision

Add a read-only scheduler translation binding containing the exact source,
PageSet, language, engine, PageGraph schema, TranslationPatch schema and renderer
identities. `PdfV3TranslationWorker` construction verifies this binding against
the PageGraph and TranslationPatch stores before any lease is claimed.

The worker remains provider-neutral. For each page it:

1. claims exactly one scheduler translation lease;
2. loads one PageGraph and verifies its artifact/source authority against the
   claim;
3. invokes a narrow page processor with the PageGraph and immutable translation
   binding;
4. accepts only a fully resolved patch matching target language, schema,
   source atoms and renderer identity, or an explicit stable preservation code;
5. commits the resolved patch to the TranslationPatch store;
6. commits scheduler patch authority only after the durable store commit.

The worker never retains PageGraphs across pages. The page processor owns local
provider and renderer integration and may be replaced without changing store or
scheduler contracts. Provider/model details remain part of TranslationPatch
identity.

PageGraph authority failures and transient patch-store I/O/lock failures are
retryable. Invalid resolved patches and deterministic patch-store conflicts are
non-retryable. Processor failures supply a stable static reason code and an
explicit retryability decision; source or translated text is never used as a
reason code.

Add `validated_recovery_inventory`. It verifies scheduler/store identities,
validates PageGraph artifacts, then validates each candidate patch against its
exact PageGraph while retaining at most one PageGraph and patch at a time. Only
authorities inside the scheduler PageSet enter the inventory. A patch committed
before scheduler state can therefore be promoted after a crash.

TranslationPatch store repair now distinguishes missing/corrupt content from
filesystem I/O failure. Missing or invalid patches remain page-local repairable
state; permission, device and read failures are propagated and cannot silently
delete durable authority.

No Tauri command, RWKV request adapter or frontend protocol is added in this
slice.

## Evidence

Automated tests cover:

- extraction followed by translation patch commit and completed scheduler state;
- exact extraction and patch recovery inventory assembly;
- promotion of a patch committed before scheduler completion state;
- rejection of a patch with a renderer identity outside the scheduler binding;
- explicit page preservation without creating patch authority;
- propagation of patch artifact I/O failures during idempotent commit.

## Consequences

### Positive

- Scheduler completion now has a real PageGraph-to-patch durable path.
- Crash recovery can reuse both extraction and translation work without loading
  a long document into memory.
- Unsupported pages preserve source explicitly rather than generating a blank
  or guessed patch.
- Provider and renderer implementations remain replaceable orchestration inputs.
- Patch repair no longer converts real disk failures into silent page loss.

### Costs

- The current worker processes one page at a time. Provider-side batching across
  pages must preserve this ownership and memory boundary.
- A complete recovery pass validates every selected extraction artifact and
  candidate patch and therefore performs bounded but document-scale I/O.
- The concrete async local-provider planner and TranslationPatch renderer adapter
  remain to be connected above this core.
- Tauri lifecycle commands, user-visible state and real 500/1,000-page translated
  corpus validation remain pending.

## Rejected Alternatives

- Commit scheduler translation state before the resolved patch is durable.
- Let pending renderer drafts enter the patch store.
- Reconstruct patch targets from source text after provider translation.
- Treat a missing or corrupt patch as corruption of the whole language store.
- Put RWKV/Tauri request types inside the native PDF persistence module.
