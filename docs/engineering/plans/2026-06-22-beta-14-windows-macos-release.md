# Rosetta 0.1.0-beta.14 Windows + macOS Release Plan

Date: 2026-06-22

## Summary

- Use `codex/windows-runtime-rewrite` as the new release baseline.
- Archive the current `main` at commit
  `5aefef91be1e371e30747dc6ed1a116778b06d29`, then replace `main` with
  `5c74e696794972402d41dd8a39132856b58e1ede` using
  `--force-with-lease`.
- Release Windows x64 and macOS Apple Silicon together as
  `0.1.0-beta.14`.
- Distribute installers through the official website and continue using
  Supabase as the source for installer artifacts, updater artifacts, and
  release metadata.
- Keep the existing manual update flow in Settings. Do not add startup update
  prompts or automatic installation in this release.

## Implementation Changes

### Release baseline and version

- Create the remote archive branch
  `archive/main-before-windows-rewrite-2026-06-22` at `5aefef9`.
- Replace remote `main` with `5c74e69` using `--force-with-lease`, then create
  the beta.14 release work from the new `main`.
- Remove the repository-root temporary diagnostic file `tmp-log.md` and
  verify that no logs, installers, private keys, credentials, or
  machine-specific release files are committed.
- Update these version sources to `0.1.0-beta.14`:
  - `rosetta-app/package.json`
  - `rosetta-app/src-tauri/Cargo.toml`
  - `rosetta-app/src-tauri/tauri.conf.json`
- Add user-facing beta.14 release notes and an engineering change-log entry.
- Update ADR 0006 to record that the Windows NVIDIA installation, translation,
  PDF, and clean first-run paths have passed real-device validation.

### Windows release pipeline

- Add a local `release-windows.ps1` script that:
  - requires a clean worktree and matching version values;
  - checks for the Tauri updater private key and Windows Authenticode
    configuration;
  - builds the NSIS installer using a code-signing certificate accessible
    through the Windows certificate store;
  - uses SHA-256 file signing and an RFC 3161 timestamp;
  - writes the versioned installer, Tauri `.sig`, SHA256 digest, and file size
    into `dist/release/`;
  - verifies that `Get-AuthenticodeSignature` reports `Valid` and that the
    signer subject matches the expected publisher.
- Add a local `publish-windows-updater.ps1` script that:
  - uploads the signed NSIS executable;
  - registers the same executable as both the website installer and Windows
    updater artifact;
  - reads the contents of the matching `.sig`;
  - creates or updates an unpublished `windows/x86_64` release row;
  - prints the commands for publishing, hiding, and rolling back the release.
- Inject certificate identifiers and signing secrets through environment
  variables. Do not store certificate thumbprints, private keys, passwords,
  or service-role credentials in the repository.
- Treat a trusted Authenticode signature as a public-release requirement. The
  release script must stop if signing or signature verification fails.

### Supabase release service

- Extend `app_releases` to support:
  - `darwin/aarch64`
  - `windows/x86_64`
- Add general installer metadata:
  - `installer_storage_path`
  - `installer_sha256`
  - `installer_size_bytes`
- Backfill the current macOS `dmg_storage_path` into
  `installer_storage_path`. Keep `dmg_storage_path` temporarily for backward
  compatibility with the existing website function.
- Extract SemVer parsing and ordering into a shared Edge Function module so
  updater and website download endpoints choose releases consistently.
- Extend `rosetta-update` to query the requested Tauri `target` and `arch`
  instead of accepting only `darwin/aarch64`.
- Add `rosetta-latest-download`:
  - JSON mode returns version, publication date, target, architecture,
    filename, size, SHA256, and a download entry point.
  - `download=1` generates a fresh short-lived signed storage URL and returns
    an HTTP redirect.
- Keep `rosetta-latest-dmg` working during the transition so the currently
  deployed website and macOS download links do not break.
- Update the macOS publish script so future macOS releases populate the new
  general installer fields.
- New and updated release rows remain `is_published=false` until platform
  smoke tests pass.

### Official website

- Load the latest published Windows and macOS release metadata separately.
- Do not cache short-lived signed storage URLs in Next.js. Download buttons
  must use the stable redirect endpoint, which creates a fresh signed URL per
  request.
