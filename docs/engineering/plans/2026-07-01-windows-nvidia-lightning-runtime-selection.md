# Windows NVIDIA Lightning Runtime Selection Plan

Date: 2026-07-01

## Summary

Windows now has two viable local RWKV runtime paths:

- `llama.cpp Vulkan`: Windows x64 default for Intel, AMD, and NVIDIA GPUs.
- `RWKV Lightning CUDA`: Windows x64 NVIDIA-only runtime. Rosetta will use a
  Rosetta-owned source-built runtime artifact hosted in
  `LeoLin4258/rosetta-assets`, rather than waiting for an upstream
  `rwkv_lightning_cuda` release that contains the loopback-host fix.

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
  `--model-path`, `--vocab-path`, `--host`, and `--port`.
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

## Rosetta-Owned Runtime Artifact Decision

Decision updated on 2026-07-01:

- Do not wait for a future upstream `rwkv_lightning_cuda` release that merges
  Rosetta's loopback-host PR.
- Build the Windows NVIDIA Lightning runtime from the Rosetta-validated source
  branch ourselves.
- Publish the resulting ZIP to
  `LeoLin4258/rosetta-assets`.
- Keep Rosetta's runtime profile pinned to a Rosetta-controlled URL, byte size,
  and SHA256.
- Use the githubdog URL as the default download URL for mainland China network
  compatibility, with the direct GitHub Release URL as a fallback.

The release artifact must still be a general SM75+ Windows x64 build, not the
local `sm120` development package validated on the RTX 5070 test machine.
The intended CUDA architecture list is:

```txt
75;80;86;87;89;90;100;120
```

The new release-pack script is:

```txt
rosetta-app/src-tauri/scripts/build-rwkv-lightning-cuda-windows-release-zip.ps1
```

It builds `bundle_rwkv_lighting_cuda`, copies the vocab and CUDA runtime DLLs,
removes DLLs outside Rosetta's runtime allowlist, writes
`rosetta-runtime-manifest.json`, and emits the final ZIP size and SHA256.
After uploading the ZIP to `rosetta-assets`, Task 2 should update the checked-in
Lightning profile metadata to the uploaded filename, size, SHA256, release tag,
and a new runtime directory name.

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

1. Download and inspect the hosted Rosetta-owned Lightning V1.0.2 ZIP.
2. Start the runtime manually with the `.pth` translation model and explicit
   loopback `--host`.
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

- What final filename, release tag, size, and SHA256 will the Rosetta-owned
  SM75+ Lightning V1.0.2 ZIP use in `rosetta-assets`?
- Does the Rosetta-owned general SM75+ ZIP validate on both the RTX 5070 test
  machine and at least one older supported NVIDIA architecture before release?
- Should onboarding install only the selected runtime, or offer an advanced
  option to download both Lightning and llama.cpp during first run?
- Should the active local runtime profile be stored only in frontend persisted
  settings, or in a small Rust-side settings file so onboarding and main-window
  runtime commands can share it without relying on frontend state?

## Recommended Implementation Order

1. Build the general SM75+ Rosetta-owned Lightning runtime ZIP from source.
2. Upload the ZIP to `LeoLin4258/rosetta-assets`.
3. Validate the uploaded ZIP on the NVIDIA Windows machine.
4. Update the Lightning profile artifact metadata and runtime directory name.
5. Add selected managed runtime profile state.
6. Make Rust managed runtime commands profile-aware.
7. Add per-profile status summaries.
8. Update onboarding: NVIDIA defaults to Lightning, llama.cpp is secondary.
9. Update Settings: install both, switch active profile.
10. Validate Markdown and PDF translation on both Lightning and llama.cpp.
11. Record the accepted result in an ADR or change-log once implemented.

## Agent Task Breakdown

This section breaks the work into handoff-sized tasks. Each task should be
small enough for one agent to complete in one context window. At the end of
each task, the agent must leave a concrete handoff note in this plan or in
`docs/engineering/change-log/`. Later agents should not start a downstream
task when an upstream task has unresolved blockers that affect runtime
artifacts, command interfaces, or selected-profile behavior.

