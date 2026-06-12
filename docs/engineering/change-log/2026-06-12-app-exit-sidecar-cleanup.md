# 2026-06-12 App Exit Sidecar Cleanup

## Context

Activity Monitor showed `rwkv-server` processes remaining after Rosetta quit.
The managed RWKV runtime already had a stop path, and local data reset already
used it, but app quit did not route through that cleanup.

## Changes

- App `ExitRequested` now prevents exit once, shuts down session sidecars, then
  resumes exit with the original exit code.
- Managed RWKV exit cleanup reuses the existing lifecycle stop path, including
  registered child kill and stale sidecar cleanup by managed-runtime signature.
- The warm pdf2zh worker is also stopped during app exit so session-owned child
  processes do not outlive Rosetta.
- Added a regression test that verifies a registered RWKV child process is
  killed by `stop_sidecar`.

## Validation

- `cargo test managed_rwkv::lifecycle::tests::stop_sidecar_kills_registered_child`
