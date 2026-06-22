# 2026-06-22 beta.14 cross-platform release

## Summary

Prepared Rosetta `0.1.0-beta.14` as the first shared Windows x64 and macOS
Apple Silicon release.

## Changes

- Added a Windows release script that requires a clean worktree, matching
  versions, and the existing Tauri updater key. Signed mode also requires a
  trusted Authenticode certificate and RFC 3161 timestamp.
- Added a Windows Supabase publish script. The signed NSIS installer is used
  for both website installation and Tauri updates.
- Generalized Supabase release metadata for `windows/x86_64` and
  `darwin/aarch64`, including installer path, SHA256, and byte size.
- Generalized the updater Edge Function and added a stable dual-platform
  website download endpoint.
- Kept `rosetta-latest-dmg` for compatibility with the existing website during
  deployment.
- Updated the macOS publish script to populate general installer metadata.
- Bumped the app to `0.1.0-beta.14` and added user-facing release notes.
- Added an explicit unsigned Windows Preview mode. It skips Authenticode only
  when requested, while still requiring the shared Tauri updater signature.
- Documented that Windows and macOS packages are built only on their native
  release machines from the same `main` commit.
- Clarified that Supabase is the sole distribution channel for Rosetta
  application installers and updater assets. GitHub Releases are not part of
  the application release flow.

## Privacy

Supabase stores only release installers, updater artifacts, signatures, hashes,
and release metadata. User documents, translations, local jobs, prompts, and
runtime logs remain local.
