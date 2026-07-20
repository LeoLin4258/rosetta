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

Blocking verification hashes the actual sidecar bytes. Model content is
verified once by the managed installer before its atomic final rename; the
installer then writes a profile-bound manifest containing the exact filename,
byte count and SHA-256. Normal runtime resolution validates that manifest plus
the live model file kind and byte count, and reuses the installed SHA-256
without reading the complete model. Install, update and repair remain the only
full model-digest boundaries. The component manifest ID is a content-derived
SHA-256 over the runtime profile/release, actual sidecar identity, trusted
installed model identity, provider/model profile identity, PDF asset-pack
release and actual translation-font identities.

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

After the selected managed translation runtime becomes `Ready`, the app shell
probes the resolver once in the background for the active target-language font
family. This pre-populates the process-local artifact digest and immutable font
caches without blocking window startup. A target-language or runtime-profile
change gets a distinct warmup key. Failure remains non-authoritative and is
retried by actual run creation; the frontend does not persist a false ready
state across app processes.

The app no longer starts the legacy Python/doclayout worker during process
startup. Native PDF v3 does not consume that worker, and prewarming it adds an
unrelated process, memory use and several seconds of background work. Legacy
commands may still start it explicitly while those commands remain in the
codebase.

## Evidence

Automated Windows tests cover:

- exact managed model/runtime manifest matching and tamper rejection;
- installed-model receipt reuse with bounded file-kind and byte-count checks;
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
- App startup and repeated probes never re-read a correctly installed model;
  model verification cost stays at the install/update/repair boundary.
- PDF v3 no longer treats legacy Python/doclayout readiness as a dependency.
- Unified translation fonts provide stable rendering and bounded font choices
  across a run.

### Costs

- A same-size model mutation outside Rosetta is not detected at ordinary
  startup. Explicit repair/reinstall is the integrity revalidation path.
- The current status probe reports only ready or a sanitized typed failure; a
  richer install/repair/capability event surface remains pending.
- Signed standalone PDF component manifests and native install/update/remove
  operations remain future Phase 7 work.

## Rejected Alternatives

- Accept provider/model/font identity from React or a run-creation payload.
- Trust provider response metadata as model identity.
- Re-hash the complete model at every App process start.
- Require the legacy Python worker and doclayout model for native PDF v3.
- Return endpoints, paths, PIDs, credentials or raw errors for diagnostics.
