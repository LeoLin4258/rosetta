[CmdletBinding()]
param(
    [switch]$Publish,
    [switch]$AllowUnsignedPreview
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$AppName = "rosetta"
$Target = "windows"
$Arch = "x86_64"
$Bucket = "rosetta-releases"
$ProjectUrl = if ($env:SUPABASE_PROJECT_URL) {
    $env:SUPABASE_PROJECT_URL.TrimEnd("/")
} else {
    "https://bdujdewqopcgwijhfbcz.supabase.co"
}
$PublisherUserAgent = "Rosetta-Release-Publisher/1.0"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$TauriDir = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$AppDir = (Resolve-Path (Join-Path $TauriDir "..")).Path
$RepoRoot = (Resolve-Path (Join-Path $AppDir "..")).Path
$DistDir = Join-Path $RepoRoot "dist\release"

function Require-Environment {
    param([Parameter(Mandatory)][string]$Name)

    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "Missing required environment variable: $Name"
    }
    return $value
}

function Read-ReleaseVersion {
    $packageVersion = (Get-Content -Raw (Join-Path $AppDir "package.json") | ConvertFrom-Json).version
    $tauriVersion = (Get-Content -Raw (Join-Path $TauriDir "tauri.conf.json") | ConvertFrom-Json).version
    $cargoText = Get-Content -Raw (Join-Path $TauriDir "Cargo.toml")
    $cargoVersion = [regex]::Match(
        $cargoText,
        '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"'
    ).Groups[1].Value
    if ($packageVersion -ne $tauriVersion -or $packageVersion -ne $cargoVersion) {
        throw "Version mismatch: package.json=$packageVersion tauri.conf.json=$tauriVersion Cargo.toml=$cargoVersion"
    }
    return $packageVersion
}

function Invoke-SupabaseUpload {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string]$StoragePath,
        [Parameter(Mandatory)][hashtable]$Headers
    )

    Invoke-RestMethod `
        -Method Post `
        -Uri "$ProjectUrl/storage/v1/object/$Bucket/$StoragePath" `
        -Headers ($Headers + @{
            "User-Agent" = $PublisherUserAgent
            "x-upsert" = "true"
        }) `
        -ContentType "application/octet-stream" `
        -InFile $FilePath | Out-Null
}

$serviceRoleKey = Require-Environment "SUPABASE_SERVICE_ROLE_KEY"
$version = Read-ReleaseVersion
$installerPath = Join-Path $DistDir "Rosetta-$version-windows-x64-setup.exe"
$signaturePath = "$installerPath.sig"

if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
    throw "Missing signed installer: $installerPath"
}
if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) {
    throw "Missing Tauri signature: $signaturePath"
}
$authenticode = Get-AuthenticodeSignature -LiteralPath $installerPath
$isUnsignedPreview = $authenticode.Status -eq "NotSigned"
if ($isUnsignedPreview -and -not $AllowUnsignedPreview) {
    throw "Installer is unsigned. Pass -AllowUnsignedPreview only for an explicitly disclosed Windows Preview release."
}
if (-not $isUnsignedPreview -and $authenticode.Status -ne "Valid") {
    throw "Installer Authenticode status is $($authenticode.Status), refusing to publish."
}
if ($AllowUnsignedPreview -and -not $isUnsignedPreview) {
    throw "-AllowUnsignedPreview was provided, but installer Authenticode status is $($authenticode.Status)."
}

$headers = @{
    Authorization = "Bearer $serviceRoleKey"
    apikey = $serviceRoleKey
}
$filename = Split-Path -Leaf $installerPath
$storagePath = "windows/x86_64/$version/$filename"
$signature = (Get-Content -Raw -LiteralPath $signaturePath).Trim()
$sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$size = (Get-Item -LiteralPath $installerPath).Length

if ($isUnsignedPreview) {
    Write-Warning "Uploading an unsigned Windows Preview installer. Tauri updater signature verification remains required."
} else {
    Write-Host "Uploading Authenticode-signed Windows installer"
}
Invoke-SupabaseUpload -FilePath $installerPath -StoragePath $storagePath -Headers $headers

$notes = if ($isUnsignedPreview) {
    "Rosetta $version Windows Preview. The installer is not Authenticode-signed and may trigger SmartScreen. In-app updates remain protected by the Tauri updater signature."
} else {
    "Rosetta $version"
}

$payload = @{
    app = $AppName
    version = $version
    target = $Target
    arch = $Arch
    storage_bucket = $Bucket
    storage_path = $storagePath
    installer_storage_path = $storagePath
    installer_sha256 = $sha256
    installer_size_bytes = $size
    signature = $signature
    notes = $notes
    is_published = [bool]$Publish
} | ConvertTo-Json -Compress

Invoke-RestMethod `
    -Method Post `
    -Uri "$ProjectUrl/rest/v1/app_releases?on_conflict=app,version,target,arch" `
    -Headers ($headers + @{
        "User-Agent" = $PublisherUserAgent
        Prefer = "resolution=merge-duplicates"
    }) `
    -ContentType "application/json" `
    -Body $payload | Out-Null

Write-Host ""
Write-Host "Windows release uploaded:"
Write-Host "  Version:   $version"
Write-Host "  Published: $([bool]$Publish)"
Write-Host "  Channel:   $(if ($isUnsignedPreview) { 'Unsigned Preview' } else { 'Signed' })"
Write-Host "  SHA256:    $sha256"
Write-Host "  Size:      $size bytes"

if (-not $Publish) {
    Write-Host ""
    Write-Host "After smoke testing, publish with:"
    Write-Host "`$env:ROSETTA_RELEASE_VERSION='$version'"
    Write-Host "Invoke-RestMethod -Method Patch -Uri '$ProjectUrl/rest/v1/app_releases?app=eq.$AppName&version=eq.$version&target=eq.$Target&arch=eq.$Arch' -Headers @{ Authorization='Bearer `$env:SUPABASE_SERVICE_ROLE_KEY'; apikey=`$env:SUPABASE_SERVICE_ROLE_KEY } -ContentType 'application/json' -Body '{`"is_published`":true}'"
    Write-Host ""
    Write-Host "Rollback uses the same command with is_published=false."
}
