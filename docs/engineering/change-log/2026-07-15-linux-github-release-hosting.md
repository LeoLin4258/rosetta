# Linux GitHub Release Hosting

Date: 2026-07-15

## Summary

Moved Linux application artifacts from Supabase Storage to GitHub Releases
after the AppImage exceeded the Supabase Free plan's fixed 50 MB object limit.
Supabase remains the authoritative metadata and publication service.

## Change

- Added optional `updater_url` and `installer_url` fields to `app_releases`.
- Allowed `storage_path` to be null when an external updater URL is present.
- Restricted external URLs to the public `LeoLin4258/rosetta` release path.
- Updated the updater and latest-download Edge Functions to support either
  external URLs or legacy signed Storage URLs.
- Changed the Linux publisher to create a GitHub prerelease, upload all Linux
  artifacts, and write an unpublished Supabase metadata row.
- Limited the Linux Tauri resource glob to Linux PDFium instead of bundling
  the Windows and macOS PDFium libraries.

## Validation

- Run `bash -n` for the Linux release publisher.
- Run TypeScript checks for both Edge Functions.
- Apply the migration and deploy both Edge Functions.
- Build the signed AppImage from a clean Ubuntu checkout and confirm only the
  Linux PDFium resource is present.
- Verify GitHub asset size and SHA256, then verify the unpublished Supabase row.
- Publish the Linux row only after manual download and in-app updater smoke
  tests pass.