- Change the header download action to navigate to the download section.
- Provide explicit actions in the hero/download surfaces:
  - `Download for Windows`
  - `Download for macOS`
- Do not automatically start a download based on the user agent. The user
  explicitly chooses the platform.
- Display the supported environment:
  - Windows 10/11 x64. The managed local model requires NVIDIA SM75 or newer;
    unsupported machines may explicitly configure an external translation
    API.
  - macOS Apple Silicon.
- If one platform has no published release, disable only that platform's
  action and keep the other platform available.
- Remove or update all existing `macOS only` copy and page metadata while
  preserving the current website's visual language. This release does not
  include a full website redesign.

## Public Interfaces

### Tauri updater platforms

- `windows-x86_64`
- `darwin-aarch64`

### Website download API

```txt
GET /functions/v1/rosetta-latest-download?target=windows&arch=x86_64
GET /functions/v1/rosetta-latest-download?target=darwin&arch=aarch64
```

Adding `download=1` returns a redirect to a newly generated signed download
URL.

### Compatibility

- The existing `rosetta-update` URL in `tauri.conf.json` remains unchanged.
  Installed beta.13 clients can therefore discover beta.14 without a config
  migration.
- The website and updater only expose rows with `is_published=true`.
- The Windows NSIS installer is also the Tauri Windows updater artifact.

## Release Sequence and Rollback

1. Deploy the database migration and Edge Functions without publishing
   beta.14.
2. Deploy the dual-platform website. It must tolerate either platform being
   unavailable.
3. On the Windows release machine, build and sign beta.14, verify a clean
   install and uninstall, then upload the release as unpublished.
4. On an Apple Silicon Mac, build, sign, notarize, staple, and verify beta.14,
   then upload the release as unpublished.
5. Publish the Windows row and immediately test beta.13 to beta.14 through the
   in-app updater. Mark the row unpublished again if the test fails.
6. Publish the macOS row and perform the equivalent updater test.
7. Verify both website download actions, then create the
   `v0.1.0-beta.14` tag and GitHub Release record.
8. To roll back a platform, set its beta.14 row to
   `is_published=false`. Beta.13 becomes the latest published website release
   again. Installed beta.14 clients are not downgraded; publish beta.15 for
   forward fixes.

## Test Plan

### Application validation

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

### Website validation

```powershell
pnpm typecheck
pnpm lint
pnpm build
```

- Verify desktop and mobile layouts.
- Verify visible keyboard focus and meaningful disabled states.
- Verify the page when Windows is unavailable, macOS is unavailable, and both
  are available.

### Windows release validation

- Authenticode signature, timestamp, Tauri `.sig`, and SHA256 are valid.
- A clean Windows machine can install and uninstall the signed NSIS package.
- First-run onboarding installs the managed runtime, translation model, and
  PDF component.
- Text translation, PDF translation, preview, and export pass.
- An installed beta.13 client discovers beta.14, downloads it, performs the
  passive installation, relaunches, and reports beta.14.
- Jobs, settings, the managed model, and the PDF component remain available
  after the application update.

### macOS release validation

- Application and DMG signing, notarization, stapling, and Gatekeeper
  assessment pass.
- An installed beta.13 client updates to beta.14 through the in-app updater.
- Regress the shared onboarding and Settings changes introduced on the
  Windows rewrite branch.

### API validation

- An older Windows or macOS version receives beta.14 for its own platform.
- beta.14 receives `204 No Content`.
- Unsupported target/architecture combinations receive `204 No Content`.
- Website download requests receive a fresh valid redirect.
- Unpublished releases are never returned by updater or website endpoints.

## Assumptions

- The first beta.14 release uses local release scripts on a Windows release
  machine and an Apple Silicon Mac. GitHub Actions release automation is out
  of scope.
- A trusted Windows code-signing certificate will be available through the
  Windows certificate store before public distribution.
- The existing beta.13 Tauri updater key remains in use. Do not generate a new
  updater key.
- Windows Authenticode and Tauri updater signatures serve different purposes
  and both are required.
- SmartScreen reputation may still need time to develop after signing. This
  does not permit publishing an unsigned installer.
- Supabase release storage remains limited to application release artifacts
  and metadata. User documents, translations, job caches, prompts, and
  runtime logs must remain local.
