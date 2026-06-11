# 2026-06-11 Local Data Reset

## Summary

Settings now includes a destructive local reset action for removing Rosetta-owned data from the current machine.

## Scope

- Stops the managed local RWKV runtime before deleting files.
- Cancels in-progress managed RWKV, PDF component, and PDF translation operations when possible.
- Deletes Rosetta job history/cache under the app data directory.
- Deletes the managed local model directory.
- Deletes the managed PDF sidecar directory.
- Deletes the Rust-side onboarding state file.
- Clears the frontend persisted settings key after a successful reset.

## Boundaries

The reset action does not delete the Rosetta application bundle, user original files, or files manually exported outside Rosetta-owned data directories.