### Task 1: Lightning V1.0.2 Artifact Validation

Status: partially validated on 2026-07-01 on a Windows NVIDIA machine; do not
use the upstream ZIP directly.

Validation notes:

- Test machine: Windows x64 with `NVIDIA GeForce RTX 5070`, compute capability
  `12.0`, driver `596.21`.
- Upstream file tested:
  `C:\Users\Leo\Downloads\RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2.zip`.
- Size: `484112749` bytes.
- SHA256:
  `9520f7d4aec4774f29b597a1e66d8f29a012af04345d54f430fcf157d37633e9`.
- Archive layout: single top-level `rwkv_lighting_cuda/` directory containing
  `rwkv_lighting_cuda.exe`, `rwkv_vocab_v20230424.txt`, and `lib/` with CUDA
  runtime libraries including `cublas64_13.dll`, `cublasLt64_13.dll`, and
  `cudart64_13.dll`.
- The existing Rosetta `.pth` translation model was available locally at
  `AppData\Local\com.rosetta.desktop\managed-rwkv\models\rwkv7-0.4b-translate-windows-pth\RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth`
  with size `901775740` bytes.
- Starting V1.0.2 with `--model-path`, `--vocab-path`, `--host ::1`, and
  `--port 28901` exited within a few seconds with Windows process exit code
  `-1073740791` and did not serve `/v1/models`. This indicates that the
  current Rosetta-safe loopback-host launch contract is not supported by the
  upstream binary.
- Starting without `--host` launched `rwkv_lighting_cuda.exe` and left a
  running process with `--port 28902`, but validation was stopped before using
  the process as an accepted endpoint because Microsoft Defender raised a
  severe `Trojan:Win32/PowhidSubExec.B` detection against the PowerShell
  command line used for the runtime test. Defender reported
  `DidThreatExecute: False`, `IsActive: False`, and `ActionSuccess: True`.
  The residual `rwkv_lighting_cuda.exe` process and a stale interrupted
  `curl.exe` download process were manually stopped, and the temporary
  validation directory
  `AppData\Local\Temp\rosetta-lightning-v1.0.2-validation` was deleted.

Decision:

- Do not pin or execute the upstream V1.0.2 ZIP directly in Rosetta.
- Task 2 must use a Rosetta-pinned artifact path, and the artifact must preserve
  Rosetta's local-only boundary by supporting an explicit loopback bind or by
  being patched to avoid `0.0.0.0`.
- Before packaging a replacement artifact, repeat endpoint validation from a
  clean staging directory with security tooling satisfied, then verify
  `/v1/models` and `/v1/batch/completions`.
- A local patched artifact now exists for development validation. Downstream
  code tasks can continue against that local artifact, but release packaging
  still needs a final hosted Rosetta-pinned artifact URL before the Lightning
  profile metadata can be updated for general users.

Local patched build notes:

- Source repo:
  `C:\Users\Leo\Documents\GitHub\rwkv_lightning_cuda`.
- Local upstream fix branch has been submitted as a PR by the user.
- Local build output:
  `C:\Users\Leo\Documents\GitHub\rwkv_lightning_cuda\build_local_sm120\bundle\rwkv_lighting_cuda`.
- Built for this test machine only with `CMAKE_CUDA_ARCHITECTURES=120`; do not
  treat this package as a general `SM75+` release artifact.
- Built executable SHA256:
  `da404edf2b4b7568a9da96907a698c504059a7afbe791db2e47704382a21674d`.
- Minimal Rosetta runtime ZIP:
  `C:\Users\Leo\Documents\GitHub\rwkv_lightning_cuda\build_local_sm120\RWKV_lightning_CUDA_sm120_Win_MSVC_V1.0.2_rosetta-loopback-local.zip`.
- Minimal ZIP size: `398159251` bytes.
- Minimal ZIP SHA256:
  `f598a748ad88581a23fe6dfa2d02155cf32a1cbac1bcae94596b5f85d80e22bf`.
- Minimal DLL set contains 17 DLLs under `lib/`, including Drogon/Trantor,
  OpenSSL, sqlite, brotli, zlib, VC runtime, and CUDA 13.3 runtime libraries.
