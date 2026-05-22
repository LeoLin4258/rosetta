# 2026-05-22 macOS Supabase Updater Release Procedure

## Date

2026-05-22

## Scope

Added the macOS updater channel implementation and release procedure, including Tauri config, Supabase schema/function, publish script, and docs.

## Changes

- Replaced the macOS release guide's stale Tauri updater artifact note with the current two-artifact release model.
- Documented that the public DMG is for manual installation and the updater artifact is for in-app updates through Supabase.
- Updated the release script to create the signed updater artifact from the signed and stapled app bundle under `dist/release/`.
- Updated the publish script to publish only the versioned `dist/release` updater artifact and matching signature.
- Added the Supabase update endpoint used by the app.
- Recorded that the first updater release supports `darwin-aarch64`.
- Listed required local secrets for publishing: `SUPABASE_SERVICE_ROLE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PATH` or `TAURI_SIGNING_PRIVATE_KEY`, and optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- Added the release and updater publish commands.
- Documented that the publish script uploads to the private `rosetta-releases` bucket and creates an unpublished `app_releases` row.
- Added the release publishing expectation and a PATCH example for hiding a bad release with `ROSETTA_RELEASE_VERSION`.
- Allowed the updater Edge Function to parse SemVer build metadata while ignoring build metadata for version precedence.

## Privacy Boundary

Supabase release storage is only for updater artifacts and release metadata. User documents, translations, job caches, prompts, and runtime logs must stay local and must not be uploaded to Supabase.

## Bug Fix (2026-05-22)

`release-macos.sh` was capturing the verbose stdout of `pnpm tauri signer sign`
and overwriting the `.sig` file with that text. The Tauri updater requires the
`.sig` to contain only the base64 signature, which Tauri signer writes itself.
The fix discards stdout and verifies the signer-written file instead.

`supabase/config.toml` was added with `verify_jwt = false` for `rosetta-update`
so the flag is persisted and `supabase functions deploy` picks it up automatically.

## Validation

The following passed on 2026-05-22:

- `pnpm typecheck` — passed
- `cargo check` — passed (one pre-existing `PdfError::Encrypted` warning, unrelated)
- `bash -n release-macos.sh` — passed
- `bash -n publish-macos-updater.sh` — passed
- Publish script dry-run with dummy key exits at missing artifact (expected exit 2)
- Apple notarization for app: `e1a7d2cc-7a72-49b0-a765-38bbbd7d461e` — Accepted
- Apple notarization for DMG: `560f5841-f7ab-4dbd-9211-be172917ed94` — Accepted
- Gatekeeper assessment of app and DMG: `accepted / source=Notarized Developer ID`
- Supabase database migration applied via web console
- Edge Function `rosetta-update` deployed via web console with JWT verification disabled
- Endpoint with unsupported platform (`target=windows`) → 204
- Endpoint with older current version (`0.0.0`) → 200 with correct Tauri updater JSON
- Endpoint with current version (`0.1.0-beta.2`) → 204
- Updater artifact and metadata row uploaded and published to Supabase

Pending:

- In-app updater smoke test (install → check → download → relaunch → Gatekeeper). Requires a device
  running a version lower than `0.1.0-beta.2`. Deferred to next release.
