# macOS Apple Silicon Managed RWKV Runtime Plan

## Summary

Rosetta v1 should prioritize a one-click local RWKV runtime for macOS Apple Silicon. The goal is a fully local, command-line-free setup path: users install Rosetta, click to install or prepare the RWKV translation runtime, wait for a readiness check, then translate long documents through Rosetta's existing job, segment, preview, and export workflow.

The target runtime architecture is:

```txt
Rosetta UI
  -> Translation runner
  -> RWKV provider adapter
  -> Rosetta-managed local sidecar process
  -> rwkv-mobile MLX runtime/API on 127.0.0.1
  -> local RWKV translate model
```

This plan supersedes the earlier Windows/NVIDIA-first runtime assumption for v1. Windows NVIDIA via `rwkv_lightning_libtorch` remains a strong later provider, but the first managed runtime milestone is macOS arm64 / Apple Silicon.

## Product Boundary

The macOS runtime work must preserve Rosetta's narrow product scope:

- local document translation
- privacy-sensitive files
- long text and document structure preservation
- batch translation through a local model API
- no chat UI
- no cloud default
- no login, sync, telemetry, or collaboration features
- no document Q&A, summarization, rewriting, or generic assistant behavior

The runtime exists only to power Rosetta's document translation workflow. It should not expose RWKV App-like chat, voice, vision, prompt playground, or general model-management features in Rosetta.

## Upstream Findings

### rwkv-mobile

Repository: <https://github.com/MollySophia/rwkv-mobile>

Relevant findings as of 2026-05-12:

- `rwkv-mobile` is a multi-backend RWKV runtime intended for mobile, desktop, C/C++ integration, Flutter integration, and an AI00/OpenAI-compatible API server.
- The README lists MLX as the Apple Silicon backend direction.
- `README_API.md` documents local HTTP endpoints that map well to Rosetta:
  - `GET /health`
  - `GET /v1/batch/supported_batch_sizes`
  - `POST /v1/chat/completions`
  - `POST /v1/batch/chat`
  - `POST /v1/batch/completions`
- `README_translate.md` documents a translation-oriented flow using chat roles plus `/v1/batch/chat`, and states that a batch should share one translation direction.
- The API responses include per-choice indexes, which Rosetta needs for stable segment order recovery.
- The repository contains an example `rwkv_server.cpp` and CMake server option paths, so a local HTTP sidecar is a plausible integration shape.
- The current GitHub workflow visible in the repository builds macOS library artifacts with server disabled, so Rosetta must verify whether a ready-to-distribute macOS arm64 server binary exists, or whether Rosetta must build/package one from source.

Implication for Rosetta:

`rwkv-mobile` should be treated as the primary candidate for the v1 macOS managed runtime, but the deliverable must be a stable macOS arm64 sidecar binary or an equivalent packaged server artifact. Rosetta should not depend on users building it locally.

### RWKV_APP

Repository: <https://github.com/RWKV-APP/RWKV_APP>

Relevant findings as of 2026-05-12:

- RWKV_APP is a Flutter/Dart local-first RWKV application that targets iOS, Android, Windows, macOS, and Linux.
- Its README describes local model loading, local inference, and desktop local API capabilities.
- Its translation store code contains batch translation concepts such as batch-mode selection, asynchronous batch generation calls, batch response buffer reads, and per-slot response handling.
- The app demonstrates that Apple/mobile-class devices can present RWKV local translation as a user-facing product feature.

Implication for Rosetta:

RWKV_APP is useful as a proof point and engineering reference for local model loading and batch translation UX, but Rosetta should not embed or depend on the Flutter app. Rosetta should reuse the runtime/API idea, not the RWKV_APP product surface.

## Target User Experience

The intended v1 user flow is:

```txt
Open Rosetta
  -> Rosetta detects macOS Apple Silicon
  -> User sees "Install local RWKV translation model"
  -> User clicks Install
  -> Rosetta downloads or imports runtime/model artifacts
  -> Rosetta verifies checksums and architecture
  -> Rosetta starts the local sidecar process
  -> Rosetta probes /health and a tiny batch translation
  -> UI shows "Local RWKV is ready"
  -> User imports a document
  -> User clicks Translate
```