- Validation from the minimal package succeeded with `--host 127.0.0.1`:
  `/v1/models` returned the expected model id, netstat showed only
  `127.0.0.1:<port>`, and `/v1/batch/completions` returned ordered
  translations for two prompts.
- Validation from the app-data runtime directory succeeded with `--host ::1`:
  `/v1/models` returned 200 at `http://[::1]:<port>/v1/models`,
  `http://127.0.0.1:<port>/v1/models` did not respond, and netstat showed
  only `[::1]:<port>`.
- Windows Defender recent detections remained limited to the earlier upstream
  ZIP test commands under the temp validation directory; no new Defender alert
  was observed from the repo-built local package.
- The local patched runtime was installed into this machine's Rosetta app data
  directory for follow-up testing:
  `C:\Users\Leo\AppData\Local\com.rosetta.desktop\managed-rwkv\runtimes\rwkv-lightning-cuda-sm75-msvc`.

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

Status: completed on 2026-07-01 for the initial hosted artifact and checked-in
profile metadata update.

Handoff notes:

- Do not wait for the upstream PR to appear in an upstream release.
- Build a Rosetta-owned general SM75+ package from source and publish it to
  `LeoLin4258/rosetta-assets`.
- Use
  `rosetta-app/src-tauri/scripts/build-rwkv-lightning-cuda-windows-release-zip.ps1`
  for the release package. The local `sm120` ZIP remains development-only.
- Rosetta now supports a hash-pinned local runtime-pack override through
  `runtimePackPath`, `runtimePackSha256`, and `runtimePackSizeBytes`, plus the
  matching environment variables `ROSETTA_RWKV_RUNTIME_PACK`,
  `ROSETTA_RWKV_RUNTIME_PACK_SHA256`, and
  `ROSETTA_RWKV_RUNTIME_PACK_SIZE_BYTES`.
- The Lightning launch contract has been updated to pass `--host` explicitly.
  Because `WINDOWS_AMD64_CUDA.bind_host` is `[::1]` for URL formatting, the
  lifecycle command builder strips IPv6 URL brackets and passes `::1` to the
  runtime CLI.
- The checked-in Lightning profile metadata now points to the hosted
  Rosetta-owned V1.0.2 artifact.
- The runtime directory name is now
  `rwkv-lightning-cuda-sm75-msvc-v1.0.2`, so older V1.0.0-derived installs are
  not mistaken for the V1.0.2 source-built runtime.
- The general SM75+ source-built ZIP was prepared locally and uploaded to
  `LeoLin4258/rosetta-assets`:
  - Source commit: `fbac9e4 Add --host option and default bind to 127.0.0.1`
  - CUDA architectures: `75;80;86;87;89;90;100;120`
  - Release tag:
    `rwkv-lightning-cuda-windows-x64-v2026.07.01.1`
  - Filename:
    `RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2_rosetta-loopback.zip`
  - Local path:
    `rosetta-app/dist/rwkv-runtime/RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2_rosetta-loopback.zip`
  - Size: `404846501` bytes
  - SHA256:
    `54ed31261492cd89d800852ee369f745ad75a9690cfcdcceada4eacfc58aeca2`
  - Server executable SHA256:
    `945c384cb85fa6a93b6d480036367798a47aaccff7e00dca635b9e5cfcc277d1`
  - Default githubdog URL:
    `https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/rwkv-lightning-cuda-windows-x64-v2026.07.01.1/RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2_rosetta-loopback.zip`
  - Direct GitHub fallback URL:
    `https://github.com/LeoLin4258/rosetta-assets/releases/download/rwkv-lightning-cuda-windows-x64-v2026.07.01.1/RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2_rosetta-loopback.zip`
  - ZIP structure and SHA256 were rechecked after packaging.
  - A temp-directory extraction probe loaded the executable and reached the
    expected missing-model error, confirming the packaged runtime DLL set is
    sufficient for process startup.
  - Both the default githubdog URL and direct GitHub fallback returned HTTP 200
    with `Content-Length: 404846501`.

Entry criteria:

