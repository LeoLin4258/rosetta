# Cross-Platform Release Checklist

Rosetta releases Windows x64, macOS Apple Silicon, and Linux x64 from the same
`main` commit and version.

`app_releases` and the Supabase Edge Functions are the authoritative release
metadata, routing, and publication channel. Windows and macOS artifacts remain
in private Supabase Storage. Linux AppImage artifacts use the public Rosetta
GitHub Release because they exceed the Supabase Free plan object limit.

Platform packages are always produced natively:

- build Windows only on the Windows release machine;
- build macOS only on the Apple Silicon Mac;
- build Linux only on the Linux x64 release machine;
- do not cross-compile or package one platform from the other.

## Prepare

1. Merge the release baseline into `main`.
2. Set the same version in:
   - `rosetta-app/package.json`
   - `rosetta-app/src-tauri/Cargo.toml`
   - `rosetta-app/src-tauri/tauri.conf.json`
3. Add in-app and repository release notes.
4. Run:

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

## Deploy release services

Apply pending Supabase migrations, then deploy:

- `rosetta-update`
- `rosetta-latest-download`
- `rosetta-latest-dmg`

Deploy the website after the Edge Functions. It handles either platform being
temporarily unavailable.

## Produce platform artifacts

- On Windows, follow [windows-release.md](windows-release.md).
- On Apple Silicon, follow [macos-release.md](macos-release.md).
- On Linux x64, follow [linux-release.md](linux-release.md).

Upload both platform releases as unpublished rows first.

## Publish

1. Smoke test the unpublished Windows artifact, then publish only its row.
2. Test the Windows in-app update and website download.
3. Smoke test the unpublished macOS artifacts, then publish only its row.
4. Test the macOS in-app update and website download.
5. Smoke test the unpublished Linux artifacts, then publish only its row.
6. Test the Linux in-app update and website download.
7. Verify updater requests for the current version return `204 No Content`.
8. Confirm the Linux GitHub prerelease tag targets the same release commit.
9. After all platform rows are published and verified, keep the GitHub release
   marked as a prerelease while the application version is a beta.

If one platform fails, set only that release row to unpublished. The other
platform remains available.
