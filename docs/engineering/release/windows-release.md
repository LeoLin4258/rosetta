# Windows Release Procedure

Rosetta Windows releases target Windows 10/11 x64. The managed local RWKV
runtime requires an NVIDIA GPU with compute capability SM75 or newer. Users on
other Windows hardware can explicitly configure an external translation API.

## Shared updater key

Windows and macOS builds use the same Tauri updater keypair because both apps
embed the same updater public key from `main`. Do not generate a new key on
Windows.

Before the first Windows release, securely copy this exact keypair from the
Mac:

```txt
~/.tauri/rosetta/updater.key
~/.tauri/rosetta/updater.key.pub
```

Store the Windows copy outside the repository, for example:

```txt
C:\Users\Leo\.tauri\rosetta\updater.key
C:\Users\Leo\.tauri\rosetta\updater.key.pub
```

Use encrypted removable storage or another end-to-end encrypted transfer. Do
not send the key through chat, email, or commit it to Git.

Verify the copied public key matches the app before building:

```powershell
$configured = (Get-Content -Raw .\rosetta-app\src-tauri\tauri.conf.json |
  ConvertFrom-Json).plugins.updater.pubkey.Trim()
$copied = (Get-Content -Raw "$HOME\.tauri\rosetta\updater.key.pub").Trim()
if ($configured -ne $copied) {
  throw "Copied updater key does not match the public key embedded in Rosetta."
}
```

## Prerequisites

- The existing Rosetta Tauri updater private key.
- Node, pnpm, Rust, and the Tauri build dependencies.
- A clean git worktree with matching versions in `package.json`,
  `Cargo.toml`, and `tauri.conf.json`.

Set the updater key for every Windows release:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$HOME\.tauri\rosetta\updater.key"
```

For a signed public release, also configure a trusted code-signing certificate
and Windows SDK `signtool.exe`:

```powershell
$env:WINDOWS_CERTIFICATE_THUMBPRINT = "<certificate thumbprint>"
$env:WINDOWS_CERTIFICATE_SUBJECT = "<expected publisher subject>"
$env:WINDOWS_TIMESTAMP_URL = "http://timestamp.digicert.com"
```

If `signtool.exe` is not on `PATH`, set `SIGNTOOL_PATH`.

## Build a signed release

From the repository root:

```powershell
.\rosetta-app\src-tauri\scripts\release-windows.ps1
```

The script:

1. rejects a dirty worktree or version mismatch;
2. verifies the certificate and expected publisher;
3. builds the NSIS installer;
4. signs it with SHA-256 and an RFC 3161 timestamp;
5. verifies `Get-AuthenticodeSignature` reports `Valid`;
6. signs the final installer bytes with the Tauri updater key;
7. writes the installer, `.sig`, SHA256, and byte size under `dist/release/`.

## Build an unsigned Windows Preview

Until an Authenticode certificate is available, build only through the
explicit Preview switch:

```powershell
.\rosetta-app\src-tauri\scripts\release-windows.ps1 -AllowUnsignedPreview
```

This skips Authenticode but still requires the shared Tauri updater private
key and produces a `.sig`. The `.sig` protects App-internal updates from
tampering. It does not remove the initial SmartScreen or unknown-publisher
warning.

## Upload as unpublished

```powershell
$env:SUPABASE_SERVICE_ROLE_KEY = "<local release credential>"
.\rosetta-app\src-tauri\scripts\publish-windows-updater.ps1 -AllowUnsignedPreview
```

For a signed release, omit `-AllowUnsignedPreview` from both commands. The
same NSIS executable is used as:

- the website installer;
- the Tauri Windows updater artifact.

The script uploads the artifact and creates an unpublished
`windows/x86_64` release row. Do not pass `-Publish` for the first upload.

## Smoke test and publish

Before publishing:

- install and uninstall on a clean Windows machine;
- complete managed RWKV and PDF onboarding;
- translate text and PDF documents, preview, and export;
- update an installed older version through Settings;
- confirm settings, jobs, models, and PDF components remain available;
- verify Rosetta and its managed processes exit after closing the main window.

After these checks pass, run the publish command printed by the upload script.
To roll back, set the same row to `is_published=false`.

An unsigned build must be described as **Windows Preview** on every download
surface. Never silently publish it as a signed or stable Windows release.