The user must not need to:

- open Terminal
- install Python, Conda, Homebrew, CUDA, or developer tools
- clone a GitHub repository
- run a server command
- manually choose a port
- manually type a localhost URL
- manually edit JSON configuration

An advanced manual artifact import path may exist for internal testing, but it must still be UI-driven.

## v1 Scope

Supported in v1:

- macOS Apple Silicon / arm64.
- Rosetta-managed local runtime sidecar.
- Runtime bound to `127.0.0.1` only.
- One recommended RWKV translation model.
- One recommended runtime provider: `rwkv-mobile` with MLX, assuming upstream validation.
- Batch translation through a provider adapter.
- English -> Chinese as the primary verified language direction.
- Existing Rosetta TXT/Markdown/PDF/DOCX pipeline integration as available.
- Pause, stop, retry, and per-segment failure handling through the existing translation workflow.

Not supported in v1:

- Intel Mac as a first-class one-click target.
- Windows one-click runtime.
- Windows AMD/Intel one-click runtime.
- Chat UI or general RWKV playground.
- User-selectable arbitrary model zoo.
- Remote cloud default.
- macOS App Store distribution constraints.
- OCR, summarization, rewriting, Q&A, or generic assistant features.

## Architecture

### Layering

Runtime work should keep three boundaries separate:

```txt
Document pipeline
  owns import, IR, segmenting, translation files, preview, export

Translation provider adapter
  owns request/response shape for a backend API

Managed runtime provider
  owns installing, verifying, starting, stopping, and probing local sidecars
```

The document pipeline must not know whether the backend is MLX, CUDA, rwkv-mobile, RWKV Lightning, or a remote user-configured endpoint. It should only know that it can submit source segment texts to a translation provider and receive ordered translation results.

### Provider Strategy

Introduce provider concepts before hard-coding macOS runtime behavior into the current connector.

Recommended provider IDs:

```txt
rwkv-mobile-batch-chat
rwkv-lightning-contents
custom-rwkv-api
```

The macOS v1 default should be:

```txt
providerId: rwkv-mobile-batch-chat
runtimeKind: managed-local
platform: macos-arm64
backend: mlx
primaryEndpoint: /v1/batch/chat
healthEndpoint: /health
batchSizeEndpoint: /v1/batch/supported_batch_sizes
```

The current `/v1/chat/completions` `contents[]` connector should become one provider path rather than the permanent shape of all RWKV translation.

### Runtime Process

The managed runtime should be a sidecar process launched by Tauri/Rust, not a library loaded into the Rosetta frontend process.

Reasons:

- isolates model/runtime crashes from the main app
- keeps the React UI independent from native inference details
- allows runtime upgrades without redesigning the document pipeline
- makes Windows NVIDIA and macOS MLX providers symmetrical later
- allows health checks, logs, process monitoring, and controlled shutdown

The sidecar must:

- bind only to `127.0.0.1`
- use an app-generated local token or password when supported
- listen on a Rosetta-managed port
- write logs without source document text or translated text
- expose readiness through `/health` or equivalent
- support batch translation through `/v1/batch/chat` or a validated equivalent

## App Data Layout

Use a versioned runtime area under the Tauri app data directory. A possible layout:

```txt
AppData/Rosetta/
  runtimes/
    rwkv-mobile-macos-arm64/
      manifest.json
      bin/
        rwkv_server
      logs/
        runtime.log
  models/
    rwkv-translate-macos/
      manifest.json
      model-file...
      vocab-or-tokenizer...
  runtime-state/
    active-runtime.json
    local-token
```

Manifest files should record:

- artifact ID
- provider ID
- platform
- architecture
- version
- source URL or import source
- checksum
- installed timestamp
- compatibility notes

Do not store source text, translated text, prompts, segments, document structure, or user documents in runtime logs or manifests.

## Artifact Strategy

Rosetta should support two installation paths:

1. **Preferred product path: managed download**
   - Rosetta downloads runtime and model artifacts from approved URLs.
   - Artifacts have pinned versions and checksums.
   - Rosetta verifies checksums before install.
   - Rosetta does not execute unverified downloaded binaries.

