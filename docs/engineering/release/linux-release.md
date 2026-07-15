# Linux x64 Release Procedure

Rosetta's first Linux release format is AppImage on Ubuntu 24.04 or newer,
x86_64. The AppImage is the manual download artifact and is also the source for
the signed Tauri updater archive.

The AppImage and signed updater archive are published through the same private
Supabase release channel as the Windows and macOS artifacts.

## Release Contract

The release script writes these files under `dist/release/`:

```txt
Rosetta-<version>-linux-x64.AppImage
Rosetta-<version>-linux-x64.AppImage.sha256
Rosetta-<version>-linux-x64.AppImage.size
Rosetta-<version>-linux-x64.AppImage.tar.gz
Rosetta-<version>-linux-x64.AppImage.tar.gz.sig
Rosetta-<version>-linux-x64.AppImage.tar.gz.sha256
Rosetta-<version>-linux-x64.AppImage.tar.gz.size
```

The plain AppImage is for website downloads. The `.AppImage.tar.gz` and `.sig`
are for Tauri's in-app updater. Do not point the updater at the plain AppImage.

## Prerequisites

- Ubuntu 24.04 x64 release host.
- Node.js, pnpm, Rust, Tauri Linux build dependencies, `file`, `tar`, and
  `sha256sum`.
- Staged Linux x64 PDFium under
  `rosetta-app/src-tauri/resources/pdf-sidecar/pdfium/linux-x64/`.
- The existing Rosetta Tauri updater private key, stored outside the repository.
- The matching updater public key beside it as `updater.key.pub`, or referenced
  through `TAURI_SIGNING_PUBLIC_KEY_PATH`.
- A clean worktree with matching versions in `package.json`, `Cargo.toml`, and
  `tauri.conf.json`.

Use the same updater keypair as Windows and macOS. Do not create a new Linux
keypair. A common local path is:

```txt
~/.tauri/rosetta/updater.key
~/.tauri/rosetta/updater.key.pub
```

## Build A Release

From the repository root:

```bash
export TAURI_SIGNING_PRIVATE_KEY_PATH="$HOME/.tauri/rosetta/updater.key"
bash rosetta-app/src-tauri/scripts/release-linux.sh
```

Set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` only if the existing key requires it.
The script rejects dirty worktrees, missing keys, version mismatches, wrong-host
builds, and non-x86_64 PDFium resources.

## Build A Local Preview

For packaging development only:

```bash
bash rosetta-app/src-tauri/scripts/release-linux.sh --unsigned-preview
```

When testing uncommitted changes, both explicit preview flags are required:

```bash
bash rosetta-app/src-tauri/scripts/release-linux.sh \
  --unsigned-preview \
  --allow-dirty-preview
```

Unsigned previews produce only the AppImage and its checksum metadata. They
must not be uploaded to the release service or presented as public releases.

## Smoke Test

On a clean Ubuntu 24.04 x64 user session:

1. Launch the AppImage without using the source checkout or `tauri dev`.
2. Complete or verify managed RWKV runtime setup.
3. Install the PDF component through the public online flow.
4. Translate TXT, Markdown, and a real PDF page.
5. Preview and export the translated PDF.
6. Close Rosetta and confirm its managed worker processes exit.

## Upload As Unpublished

Apply `202607150001_linux_app_releases.sql` and deploy the updated
`rosetta-update` and `rosetta-latest-download` Edge Functions before the first
Linux upload.

Set the release credential only in the local shell, then upload both the
AppImage and signed updater archive:

```bash
export SUPABASE_SERVICE_ROLE_KEY="<local release credential>"
bash rosetta-app/src-tauri/scripts/publish-linux-updater.sh
```

The script validates the exact filenames, versions, architecture, archive
contents, signatures, hashes, and sizes. It stores the updater archive under
`storage_path`, the AppImage under `installer_storage_path`, and creates an
unpublished `linux/x86_64` row.

After testing the unpublished artifacts, run the `PATCH` command printed by
the script. Rollback uses the same command with `is_published=false`.

Verify both public endpoints after publishing:

```bash
curl -s \
  'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=linux&arch=x86_64&current_version=0.0.0'

curl -s \
  'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-latest-download?target=linux&arch=x86_64'
```

If FUSE 2 compatibility is unavailable on the test system, install Ubuntu's
`libfuse2t64` package. `APPIMAGE_EXTRACT_AND_RUN=1` may be used for diagnosis,
but the public acceptance test must also cover normal AppImage launch.

## Compatibility Boundary

The current native build host uses Ubuntu 24.04 and glibc 2.39. Do not describe
this artifact as Ubuntu 22.04-compatible without rebuilding on an older glibc
baseline and repeating the complete smoke test.
