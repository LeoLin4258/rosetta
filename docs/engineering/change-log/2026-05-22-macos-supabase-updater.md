# 2026-05-22 macOS Supabase Updater Release Procedure

## Date

2026-05-22

## Scope

Document the macOS release procedure for publishing Rosetta's Tauri updater artifact through Supabase, without changing release scripts, Supabase functions, migrations, or application code.

## Changes

- Replaced the macOS release guide's stale Tauri updater artifact note with the current two-artifact release model.
- Documented that the public DMG is for manual installation and the updater artifact is for in-app updates through Supabase.
- Added the Supabase update endpoint used by the app.
- Recorded that the first updater release supports `darwin-aarch64`.
- Listed required local secrets for publishing: `SUPABASE_SERVICE_ROLE_KEY`, `TAURI_SIGNING_PRIVATE_KEY`, and optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- Added the release and updater publish commands.
- Documented that the publish script uploads to the private `rosetta-releases` bucket and creates an unpublished `app_releases` row.
- Added the release publishing expectation and a PATCH example for hiding a bad release with `ROSETTA_RELEASE_VERSION`.

## Privacy Boundary

Supabase release storage is only for updater artifacts and release metadata. User documents, translations, job caches, prompts, and runtime logs must stay local and must not be uploaded to Supabase.

## Validation

- `sed -n '140,240p' docs/engineering/release/macos-release.md`
- `sed -n '1,220p' docs/engineering/change-log/2026-05-22-macos-supabase-updater.md`
- Placeholder scan across the macOS release guide and this change log.
