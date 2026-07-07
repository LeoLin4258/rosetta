# 2026-07-07 Managed RWKV Windows Loopback Diagnostics and Repair

## Context

Windows beta.18 users reported onboarding failures after Rosetta downloaded the
managed llama.cpp Vulkan runtime and RWKV GGUF model. Runtime logs showed
llama-server successfully loaded the model and printed a listener on
`http://127.0.0.1:<port>`, but Rosetta self-check and manual `curl` timed out.

Manual diagnostics proved the failure was below HTTP:

- `Get-NetTCPConnection` showed the runtime PID listening on `127.0.0.1:<port>`.
- `curl --noproxy "*"` and `Test-NetConnection 127.0.0.1 -Port <port>` timed out.
- A handwritten PowerShell `TcpListener` on `127.0.0.1:58100` also could not be
  connected to from the same machine.
- `ping 127.0.0.1` returned general failure / status `11050`.

The app-level failure was therefore a downstream symptom of broken Windows
loopback TCP, not a model, Vulkan, llama.cpp, or Rosetta provider bug.

## Changes

### 1. Add managed runtime connectivity diagnostics

Added `diagnose_managed_rwkv_connectivity`, a Tauri command that performs a
runtime-independent loopback check:

- Bind a temporary `TcpListener` to `127.0.0.1:0`, then connect back to it from
  the same Rosetta process with a short timeout.
- Repeat for `::1` when available.
- Classify the currently selected managed runtime profile by its real
  `bind_host`, so llama.cpp Vulkan checks IPv4 and the CUDA profile checks IPv6.
- On Windows, collect read-only snapshots of `Get-NetConnectionProfile` and
  `Get-NetFirewallProfile` with short timeouts.

The command returns structured diagnostics for the frontend: loopback probe
results, network profile category, firewall profile state, suspected issue, a
message, recommended actions, and an optional copyable PowerShell hint for
switching a Public network to Private.

### 2. Add one-click repair for the common Public-network case

Added `repair_managed_rwkv_connectivity`. When loopback is broken and Windows
reports a Public network profile, Rosetta can set the current network profile to
Private and then re-run the same loopback diagnostic.

The command first tries the repair directly. If Windows requires elevation, it
launches a short PowerShell repair script through UAC (`Start-Process -Verb
RunAs`) and waits for it to complete. The frontend does not ask users to paste
PowerShell by default; the manual command is still available under technical
details for support cases.

This is intentionally narrow: it only changes Public network profiles to
Private. It does not disable firewalls, uninstall security software, remove WFP
filters, or touch third-party drivers.

### 3. Surface diagnosis and repair in onboarding and settings

Onboarding now automatically runs the connectivity diagnostic when managed RWKV
start/probe fails, then shows a compact panel beside the copyable error bundle.
Settings shows the same panel when the local runtime is failed or has a recent
managed-runtime error.

The default UI is written for normal users: explain that Windows blocked the
local connection and offer one primary action, "修复连接并重试". Technical
details such as `127.0.0.1`, `::1`, network profile category, and the manual
PowerShell command are collapsed by default.

Onboarding retries the RWKV setup after a successful repair. Settings retries
starting the failed runtime profile after the repair diagnostic passes.

Manual support command example:

```powershell
Set-NetConnectionProfile -InterfaceAlias "WLAN" -NetworkCategory Private
```

### 4. Fix misleading health-path copy

The lifecycle probe messages no longer hardcode `/health`; they use each
profile's actual `health_path`. The Windows llama.cpp profiles health-check
`/v1/models`, while translation goes to `/completion` or `/v1/batch/completions`.

The profile summary exposed to TypeScript now includes `healthPath`, and
onboarding diagnostics distinguish `healthPath` from `batchChatPath`.

PowerShell JSON can serialize `NetworkCategory` as numeric enum values. Rosetta
now maps `0/1/2` to `Public/Private/DomainAuthenticated` before rendering, so
normal users do not see opaque output such as `WLAN: 0`.

### 5. Keep the repair prompt in the user's task path

The Settings page now renders the connectivity repair panel inside the failed
runtime profile card, directly below the "启动失败" status summary. This keeps
the flow readable: the user sees the failed runtime, then the specific fix for
that failure. The generic top-level managed-runtime error banner was removed to
avoid duplicating the same failure above the actionable card.

The workspace translation toolbar now shows an inline warning when local runtime
translation is selected but the managed runtime is failed or unavailable. The
disabled translate button is accompanied by a visible "打开设置修复" action that
navigates to `settings?panel=local-runtime`, expands the local runtime section,
and scrolls to the repair panel.

The connectivity panel no longer uses a green icon before diagnostics have run.
Unknown or failing connectivity states use the warning treatment; only a passing
diagnostic uses the success treatment.

### 6. Append loopback hint to runtime start/probe errors

When a sidecar health wait or manual probe fails, Rust runs the local loopback
check. If the selected profile's loopback target cannot connect, the returned
error includes a clear note that the problem is likely Windows loopback TCP
filtering, firewall, WFP, or security software rather than RWKV runtime startup.

## Verification

- `pnpm typecheck`
- `cargo fmt --check`
- `cargo check`
- `cargo test managed_rwkv::diagnostics`

## Boundary

This change does not identify the exact WFP callout or uninstall third-party
drivers. It gives Rosetta users a reliable first in-app split:

- If the loopback diagnostic fails, stay at Windows networking / firewall /
  security software / safe-mode diagnostics.
- If the loopback diagnostic passes, continue with normal runtime logs, process
  state, endpoint paths, and port ownership.
