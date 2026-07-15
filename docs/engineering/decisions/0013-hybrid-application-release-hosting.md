# ADR 0013: Hybrid Application Release Hosting

Date: 2026-07-15

## Status

Accepted

## Context

Rosetta's Linux AppImage is about 93 MiB because it carries WebKitGTK,
JavaScriptCore, GTK, ICU, and their runtime dependencies. The Supabase Free
plan has a fixed 50 MB object limit, so neither the AppImage nor its Tauri
updater archive can be stored in the existing private release bucket.

Splitting the updater archive would break Tauri's updater contract. A smaller
Debian package would also replace the accepted AppImage update contract and
would rely on system package management.

## Decision

- Keep `public.app_releases` and the Supabase Edge Functions as the
  authoritative release metadata, platform routing, and publication boundary.
- Allow a release row to point either to the existing private Supabase Storage
  object or to a direct GitHub Release asset URL.
- Restrict external application artifact URLs at the database boundary to
  `https://github.com/LeoLin4258/rosetta/releases/download/...`.
- Store Linux AppImage, updater archive, updater signature, hashes, and sizes
  on a versioned prerelease in the public Rosetta GitHub repository.
- Keep Windows and macOS releases on Supabase Storage until their release
  process has a reason to move. The Edge Functions prefer an external URL when
  present and otherwise preserve the signed-Storage behavior.
- Keep Supabase rows unpublished until platform smoke tests pass. A GitHub
  prerelease asset may be directly reachable before the Supabase row is
  published, but it is not returned by Rosetta's updater or website API.

## Consequences

- Linux releases are no longer blocked by Supabase's 50 MB object limit.
- Tauri continues to receive one signed `.AppImage.tar.gz` URL and does not
  need a client update protocol change.
- GitHub becomes part of the Linux application distribution availability
  path. Mainland China download behavior must be tested separately from the
  local runtime and PDF component mirrors.
- Every Linux application version requires a GitHub tag and prerelease in
  addition to its Supabase metadata row.
