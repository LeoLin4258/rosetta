# 2026-06-22 beta.14 cross-platform release

## Summary

Prepared Rosetta `0.1.0-beta.14` as the first shared Windows x64 and macOS
Apple Silicon release.

## Changes

- Added a Windows release script that requires a clean worktree, matching
  versions, a trusted Authenticode certificate, an RFC 3161 timestamp, and the
  existing Tauri updater key.
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

## Privacy

Supabase stores only release installers, updater artifacts, signatures, hashes,
and release metadata. User documents, translations, local jobs, prompts, and
runtime logs remain local.