2. **Internal/advanced path: local artifact import**
   - User selects a runtime/model artifact through a Tauri dialog.
   - Rosetta scans and validates architecture, manifest, checksum if known, and required files.
   - This path is useful while upstream packaging is still settling.

Before public release, confirm:

- runtime license allows bundling or managed download
- model license allows bundling or managed download
- artifact URLs are stable enough for pinned manifests
- macOS binaries can be signed/notarized or packaged in a way compatible with Rosetta distribution

## Tauri/Rust Runtime Module

The runtime module should expose narrow commands. Suggested command surface:

```txt
get_managed_rwkv_runtime_status
get_managed_rwkv_install_plan
prepare_managed_rwkv_install
scan_managed_rwkv_artifacts
install_managed_rwkv_runtime
start_managed_rwkv_runtime
stop_managed_rwkv_runtime
probe_managed_rwkv_runtime
get_managed_rwkv_runtime_logs_summary
```

The command surface should not expose arbitrary process execution, arbitrary filesystem reads, or broad runtime configuration from the frontend.

Runtime state should include:

```txt
unsupported
not-installed
installing
installed
starting
ready
failed
stopped
```

Compatibility checks should include:

- OS is macOS
- architecture is arm64
- runtime binary exists and is executable
- model artifact exists
- runtime/model manifest versions are compatible
- selected provider supports batch translation

## Translation Adapter

The macOS default adapter should target `rwkv-mobile` batch chat.

Conceptual request mapping:

```txt
Rosetta batch:
  [{ segmentId, sourceText, sourceLang, targetLang }]

rwkv-mobile /v1/batch/chat:
  conversations:
    [
      { messages: [{ role: "user", content: sourceText }] },
      ...
    ]
```

Response handling requirements:

- use response choice index to restore request order
- reject missing indexes
- reject empty content for translatable segments
- preserve already completed or edited translations when retrying unrelated failed segments
- never silently write a mismatched translation into a segment
- return per-segment failure information when the backend allows partial results

### Language Direction

`rwkv-mobile` translation docs describe chat roles such as source language role and target language role. Rosetta should prefer request-level language control if the backend supports it, because global role state can conflict with concurrent translation jobs.

If roles are global:

- v1 should run only one active translation direction per runtime process.
- different language-direction jobs must be queued or use separate runtime processes/ports.
- the UI should avoid implying unrestricted concurrent multi-language translation.

If roles can be request-scoped:

- include language direction in each batch request.
- keep Rosetta's existing per-file language settings.

This is an upstream validation item before implementation is considered stable.

### Batch Policy

The adapter should query `/v1/batch/supported_batch_sizes` when available. Rosetta's scheduler should combine that with its own segment length buckets.

Recommended policy:

```txt
small segments:
  use the largest supported batch size that remains stable

medium segments:
  use a moderate batch size

large segments:
  use smaller batches

huge segments:
  split before translation
```

Batch membership must keep one language direction per batch. It should also avoid mixing extremely short and extremely long segments when that harms throughput or timeout behavior.

## UI Flow

### Settings

Settings should present local runtime as the primary v1 path on macOS Apple Silicon:

```txt
Local RWKV
  Status: Not installed / Installing / Ready / Failed
  Runtime: rwkv-mobile MLX
  Model: RWKV Translate
  Action: Install / Start / Stop / Repair / Open logs summary
```

External API configuration should remain available as an advanced or fallback option. Remote API usage must clearly say that document text will be sent to that configured address.

### First Run

On first launch on macOS arm64, if no local runtime is installed:

- show a restrained setup panel in Settings or an onboarding state
- explain that translation will run locally after installation
- show estimated download size when known
- require an explicit user action before downloading large artifacts
- show progress for download, verification, install, and readiness probe

Do not present this as a landing page or generic AI assistant setup.

### Jobs Page

The jobs workflow should only need a readiness gate:

- if local runtime is ready, translate normally
- if not installed, guide to install
- if installed but stopped, offer start
- if failed, show a concise diagnostic and repair path
- if unsupported platform, offer external API configuration

