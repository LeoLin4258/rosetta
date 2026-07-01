# Windows NVIDIA Lightning Runtime Selection Plan

Date: 2026-07-01

## Summary

Windows now has two viable local RWKV runtime paths:

- `llama.cpp Vulkan`: Windows x64 default for Intel, AMD, and NVIDIA GPUs.
- `RWKV Lightning CUDA`: Windows x64 NVIDIA-only runtime, now unblocked by the
  upstream `rwkv_lightning_cuda` `V1.0.2` release.

For NVIDIA Windows users, Rosetta should prefer the Lightning path during
first-run onboarding, while still keeping llama.cpp available as a lower-visual
weight option. In Settings, users should be able to install both runtime
profiles and choose which installed profile is active for translation.

This plan records the exploratory findings and intended UI / runtime direction.
It does not change the product boundary: Rosetta remains a local-first document
translation workbench, not a generic AI assistant or cloud translation service.

## Product Decision

### Onboarding

For Windows machines with an NVIDIA GPU that satisfies the Lightning CUDA
requirement:

- The primary onboarding path installs `RWKV Lightning CUDA` and the matching
  `.pth` RWKV translation model.
- The primary call to action should be visually dominant and should describe
  Lightning as the recommended NVIDIA path.
- A secondary, visually quieter option should let the user choose
  `llama.cpp Vulkan` instead.
- The llama.cpp option should not read as a warning or degraded mode; it is a
  stable fallback and remains the general Windows runtime. It is only visually
  secondary for NVIDIA onboarding because Lightning is the desired NVIDIA
  default.
- If NVIDIA detection fails, or the GPU does not meet the SM75+ requirement,
  onboarding should continue to default to `llama.cpp Vulkan` when the machine
  can use the Windows llama.cpp path.

For non-NVIDIA Windows machines:

- Keep the current Windows llama.cpp Vulkan onboarding path.
- Do not show Lightning as an installable option.

For macOS:

- No change. Apple Silicon continues to use the MLX profile.

### Settings

Settings must support runtime management after onboarding:

- Show the available local runtime profiles for the current platform.
- On Windows NVIDIA, show both `RWKV Lightning CUDA` and `llama.cpp Vulkan`.
- Each profile should expose its own install / repair state.
- Users may install either profile, or both.
- Users may switch the active local runtime profile.
- Switching should stop the currently running managed runtime before starting
  the newly selected runtime.
- Translation dispatch must follow the selected active profile, not merely the
  recommended profile for the platform.

The Settings surface should keep the hierarchy restrained:

- The current active local profile should be clear.
- The recommended NVIDIA Lightning profile should be marked as recommended on
  supported NVIDIA machines.
- The llama.cpp profile should remain visible but visually secondary when
  Lightning is supported.
- Technical labels such as provider id, endpoint, model hash, and paths should
  remain in an expanded technical area, not dominate the normal settings view.

## Current Code Findings

The codebase already contains most of the low-level Lightning support:

- `WINDOWS_AMD64_CUDA` exists as an enabled secondary runtime profile in
  `rosetta-app/src-tauri/src/managed_rwkv/profile.rs`.
- The Lightning profile uses provider id `rwkv-lightning-contents`.
- The Lightning profile uses `/v1/batch/completions`.
- `lifecycle.rs` already knows how to launch Lightning with
  `--model-path`, `--vocab-path`, and `--port`.
- The frontend provider resolver already supports
  `rwkv-lightning-contents`, `rwkv-mobile-batch-chat`, and
  `llama-cpp-chat-completions`.
- The PDF translation shim already accepts `rwkv-lightning-contents` and
  `llama-cpp-chat-completions`.

The main missing layer is not provider implementation. The missing layer is
profile selection:

- Rust managed runtime commands currently resolve a single profile through
  `current_profile()`.
- On Windows, `current_profile()` resolves the first enabled Windows profile,
  currently `windows-amd64-llamacpp-vulkan`.
