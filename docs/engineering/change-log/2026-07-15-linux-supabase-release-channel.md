# Linux Supabase Release Channel

Date: 2026-07-15

## Summary

Extended Rosetta's existing private Supabase application release channel to
support Linux x64 AppImage downloads and Tauri in-app updates.

## Change

- Added `linux/x86_64` to the `app_releases` supported-platform constraint.
- Added Linux x64 routing to the updater and latest-download Edge Functions.
- Added `publish-linux-updater.sh` to validate and upload the public AppImage,
  signed updater archive, signature, hashes, sizes, and unpublished metadata.
- Updated the cross-platform and Linux release procedures.

The updater artifact remains private in Supabase Storage and is exposed only
through short-lived signed URLs. No document, translation, prompt, job cache,
model runtime log, or other user data is uploaded by this release flow.

## Publication Boundary

The release row is unpublished by default. The database migration and Edge
Functions must be deployed before the first Linux upload. Website presentation
is a separate stage after the updater and download endpoints pass smoke tests.

## Deployment And Validation

- Applied migration `202607150001_linux_app_releases.sql` to the linked
  Supabase project.
- Deployed `rosetta-update` and `rosetta-latest-download` with Linux x64
  routing.
- Confirmed a Linux updater request returns `204 No Content` while no newer
  published Linux row exists.
- Confirmed the Linux download request reaches the supported-platform path and
  returns `No release available` while no Linux row exists.
- Confirmed the shared updater private key produces a non-empty signature for
  a temporary AppImage updater archive on the Ubuntu release host.

No Linux artifact or release row has been uploaded yet. A clean, versioned
release baseline is required before running the real build and publisher.