Do not add model management noise to the main document workbench.

## Lifecycle and Cancellation

The runtime module should:

- start the sidecar on demand or when the user enables local runtime
- avoid starting model inference before the user begins translation or explicitly starts runtime
- stop the sidecar on user request
- cleanly stop the sidecar when the app exits where possible
- detect crashed sidecars and mark runtime failed/stopped
- avoid orphaned processes after failed launches

Translation cancellation should cancel Rosetta's current request and, if the backend supports it, signal the backend to interrupt generation. If backend cancellation only drops the HTTP connection, Rosetta must still restore current `translating` segments to retryable states according to existing data-model conventions.

## Privacy and Security Requirements

Runtime security requirements:

- bind only to `127.0.0.1`, not `0.0.0.0`
- generate a per-install or per-run local token/password where supported
- never log source segment text or translated output
- avoid putting document text into error messages, raw response previews, or diagnostics
- keep remote API opt-in and visibly distinct from local runtime
- do not broaden Tauri filesystem permissions for runtime convenience
- do not allow the frontend to supply arbitrary executable paths
- validate installed artifacts before executing them

If runtime logs are shown in the UI, show a redacted summary rather than raw logs by default.

## Implementation Phases

### Phase 0: Upstream Contract Validation

Goal: prove the exact macOS runtime contract before wiring product UI.

Tasks:

- confirm macOS arm64 runtime artifact shape with RWKV engineers
- confirm MLX backend startup command
- confirm model format and tokenizer/vocab requirements
- confirm `/v1/batch/chat` stability on Apple Silicon
- confirm supported batch size endpoint behavior
- confirm language-direction handling
- confirm cancellation behavior
- confirm license/distribution constraints

Exit criteria:

- a local macOS arm64 runtime can be launched without Terminal by a script or Tauri prototype
- `/health` succeeds
- a tiny English -> Chinese batch succeeds
- response indexes map correctly to input items
- no sensitive text appears in runtime logs by default

### Phase 1: Provider Adapter Split

Goal: prevent macOS provider behavior from being hard-coded into the existing RWKV connector.

Tasks:

- define provider IDs and capability shape
- keep the current `contents[]` connector as `rwkv-lightning-contents`
- add `rwkv-mobile-batch-chat` request/response mapping
- add provider-specific probe logic
- keep translation runner consuming a provider-neutral result shape

Exit criteria:

- existing external API flow still works
- rwkv-mobile API can be probed and used when manually configured
- batch response order and missing-index failures are tested

### Phase 2: Managed Runtime Skeleton

Goal: restore managed runtime work with macOS-first constraints.

Tasks:

- create macOS arm64 compatibility detection
- create runtime/model manifest readers
- create install-plan status for local artifacts
- create start/stop/probe commands for one fixed runtime provider
- bind sidecar to localhost
- manage token/password and port selection

Exit criteria:

- Rosetta can detect unsupported platforms
- Rosetta can detect installed macOS artifacts
- Rosetta can start and stop the local sidecar
- Rosetta can probe readiness without translating user documents

### Phase 3: UI Installation Flow

Goal: provide the no-command-line user experience.

Tasks:

- add Settings local runtime panel
- add install/import action
- show progress and errors
- show ready/failed/stopped state
- connect Jobs page readiness gate
- keep external API fallback available

Exit criteria:

- a non-technical user can prepare local RWKV from the app UI
- failed install/start states are understandable
- the main workbench remains document-focused

### Phase 4: Translation Integration

Goal: route real Rosetta jobs through the managed local provider.

Tasks:

- use runtime-provided `baseUrl` and provider ID
- query supported batch sizes
- apply segment length buckets
- run translation through `/v1/batch/chat`
- support stop/retry semantics
- persist translations through existing translation-file workflow

Exit criteria:

- a long Markdown/TXT document translates locally on an M-series Mac
- progress updates incrementally
- failures can be retried
- stop restores in-flight work to retryable states
- preview and export require no runtime-specific special cases

### Phase 5: Packaging and Release Hardening

Goal: make the local runtime distributable outside a developer machine.

Tasks:

