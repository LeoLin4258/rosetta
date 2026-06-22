[CmdletBinding()]
param(
    [switch]$AllowUnsignedPreview
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$AppName = "Rosetta"
$ExpectedPublisher = $env:WINDOWS_CERTIFICATE_SUBJECT
$CertificateThumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT
$TimestampUrl = if ($env:WINDOWS_TIMESTAMP_URL) {
    $env:WINDOWS_TIMESTAMP_URL
} else {
    "http://timestamp.digicert.com"
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$TauriDir = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$AppDir = (Resolve-Path (Join-Path $TauriDir "..")).Path
$RepoRoot = (Resolve-Path (Join-Path $AppDir "..")).Path
$DistDir = Join-Path $RepoRoot "dist\release"

function Require-Command {
    param([Parameter(Mandatory)][string]$Name)

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "Missing required command: $Name"
    }
    return $command.Source
}

function Require-Environment {
    param([Parameter(Mandatory)][string]$Name)

    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "Missing required environment variable: $Name"
    }
    return $value
}

function Resolve-SignTool {
    if ($env:SIGNTOOL_PATH) {
        if (-not (Test-Path -LiteralPath $env:SIGNTOOL_PATH -PathType Leaf)) {
            throw "SIGNTOOL_PATH does not exist: $env:SIGNTOOL_PATH"
        }
        return (Resolve-Path -LiteralPath $env:SIGNTOOL_PATH).Path
    }

    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $kitsRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    if (Test-Path -LiteralPath $kitsRoot) {
        $candidate = Get-ChildItem -LiteralPath $kitsRoot -Filter signtool.exe -Recurse -File |
            Where-Object { $_.FullName -match "\\x64\\signtool\.exe$" } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($candidate) {
            return $candidate.FullName
        }
    }

    throw "Could not find signtool.exe. Install the Windows SDK or set SIGNTOOL_PATH."
}

function Read-ReleaseVersion {
    $packageVersion = (Get-Content -Raw (Join-Path $AppDir "package.json") | ConvertFrom-Json).version
    $tauriVersion = (Get-Content -Raw (Join-Path $TauriDir "tauri.conf.json") | ConvertFrom-Json).version
    $cargoText = Get-Content -Raw (Join-Path $TauriDir "Cargo.toml")
    $cargoMatch = [regex]::Match($cargoText, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"')
    if (-not $cargoMatch.Success) {
        throw "Could not read package version from Cargo.toml"
    }
    $cargoVersion = $cargoMatch.Groups[1].Value

    if ($packageVersion -ne $tauriVersion -or $packageVersion -ne $cargoVersion) {
        throw "Version mismatch: package.json=$packageVersion tauri.conf.json=$tauriVersion Cargo.toml=$cargoVersion"
    }
    return $packageVersion
}

function Assert-CleanWorktree {
    $status = & git -C $RepoRoot status --porcelain --untracked-files=all
    if ($LASTEXITCODE -ne 0) {
        throw "git status failed"
    }
    if ($status) {
        throw "Release builds require a clean worktree.`n$status"
    }
}

Require-Command git | Out-Null
Require-Command node | Out-Null
Require-Command pnpm | Out-Null
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and -not $env:TAURI_SIGNING_PRIVATE_KEY_PATH) {
    throw "Set TAURI_SIGNING_PRIVATE_KEY_PATH or TAURI_SIGNING_PRIVATE_KEY for updater signing."
}
if ($env:TAURI_SIGNING_PRIVATE_KEY_PATH -and -not (Test-Path -LiteralPath $env:TAURI_SIGNING_PRIVATE_KEY_PATH -PathType Leaf)) {
    throw "TAURI_SIGNING_PRIVATE_KEY_PATH does not exist: $env:TAURI_SIGNING_PRIVATE_KEY_PATH"
}

Assert-CleanWorktree
$version = Read-ReleaseVersion

