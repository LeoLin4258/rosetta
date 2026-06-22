# Windows Release Procedure

Rosetta Windows releases target Windows 10/11 x64. The managed local RWKV
runtime requires an NVIDIA GPU with compute capability SM75 or newer. Users on
other Windows hardware can explicitly configure an external translation API.

## Prerequisites

- A trusted code-signing certificate with its private key in
  `Cert:\CurrentUser\My`.
- Windows SDK `signtool.exe`.
- The existing Rosetta Tauri updater private key.
- Node, pnpm, Rust, and the Tauri build dependencies.
- A clean git worktree with matching versions in `package.json`,
  `Cargo.toml`, and `tauri.conf.json`.

Set release-session environment variables without committing their values:

```powershell
$env:WINDOWS_CERTIFICATE_THUMBPRINT = "<certificate thumbprint>"
$env:WINDOWS_CERTIFICATE_SUBJECT = "<expected publisher subject>"
$env:WINDOWS_TIMESTAMP_URL = "http://timestamp.digicert.com"
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "C:\secure\rosetta\updater.key"
```

If `signtool.exe` is not on `PATH`, set `SIGNTOOL_PATH`.

## Build and sign

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

## Upload as unpublished

```powershell
$env:SUPABASE_SERVICE_ROLE_KEY = "<local release credential>"
.\rosetta-app\src-tauri\scripts\publish-windows-updater.ps1
```

The same signed NSIS executable is used as:

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

Never publish an unsigned installer. SmartScreen reputation can take time to
develop, but that does not replace trusted Authenticode signing.