- sign/notarize runtime sidecar if bundled
- verify executable permissions after install
- verify Gatekeeper behavior
- test clean install on a fresh macOS user account
- test app update without deleting models
- test runtime upgrade/migration
- test corrupted artifact repair

Exit criteria:

- Rosetta can be installed on a clean Apple Silicon Mac and run local translation without Terminal
- runtime artifacts are verified
- crash and failure paths do not strand the app in a broken state

## Acceptance Criteria

v1 local runtime acceptance:

- On a clean macOS Apple Silicon machine, a user can install/prepare RWKV from Rosetta UI without Terminal.
- Rosetta starts the local RWKV sidecar automatically or through a visible app button.
- The sidecar binds only to localhost.
- Rosetta can probe runtime health and batch translation readiness.
- Rosetta can translate a long document through the local runtime.
- Segment order is correct after batch translation.
- Stop, retry, and failure states remain recoverable.
- Source text and translated text are not written into runtime logs or diagnostics.
- Unsupported platforms show a clear fallback path rather than broken controls.

## Risks

### No Stable macOS Server Artifact

Risk: upstream provides a library but not a packaged macOS arm64 HTTP server.

Mitigation:

- build/package `rwkv_server` from `rwkv-mobile` as a Rosetta-controlled artifact
- keep manual artifact import during internal testing
- do not promise public one-click runtime until artifact packaging is verified

### Global Language Roles

Risk: translation direction is controlled by global server state.

Mitigation:

- v1 allows one active translation direction per runtime process
- queue conflicting jobs
- prefer request-scoped language parameters if upstream supports them

### Batch Instability on MLX

Risk: MLX backend works for single prompts but has unstable or limited batch behavior.

Mitigation:

- probe supported batch sizes
- maintain conservative defaults
- split by segment length buckets
- fallback to smaller batch sizes on repeated failures

### macOS Distribution Friction

Risk: sidecar execution is blocked by signing, notarization, quarantine, or permissions.

Mitigation:

- treat packaging as a first-class milestone
- test on clean machines
- avoid Mac App Store constraints for v1 unless explicitly required

### Runtime Logs Leak Document Text

Risk: upstream runtime logs prompts or outputs.

Mitigation:

- configure quiet logging if available
- redact or suppress logs by default
- verify logs during Phase 0
- avoid exposing raw logs in UI

## Questions for RWKV Engineers

Before implementation should be considered committed, confirm:

- Is `rwkv-mobile` the recommended macOS Apple Silicon runtime for Rosetta v1?
- Is there a stable macOS arm64 server binary, or should Rosetta build/package one?
- What is the exact MLX startup command and required arguments?
- What model format should Rosetta use on macOS?
- What tokenizer/vocab files are required?
- Is `/v1/batch/chat` stable on macOS MLX?
- Does `/v1/batch/supported_batch_sizes` reflect the actual loaded model/backend?
- What batch sizes are recommended for M1, M2, M3, and M4 machines?
- What is the recommended maximum source segment length?
- Is language direction request-scoped or global?
- Can `force_language` / `force_lang` replace global role setup for translation?
- Does the runtime support cancelling an in-progress batch?
- Does the runtime log prompts or generated text by default?
- Can Rosetta redistribute or one-click download the runtime and model?
- What versions should Rosetta pin for the first release?

## Documentation Follow-Up

If this plan is accepted and implementation begins:

- add a new ADR for the macOS-first managed runtime decision
- update `0002-pause-managed-rwkv-runtime.md` with a supersession note rather than deleting its historical context
- update data-model conventions only if runtime/provider state becomes durable job data
- add a change-log entry when the runtime provider implementation lands

The likely ADR title:

```txt
0003-macos-first-managed-rwkv-runtime.md
```

The ADR should record:

- v1 managed runtime target is macOS Apple Silicon
- `rwkv-mobile` MLX sidecar is the preferred runtime candidate
- Windows NVIDIA `rwkv_lightning_libtorch` is deferred
- translation pipeline uses provider adapters
- managed runtime and translation provider boundaries remain separate

## Validation

For implementation phases, use the normal validation set when relevant:

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Do not run dev servers or production builds unless explicitly requested for UI verification or release packaging.