$signTool = $null
$certificate = $null
if ($AllowUnsignedPreview) {
    Write-Warning "Building an unsigned Windows Preview. Users may see SmartScreen or unknown-publisher warnings."
} else {
    $signTool = Resolve-SignTool
    $CertificateThumbprint = Require-Environment "WINDOWS_CERTIFICATE_THUMBPRINT"
    $ExpectedPublisher = Require-Environment "WINDOWS_CERTIFICATE_SUBJECT"

    $certificate = Get-ChildItem Cert:\CurrentUser\My |
        Where-Object { $_.Thumbprint -eq ($CertificateThumbprint -replace "\s", "").ToUpperInvariant() } |
        Select-Object -First 1
    if (-not $certificate -or -not $certificate.HasPrivateKey) {
        throw "The configured code-signing certificate was not found with a private key in Cert:\CurrentUser\My."
    }
    if ($certificate.Subject -notlike "*$ExpectedPublisher*") {
        throw "Certificate subject '$($certificate.Subject)' does not contain expected publisher '$ExpectedPublisher'."
    }
    if ($certificate.NotAfter -le (Get-Date)) {
        throw "The configured code-signing certificate expired on $($certificate.NotAfter)."
    }
}

Write-Host "Building Rosetta $version Windows x64 NSIS package"
Push-Location $AppDir
try {
    & pnpm tauri build --bundles nsis --no-sign
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri Windows build failed."
    }
} finally {
    Pop-Location
}

$bundleDir = Join-Path $TauriDir "target\release\bundle\nsis"
$builtInstaller = Get-ChildItem -LiteralPath $bundleDir -Filter *.exe -File |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1
if (-not $builtInstaller) {
    throw "No NSIS installer found under $bundleDir"
}

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
$installerPath = Join-Path $DistDir "$AppName-$version-windows-x64-setup.exe"
Copy-Item -LiteralPath $builtInstaller.FullName -Destination $installerPath -Force

$publisher = "Unsigned Windows Preview"
if ($AllowUnsignedPreview) {
    $authenticode = Get-AuthenticodeSignature -LiteralPath $installerPath
    if ($authenticode.Status -ne "NotSigned") {
        throw "Unsigned Preview expected Authenticode status NotSigned, got $($authenticode.Status)."
    }
} else {
    Write-Host "Applying Authenticode signature"
    & $signTool sign `
        /sha1 $certificate.Thumbprint `
        /fd SHA256 `
        /tr $TimestampUrl `
        /td SHA256 `
        /v `
        $installerPath
    if ($LASTEXITCODE -ne 0) {
        throw "signtool failed to sign the installer."
    }

    $authenticode = Get-AuthenticodeSignature -LiteralPath $installerPath
    if ($authenticode.Status -ne "Valid") {
        throw "Authenticode verification failed: $($authenticode.Status) $($authenticode.StatusMessage)"
    }
    if ($authenticode.SignerCertificate.Subject -notlike "*$ExpectedPublisher*") {
        throw "Signed installer publisher '$($authenticode.SignerCertificate.Subject)' does not contain '$ExpectedPublisher'."
    }
    $publisher = $authenticode.SignerCertificate.Subject
}

$signaturePath = "$installerPath.sig"
Remove-Item -LiteralPath $signaturePath -Force -ErrorAction SilentlyContinue
$signerArgs = @("--silent", "tauri", "signer", "sign")
if ($env:TAURI_SIGNING_PRIVATE_KEY_PATH) {
    $signerArgs += @("-f", $env:TAURI_SIGNING_PRIVATE_KEY_PATH)
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    $signerArgs += "--password="
}
$signerArgs += $installerPath

Push-Location $AppDir
try {
    & pnpm @signerArgs | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri updater signing failed."
    }
} finally {
    Pop-Location
}
if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) {
    throw "Tauri updater signature was not created: $signaturePath"
}

$sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$shaPath = "$installerPath.sha256"
$sizePath = "$installerPath.size"
Set-Content -LiteralPath $shaPath -Value "$sha256  $([IO.Path]::GetFileName($installerPath))" -NoNewline
Set-Content -LiteralPath $sizePath -Value (Get-Item -LiteralPath $installerPath).Length -NoNewline

Write-Host ""
Write-Host "Windows release artifacts ready:"
Write-Host "  Installer: $installerPath"
Write-Host "  Signature: $signaturePath"
Write-Host "  SHA256:    $sha256"
Write-Host "  Publisher: $publisher"
Write-Host ""
if ($AllowUnsignedPreview) {
    Write-Host "Next: run rosetta-app/src-tauri/scripts/publish-windows-updater.ps1 -AllowUnsignedPreview"
} else {
    Write-Host "Next: run rosetta-app/src-tauri/scripts/publish-windows-updater.ps1"
}
