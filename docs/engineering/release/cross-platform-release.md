# Windows and macOS Release Checklist

Rosetta releases Windows x64 and macOS Apple Silicon from the same `main`
commit and version.

Supabase Storage and `app_releases` are the only distribution channel for
Rosetta application installers, updater artifacts, signatures, hashes, and
release metadata. Do not upload application release assets to GitHub Releases.
GitHub may optionally carry a source tag, but a tag is not required for
installer distribution or App-internal updates.

Platform packages are always produced natively:

- build Windows only on the Windows release machine;
- build macOS only on the Apple Silicon Mac;
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

Upload both platform releases as unpublished rows first.

## Publish

1. Smoke test the unpublished Windows artifact, then publish only its row.
2. Test the Windows in-app update and website download.
3. Smoke test the unpublished macOS artifacts, then publish only its row.
4. Test the macOS in-app update and website download.
5. Verify updater requests for the current version return `204 No Content`.
6. After both platform rows are published and verified, optionally create a
   source-code git tag. Do not create a GitHub Release or upload application
   installers to GitHub.

If one platform fails, set only that release row to unpublished. The other
platform remains available.
