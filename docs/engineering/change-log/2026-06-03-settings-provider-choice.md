# 2026-06-03 Settings Provider Choice

Added an explicit Settings choice for the translation backend.

## Scope

- Added `RwkvConnectionConfig.providerPreference`.
- Settings now lets users choose between the Rosetta-managed local model and the remote API.
- The backend choice is made by clicking the local/remote cards directly, not through a separate tab control.
- Backend switching is guarded against repeated rapid clicks and disabled while a translation run is active.
- Settings status badges now use semantic colors for ready, needs action, processing, and error states.
- Workspace and translation preview dispatch respect the selected backend.
- Settings copy now uses the terms "本地模型" and "远程 API" consistently.

## Compatibility

Older persisted settings do not include `providerPreference`. New installs default to `local`, while existing persisted RWKV settings without the new field migrate to `remote-api` so a previously configured API remains the active backend after upgrade.

## Validation

- `pnpm typecheck`
