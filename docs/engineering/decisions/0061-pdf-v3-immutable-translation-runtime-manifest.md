# ADR 0061: PDF v3 Immutable Translation Runtime Manifest

Date: 2026-07-18

Status: Accepted

Refines ADR 0023, ADR 0057, ADR 0058 and ADR 0060.

## Context

The PDF v3 page processor and final exporter already consumed explicit
provider/model identities, renderer policy and unified font assets. Those
values were still assembled independently by each caller. A resumed run could
therefore use a different model, font file or fit policy without changing its
scheduler identity, producing internally valid but cross-page inconsistent
patches.

Persisting provider connection configuration would not solve the identity
problem and would put endpoints or credentials into job state. Resolving fonts
from operating-system directories would also make recovery and export depend
on mutable machine state.

## Decision

Add one immutable `runtime-manifest.json` beside each PDF v3 scheduler run. The
manifest is capped at 64 KiB and binds:

- source fingerprint, source page count and canonical exact PageSet;
- source/target language, engine, PageGraph, TranslationPatch and renderer
  versions;
- positive translation revision and exact renderer fit policy;
- component ID/version/manifest ID/build SHA-256, platform and architecture;
- provider ID plus model ID and model SHA-256;
- Regular and optional Bold font asset ID, weight, face index, byte count and
  complete-file SHA-256.

The manifest ID is the SHA-256 of its canonical compact JSON with the ID field
cleared. Creation writes a unique synced temporary file and exposes it with one
rename. Recommitting the exact manifest is idempotent; any different manifest
at the same run path is a conflict and cannot replace the original binding.
Reads are size-bounded, reject unknown JSON fields and recompute every
identity.

The manifest deliberately stores no provider endpoint, token, body password,
font path, source text or translated text. Live connection configuration stays
process-local. A live runtime binding must match the manifest's provider kind,
current platform/architecture and exact font descriptors before it can create
a page-processor configuration. Processor configuration fields are private and
the production constructor now derives them from that validated binding.

The component manager remains responsible for verifying the installed model
artifact and proving that the launched provider uses the selected component.
Provider output cannot assert or change model identity. The runtime manifest is
the job's expected identity, not a substitute for component health checks.

## Evidence

Automated Windows tests cover:

- deterministic build, atomic commit, bounded reload and idempotent recommit;
- manifest JSON containing no Windows font path;
- exact provider and Regular/Bold font binding;
- immutable translation-revision conflict rejection;
- live font-byte drift rejection before processor construction;
- all existing renderer-owning processor success, preservation, cancellation
  and failure paths after configuration is derived from the bound runtime.

## Consequences

### Positive

- A long or resumed run has one inspectable provider/model/font/render identity.
- Page processing and final export can share the same immutable font assets.
- Font paths and provider credentials do not enter durable job state.
- Runtime drift fails before provider I/O or PDF mutation.
- The manifest is constant-size regardless of document page count.

### Costs

- A new native PDF v3 run must commit its runtime manifest before translation.
- The future Tauri lifecycle layer must obtain component identities from the
  verified component manager and bind the live provider before constructing a
  worker or exporter.
- Actual model-process identity still depends on component launch and health
  verification; it cannot be proven by the provider response contract.
- Existing isolated beta PDF v3 artifacts are discarded rather than migrated.

## Rejected Alternatives

- Let each page processor caller supply unrelated identity strings and fonts.
- Persist provider URLs, tokens or passwords in the run manifest.
- Infer model identity from provider output or response metadata.
- Persist platform-specific font paths and reopen them during export.
- Allow a resumed run to replace its manifest in place.
