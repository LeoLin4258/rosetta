# ADR 0065: PDF v3 Trusted Translation Component Resolver

Date: 2026-07-19

Status: Accepted

Refines ADR 0057, ADR 0058 and ADR 0061.

## Context

The immutable PDF v3 runtime manifest can bind a run to exact component,
provider, model and font identities, but it does not prove where those values
came from. Allowing the frontend or a run-creation request to supply them would
make the binding self-asserted. The managed RWKV status surface also reports
installation and process state, but did not strictly validate installed
manifests or bind a healthy process to the actual model, executable and font
bytes used by PDF v3.

The native path needs unified translation fonts, but it does not need the
legacy Python worker or doclayout model. Requiring that entire legacy runtime
would retain an unnecessary black-box dependency and prevent its later
removal.

## Decision

Add one native `PdfV3ComponentState` as the trusted resolver for PDF v3
translation components. It derives identity from native-owned profiles and
installed artifacts; callers provide only the target language needed to select
the unified translation-font family.

The managed RWKV binding is accepted only when:

- the registered process is `Ready` and its profile is enabled for the current
  OS and architecture;
- its native install plan is complete;
- model and managed-runtime manifests exactly match the compile-time profile,
  including artifact names, byte counts and release SHA-256 values;
- the profile-specific loopback health probe succeeds;
- profile, PID and base URL are unchanged across the health probe and again
  after artifact hashing completes.

Blocking verification hashes the actual sidecar and model bytes. Direct model
files use a path, byte-count and modification-time digest cache. Extracted
model directories use a deterministic sorted content digest, reject symlinks
and include relative paths and byte counts. The component manifest ID is a
content-derived SHA-256 over the runtime profile/release, actual sidecar and
model identities, provider/model profile identity, PDF asset-pack release and
actual translation-font identities.

The resolver reads only the three bundled BabelDOC font files from the legacy
PDF component pack. It requires the pack release manifest and verifies the
complete bytes of Source Han Sans CN Regular/Bold or Go Noto Kurrent Regular
against compile-time SHA-256 values. Parsed font bytes are cached immutably and
become the live font authority. Python, the legacy worker executable and the
doclayout model are deliberately not readiness requirements.

Provider connection configuration remains process-local. A typed Tauri probe
returns status schema `rosetta-pdf-v3-component-status/1` containing only
component/build, platform, provider/model, runtime-release and font-byte
identity. It does not return paths, base URLs, endpoints, PIDs, tokens,
passwords, raw probe errors or document text.

Trusted PDF v3 run creation must consume this resolver directly. It must not
accept component, provider, model, font or process identity from the frontend.

## Evidence

Automated Windows tests cover:

- exact managed model/runtime manifest matching and tamper rejection;
- file-digest cache invalidation after artifact stamp changes;
- deterministic extracted-directory hashing and content-drift detection;
- component manifest identity drift when runtime content identity changes;
- a strict serialized status field set without machine-local or credential
  fields;
- compilation of the registered Tauri state and probe command.

## Consequences

### Positive

- PDF v3 runtime manifests can be built from native-verified identity instead
  of frontend assertions.
- Runtime switches during expensive verification fail closed.
- Repeated probes avoid re-reading large direct model files when their artifact
  stamp is unchanged.
- PDF v3 no longer treats legacy Python/doclayout readiness as a dependency.
- Unified translation fonts provide stable rendering and bounded font choices
  across a run.

### Costs

- The first probe hashes the complete direct model or extracted model tree.
- Extracted model directories are content-hashed on each resolution until a
  stronger immutable install receipt is introduced.
- The current status probe reports only ready or a sanitized typed failure; a
  richer install/repair/capability event surface remains pending.
- Signed standalone PDF component manifests and native install/update/remove
  operations remain future Phase 7 work.

## Rejected Alternatives

- Accept provider/model/font identity from React or a run-creation payload.
- Trust provider response metadata as model identity.
- Treat a matching install manifest as proof without hashing live artifacts.
- Require the legacy Python worker and doclayout model for native PDF v3.
- Return endpoints, paths, PIDs, credentials or raw errors for diagnostics.