- Task 1 has rejected the upstream ZIP as a direct Rosetta dependency.
- Final artifact filename, size, SHA256, and download URL are known.

Scope:

- Update only the Lightning runtime profile metadata and related artifact
  staging documentation or scripts.
- Prefer a new runtime directory name for V1.0.2, for example
  `rwkv-lightning-cuda-sm75-msvc-v1.0.2`.
- Do not alter llama.cpp behavior.

Output:

- Lightning profile points to the Rosetta-owned V1.0.2 runtime artifact in
  `rosetta-assets`.
- Profile tests cover the expected filename, directory name, endpoint, provider
  id, and bind host.

Validation:

- Rust profile/layout tests pass.
- Existing llama.cpp profile tests still pass.

Handoff:

- Record the final artifact source.
- Record that loopback safety is provided by Rosetta's source-built runtime
  branch until the upstream project ships an equivalent release.

### Task 3: Profile-Aware Runtime Commands

Status: initial command surface completed on 2026-07-01.

Handoff notes:

- `get_managed_rwkv_runtime_status(profileId?)`,
  `get_managed_rwkv_install_plan(profileId?)`,
  `start_managed_rwkv_runtime(profileId?)`,
  `stop_managed_rwkv_runtime(profileId?)`,
  `probe_managed_rwkv_runtime(profileId?)`, and
  `get_managed_rwkv_runtime_logs_summary(profileId?)` now accept an optional
  profile id while preserving the default-profile fallback when omitted.
- `install_managed_rwkv_runtime(options?)` now accepts
  `options.profileId`, keeping all install parameters inside the existing
  options object.
- The Rust command resolver rejects unknown, disabled, and platform-mismatched
  profile ids with explicit errors.
- The TypeScript wrapper functions in `src/lib/rwkvRuntime.ts` expose the same
  optional `profileId` argument.
- This task intentionally does not yet expose per-profile status summaries or
  active runtime selection. Those remain Task 4 and Task 5.

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

Current blocker / handoff note:

- As of 2026-07-01, the dev onboarding "install local translation engine"
  button still installs the default Windows profile, which is
  `windows-amd64-llamacpp-vulkan`.
- The screenshot showing approximately `478 MB` is therefore the llama.cpp GGUF
  model path (`501498208` bytes shown as MiB), not Lightning.
- Do not use the current onboarding button as evidence that Lightning default
  onboarding works.
- The Rust command layer and TypeScript wrappers already accept explicit
  `profileId`, so the next implementation step is frontend state / flow wiring,
  not new artifact work.
- Until Task 5 / Task 6 are implemented, Lightning can only be tested through an
  explicit `profileId: "windows-amd64-rwkv-lightning-cuda"` path or a temporary
  diagnostic invocation.

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

- Managed runtime commands can be called with an explicit profile id.
- Lightning V1.0.2 profile metadata is hosted and stable.
- Task 5 selected-profile persistence is preferred before this task, but this
  task may proceed first if onboarding keeps its selected profile in local
  component state and passes it explicitly through install / status / start /
  probe.

Scope:

- Update onboarding only.
- On supported NVIDIA Windows, make Lightning the primary install path.
- Present llama.cpp Vulkan as a visually quieter secondary option.
- On unsupported NVIDIA or non-NVIDIA Windows, keep llama.cpp as the default
  local runtime path.
- Keep the existing option to skip RWKV setup.
- Make every managed-runtime call in the onboarding flow use the selected
  profile id:
  - `getManagedRwkvRuntimeStatus(profileId)`
  - `installManagedRwkvRuntime({ profileId, ... })`
  - `startManagedRwkvRuntime(profileId)`
  - `probeManagedRwkvRuntime(profileId)`
  - `getManagedRwkvRuntimeLogsSummary(profileId)`
- Do not rely on the default `current_profile()` fallback for NVIDIA
  onboarding, because on Windows that fallback currently resolves to
  `windows-amd64-llamacpp-vulkan`.
- The user-facing first screen must make it clear which runtime will be
  installed. The primary NVIDIA path should name `RWKV Lightning CUDA`, and the
  secondary fallback should name `llama.cpp Vulkan`.

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