- Runtime status exposes `candidateProfiles`, but install / start / stop /
  probe still operate on the current profile only.
- The frontend Settings UI lists secondary profiles as technical information,
  but does not let the user select, install, or activate them.
- Onboarding installs the profile returned by `current_profile()` and cannot
  yet choose Lightning for NVIDIA machines.

## Upstream Lightning V1.0.2 Findings

Checked on 2026-07-01:

- Release page:
  `https://github.com/Alic-Li/rwkv_lightning_cuda/releases/tag/V1.0.2`
- Release note:
  `fix shift_state Data races across CUDA blocks`
- Windows asset:
  `RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2.zip`
- GitHub displayed size:
  approximately `462 MB`
- GitHub displayed SHA256:
  `9520f7d4aec4774f29b597a1e66d8f29a012af04345d54f430fcf157d37633e9`
- Expanded assets page:
  `https://github.com/Alic-Li/rwkv_lightning_cuda/releases/expanded_assets/V1.0.2`

This is different from the older Rosetta-pinned Lightning artifact:

- The old Rosetta artifact was derived from upstream `V1.0.0`.
- The old upstream artifact was a `.7z`.
- Rosetta produced a deterministic ZIP and patched the runtime bind address
  from `0.0.0.0` to IPv6 loopback `::1`.
- That patch was necessary because the old runtime did not accept `--host`
  and binding to `0.0.0.0` violates Rosetta's local-only boundary.

Before switching Rosetta to V1.0.2, the new ZIP must be validated on a real
NVIDIA Windows machine. In particular:

- Confirm the archive layout still contains `rwkv_lighting_cuda.exe`,
  `rwkv_vocab_v20230424.txt`, and the required runtime libraries.
- Confirm whether `--host` is supported.
- Confirm whether the server still binds to `0.0.0.0` by default.
- If it still binds to `0.0.0.0`, keep using a Rosetta-pinned patched artifact
  instead of downloading and executing the upstream ZIP directly.
- Confirm `/v1/models` and `/v1/batch/completions` still match the existing
  Rosetta adapter.

## Proposed Technical Shape

### Runtime Profile Selection

Add an explicit selected local runtime profile concept.

Recommended shape:

- Store selected profile id in local app settings, not in job cache.
- Keep `providerPreference` as the high-level choice between `local` and
  `remote-api`.
- Add a separate local runtime selection such as
  `managedRuntimeProfileId`.
- Default profile selection should be platform and hardware aware:
  - macOS Apple Silicon: `macos-arm64-mlx`
  - Windows NVIDIA SM75+: `windows-amd64-rwkv-lightning-cuda`
  - Other Windows x64 machines: `windows-amd64-llamacpp-vulkan`

This keeps the current external API boundary intact:

- `providerPreference: "local"` means "use the selected managed local runtime".
- `providerPreference: "remote-api"` means "use the explicitly configured
  remote API".
- The selected managed runtime then determines the provider adapter:
  - MLX: `rwkv-mobile-batch-chat`
  - Lightning: `rwkv-lightning-contents`
  - llama.cpp: `llama-cpp-chat-completions`

### Rust Command Surface

Managed runtime commands should accept or resolve an explicit profile id.

Candidate command evolution:

- `get_managed_rwkv_runtime_status(profileId?: string)`
- `get_managed_rwkv_install_plan(profileId?: string)`
- `install_managed_rwkv_runtime(options?: { profileId?: string, ... })`
- `start_managed_rwkv_runtime(profileId?: string)`
- `stop_managed_rwkv_runtime(profileId?: string)`
- `probe_managed_rwkv_runtime(profileId?: string)`
- `get_managed_rwkv_runtime_logs_summary(profileId?: string)`

The commands can preserve backward compatibility by falling back to the
default profile when `profileId` is omitted.

Important lifecycle rules:

- Only one managed RWKV sidecar should run at a time.
- Starting a profile while another profile is running should either fail with
  a clear message or stop the current profile first through an explicit switch
  action.
- The UI-facing switch flow should stop the current runtime, refresh status
  for both profiles, then start the selected runtime if the user requested it.
- Stale process cleanup must use the target profile's sidecar signature.

### Status Shape

The existing `candidateProfiles` array is a useful starting point, but Settings
needs per-profile readiness. A single `status.profile` is not enough once
users can install both runtimes.

Recommended direction:

- Keep a selected-profile status for the existing simple path.
- Add per-profile status summaries for platform candidates:
  - profile metadata
  - hardware support result for that profile
  - install plan for that profile
  - installed / missing / ready state
  - running state only if that profile owns the live process

This allows Settings to render:

- Lightning: installed / not installed / running / unsupported
- llama.cpp: installed / not installed / running / available

without forcing the user to select a profile before seeing whether it is
available.

### Artifact Layout

The current layout is already mostly profile-safe:

- Lightning runtime dir:
  `managed-rwkv/runtimes/rwkv-lightning-cuda-sm75-msvc`
- llama.cpp runtime dir:
  `managed-rwkv/runtimes/llama-cpp-vulkan-b9775`
- Lightning model dir:
  `managed-rwkv/models/rwkv7-0.4b-translate-windows-pth`
- llama.cpp model dir:
  `managed-rwkv/models/rwkv7-g1d-0.4b-translate-gguf-q8`

For the V1.0.2 update, prefer a new runtime directory name rather than
silently reusing the old V1.0.0-derived directory:

```txt
managed-rwkv/
  runtimes/
    rwkv-lightning-cuda-sm75-msvc-v1.0.2/
```

This avoids accidentally treating an old installed Lightning runtime as the
fixed V1.0.2 runtime.

The log path may need profile separation. Today `runtime.log` is shared across
profiles. For a multi-runtime Settings UI, profile-specific logs would be
clearer:

```txt
managed-rwkv/
  logs/
    runtime.windows-amd64-rwkv-lightning-cuda.log
    runtime.windows-amd64-llamacpp-vulkan.log
```

This is not strictly required for first implementation, but the UI should at
least show the active profile id next to the log tail.

## UX Flow Details

### NVIDIA Windows Onboarding

Suggested first screen structure:

- Header: local translation engine setup.
- Primary engine card: `RWKV Lightning CUDA`
  - marked recommended for NVIDIA
  - shows detected GPU name and SM capability when available
  - primary install button
- Secondary text/button/card: `Use llama.cpp Vulkan instead`
  - visually quieter
  - mentions broader compatibility
  - no alarming copy
- Existing "skip RWKV" path remains available.

If Lightning install fails:

- Show the concrete backend error.
- Offer retry.
- Offer a secondary fallback action to install llama.cpp Vulkan.
- Do not automatically switch to remote API.

### Settings Runtime Management

Suggested Settings shape:

- Keep the current top-level "local model / remote service" choice.
- Inside "Manage local model", add a local runtime selector.
- On NVIDIA Windows, display two runtime rows/cards:
  - `RWKV Lightning CUDA`
  - `llama.cpp Vulkan`
- Each row should show:
  - recommendation badge when applicable
  - hardware status
  - install state
  - active/running badge
  - install / repair / start / stop / use this runtime actions as appropriate
- Disable switching while a document translation or PDF translation run is
  active.

The selected runtime should drive translation immediately after switch:

- Workspace `buildProvider()` should read the selected managed runtime status.
- PDF translation should receive the selected provider id and endpoint.
- The Settings "current engine" badge should say "local model" at the top
  level and show the specific selected runtime inside the local model panel.

## Validation Plan

### Local AMD Windows Development Machine

This machine cannot validate Lightning inference, but can still validate much
of the implementation:

- TypeScript typecheck.
- Rust compile and unit tests.
- Settings UI renders multiple candidate profiles.
- Non-NVIDIA machine defaults to llama.cpp.
- Lightning is not offered as installable when NVIDIA support is missing.
- Existing llama.cpp install/start/translation path is not regressed.

Relevant commands:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
cargo test managed_rwkv
```

### NVIDIA Windows Machine

Manual validation is required:

1. Download and inspect `RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2.zip`.
2. Start the runtime manually with the `.pth` translation model.
3. Confirm loopback binding behavior with `netstat`.
4. Probe `/v1/models`.
5. Probe `/v1/batch/completions` with two texts.
6. Install Lightning through Rosetta onboarding.
7. Translate a Markdown file through Lightning.
8. Translate a small PDF through Lightning.
9. Install llama.cpp from Settings.
10. Switch active runtime from Lightning to llama.cpp.
11. Translate the same Markdown file through llama.cpp.
12. Switch back to Lightning.
13. Exit Rosetta and confirm no stale sidecar process remains.
14. Clear local data and confirm both runtime profiles are removed.

## Open Questions

- Does upstream V1.0.2 still bind to `0.0.0.0` by default?
- Does upstream V1.0.2 support `--host`?
- Does V1.0.2 still need the runtime DLL allowlist used by the V1.0.0
  staging script?
- Should Rosetta publish a repackaged, SHA256-pinned V1.0.2 ZIP in
  `rosetta-assets`, or can it safely pin and download the upstream ZIP
  directly after validation?
- Should onboarding install only the selected runtime, or offer an advanced
  option to download both Lightning and llama.cpp during first run?
- Should the active local runtime profile be stored only in frontend persisted
  settings, or in a small Rust-side settings file so onboarding and main-window
  runtime commands can share it without relying on frontend state?

## Recommended Implementation Order

1. Validate the upstream V1.0.2 Windows ZIP on the NVIDIA Windows machine.
2. Decide whether Rosetta needs a repackaged / patched V1.0.2 runtime artifact.
3. Update the Lightning profile artifact metadata and runtime directory name.
4. Add selected managed runtime profile state.
5. Make Rust managed runtime commands profile-aware.
6. Add per-profile status summaries.
7. Update onboarding: NVIDIA defaults to Lightning, llama.cpp is secondary.
8. Update Settings: install both, switch active profile.
9. Validate Markdown and PDF translation on both Lightning and llama.cpp.
10. Record the accepted result in an ADR or change-log once implemented.

## Agent Task Breakdown

This section breaks the work into handoff-sized tasks. Each task should be
small enough for one agent to complete in one context window. At the end of
each task, the agent must leave a concrete handoff note in this plan or in
`docs/engineering/change-log/`. Later agents should not start a downstream
task when an upstream task has unresolved blockers that affect runtime
artifacts, command interfaces, or selected-profile behavior.

### Task 1: Lightning V1.0.2 Artifact Validation

Entry criteria:

- Access to the NVIDIA Windows test machine.
- Access to the upstream V1.0.2 Windows ZIP:
  `RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2.zip`.
- Existing `.pth` translation model is available or can be downloaded from the
  current Lightning model profile.

Scope:

- Download and inspect the upstream ZIP.
- Confirm the archive contains `rwkv_lighting_cuda.exe`,
  `rwkv_vocab_v20230424.txt`, and the required runtime libraries.
- Start the runtime manually with the `.pth` translation model.
- Check whether `--host` is supported.
- Check whether the runtime binds only to loopback or still binds to
  `0.0.0.0`.
- Probe `/v1/models`.
- Probe `/v1/batch/completions` with two short source texts.

Output:

- A clear decision: use upstream ZIP directly, or create a Rosetta-pinned
  repackaged / patched artifact.
- Concrete notes for archive layout, bind behavior, endpoint compatibility, and
  any runtime startup errors.

Validation:

- `netstat` or equivalent proves the bind address.
- `/v1/models` returns successfully.
- `/v1/batch/completions` returns two usable translations in the expected
  choice order.

Handoff:

- Record the validation result in this plan before any code changes begin.
- If patching is required, include the required patch behavior and exact reason.

### Task 2: Lightning Profile Artifact Update

Entry criteria:

- Task 1 has decided whether the artifact is upstream-direct or Rosetta-pinned.
- Final artifact filename, size, SHA256, and download URL are known.

Scope:

- Update only the Lightning runtime profile metadata and related artifact
  staging documentation or scripts.
- Prefer a new runtime directory name for V1.0.2, for example
  `rwkv-lightning-cuda-sm75-msvc-v1.0.2`.
- Do not alter llama.cpp behavior.

Output:

- Lightning profile points to the V1.0.2 runtime artifact.
- Profile tests cover the expected filename, directory name, endpoint, provider
  id, and bind host.

Validation:

- Rust profile/layout tests pass.
- Existing llama.cpp profile tests still pass.

Handoff:

- Record the final artifact source.
- Record whether loopback safety is native to upstream V1.0.2 or provided by a
  Rosetta patch.

### Task 3: Profile-Aware Runtime Commands

Entry criteria:

- Lightning V1.0.2 profile metadata is stable.

Scope:

- Make managed RWKV commands accept an optional profile id while preserving
  existing default behavior when omitted.
- Target commands:
  - `get_managed_rwkv_runtime_status`
  - `get_managed_rwkv_install_plan`
  - `install_managed_rwkv_runtime`
  - `start_managed_rwkv_runtime`
  - `stop_managed_rwkv_runtime`
  - `probe_managed_rwkv_runtime`
  - `get_managed_rwkv_runtime_logs_summary`
- Add a profile resolver that rejects unknown, disabled, or platform-mismatched
  profile ids with clear errors.

Output:

- Rust command layer can target either Lightning or llama.cpp explicitly.
- Existing call sites still work through default profile fallback.

Validation:

- Rust tests cover default profile fallback.
- Rust tests cover explicit Lightning and llama.cpp profile resolution on
  Windows x64.
- Unknown profile id returns a clear error.

Handoff:

- Record the final command argument shape and fallback behavior.

### Task 4: Per-Profile Runtime Status

Entry criteria:

- Profile-aware runtime commands exist.

Scope:

- Expose enough status for Settings to render all available local runtime
  profiles without requiring the user to switch first.
- Include profile metadata, hardware support, install plan, installed state,
  and whether the profile owns the running process.
- Keep the existing selected-profile status available for compatibility.

Output:

- Frontend can display Lightning and llama.cpp status independently.
- Non-NVIDIA Windows can see that Lightning is unavailable, not merely
  uninstalled.

Validation:

- TypeScript types match Rust serialization.
- Rust serialization tests or snapshot-style assertions cover representative
  profile status values.

Handoff:

- Include example status payloads or concise field notes for:
  - NVIDIA Windows with Lightning supported.
  - Windows without supported NVIDIA hardware.
  - macOS Apple Silicon.

### Task 5: Persisted Active Local Runtime Selection

Entry criteria:

- Frontend can inspect multiple runtime profiles.

Scope:

- Add app-level persisted selected managed runtime profile state, separate from
  `providerPreference`.
- Keep `providerPreference` as the high-level `local` versus `remote-api`
  choice.
- Make workspace and PDF translation dispatch use the selected local runtime
  profile when `providerPreference === "local"`.

Defaults:

- macOS Apple Silicon: `macos-arm64-mlx`.
- Windows NVIDIA SM75+: `windows-amd64-rwkv-lightning-cuda`.
- Other Windows x64: `windows-amd64-llamacpp-vulkan`.

Output:

- Existing users without a persisted selected profile receive the correct
  platform/hardware default.
- Translation provider selection follows the selected profile:
  - MLX -> `rwkv-mobile-batch-chat`
  - Lightning -> `rwkv-lightning-contents`
  - llama.cpp -> `llama-cpp-chat-completions`

Validation:

- TypeScript provider-selection tests cover all three managed provider ids.
- Existing remote API preference still routes to `rwkv-lightning-contents`
  external API configuration.

Handoff:

- Record the selected-profile storage key and fallback rules.

### Task 6: NVIDIA Onboarding UX

Entry criteria:

- Selected runtime profile can drive install and start commands.

Scope:

- Update onboarding only.
- On supported NVIDIA Windows, make Lightning the primary install path.
- Present llama.cpp Vulkan as a visually quieter secondary option.
- On unsupported NVIDIA or non-NVIDIA Windows, keep llama.cpp as the default
  local runtime path.
- Keep the existing option to skip RWKV setup.

Output:

- NVIDIA Windows onboarding installs Lightning and the `.pth` model by default.
- User can choose llama.cpp during onboarding.
- Lightning install failure offers retry and a secondary fallback to llama.cpp.
- Unsupported machines do not see Lightning as installable.

Validation:

- Manual UI behavior notes for:
  - NVIDIA SM75+ Windows.
  - NVIDIA below SM75 or missing driver.
  - AMD / Intel Windows.
  - macOS Apple Silicon unchanged.

Handoff:

- Add screenshots or concise state notes to this task section or a change-log
  entry.

### Task 7: Settings Runtime Management UX

Entry criteria:

- Onboarding can select and install the intended runtime profile.
- Per-profile status is available to the frontend.

Scope:

- Update Settings local model management.
- Let users install, repair, activate, start, stop, and inspect both Lightning
  and llama.cpp on supported NVIDIA Windows.
- Disable runtime switching while document translation or PDF translation is
  active.
- Keep technical details such as paths, provider ids, endpoints, and hashes in
  an expanded technical area.

Output:

- Settings shows which local runtime is active.
- Settings shows install state for each available profile.
- Users can install both profiles and switch between them.

Validation:

- TypeScript typecheck passes.
- Manual UI flow notes cover:
  - install Lightning only.
  - install llama.cpp only.
  - install both.
  - switch Lightning -> llama.cpp.
  - switch llama.cpp -> Lightning.

Handoff:

- Record the final user-facing labels and switching behavior.

### Task 8: Switching, Cleanup, And Reset Hardening

Entry criteria:

- Settings can switch active runtime profiles.

Scope:

- Ensure only one managed RWKV sidecar runs at a time.
- Make runtime switch stop the currently running profile before starting the
  newly selected profile.
- Make stale sidecar cleanup profile-aware.
- Make app exit cleanup and local data reset handle all installed managed RWKV
  runtime profiles.

Output:

- Runtime switching does not leave old `rwkv_lighting_cuda.exe` or
  `llama-server.exe` processes behind.
- Local data reset removes both runtime installs and both model families.

Validation:

- Windows process checks after:
  - switching runtime profiles.
  - app exit.
  - local data reset.
- Existing macOS exit behavior remains unchanged.

Handoff:

- Record any Windows file-lock behavior or cleanup limitations.

### Task 9: End-To-End Validation And Final Engineering Record

Entry criteria:

- Implementation tasks are complete.

Scope:

- Run local AMD Windows validation.
- Run NVIDIA Windows validation.
- Add a final change-log or ADR if the runtime-selection behavior is accepted
  as durable architecture.

Output:

- Final validation matrix for Lightning and llama.cpp.
- Clear list of unresolved risks or release blockers, if any.

Validation:

- Local AMD Windows:
  - `cd rosetta-app`
  - `pnpm typecheck`
  - `cd src-tauri`
  - `cargo check`
  - `cargo test rosetta_jobs`
  - `cargo test managed_rwkv`
- NVIDIA Windows:
  - clean onboarding installs Lightning by default.
  - Markdown translation works through Lightning.
  - PDF translation works through Lightning.
  - Settings installs llama.cpp.
  - switching Lightning -> llama.cpp changes translation provider.
  - switching llama.cpp -> Lightning changes translation provider back.
  - app exit and reset leave no stale managed runtime processes.

Handoff:

- Link the final change-log or ADR from this plan.
- Note whether the implementation is ready for release packaging.
