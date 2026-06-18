param(
    [string]$SourceArchive = "$env:USERPROFILE\Downloads\RWKV_lightning_CUDA_sm75+_Win_MSVC.7z",
    [string]$OutputArchive = "$env:USERPROFILE\Downloads\RWKV_lightning_CUDA_sm75+_Win_MSVC.zip",
    [string]$ExpectedSourceSha256 = "454a41ad215d4adf156c261991f0732ed22e64e7eed9780321848050435d7a7c"
)

$ErrorActionPreference = "Stop"
$env:PATH = [System.Environment]::GetEnvironmentVariable('PATH', 'Machine')
$BuildRoot = Join-Path ([IO.Path]::GetTempPath()) "rosetta-rwkv-runtime-$PID"
$ResolvedBuildRoot = [IO.Path]::GetFullPath($BuildRoot)
$ResolvedTempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
if (-not $ResolvedBuildRoot.StartsWith($ResolvedTempRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Unsafe build root: $ResolvedBuildRoot"
}
if (-not (Test-Path -LiteralPath $SourceArchive)) {
    throw "Source archive not found: $SourceArchive"
}
$ActualSourceSha256 = (Get-FileHash -LiteralPath $SourceArchive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($ActualSourceSha256 -ne $ExpectedSourceSha256.ToLowerInvariant()) {
    throw "Source archive SHA256 mismatch. Expected $ExpectedSourceSha256, got $ActualSourceSha256"
}
if (Test-Path -LiteralPath $ResolvedBuildRoot) {
    Remove-Item -LiteralPath $ResolvedBuildRoot -Recurse -Force
}

try {
    New-Item -ItemType Directory -Path $ResolvedBuildRoot | Out-Null
    tar -xf $SourceArchive -C $ResolvedBuildRoot
    if ($LASTEXITCODE -ne 0) { throw "Failed to extract source archive" }

    foreach ($required in @(
        "rwkv_lighting_cuda.exe",
        "rwkv_vocab_v20230424.txt",
        "lib"
    )) {
        if (-not (Test-Path -LiteralPath (Join-Path $ResolvedBuildRoot $required))) {
            throw "Source archive is missing $required"
        }
    }

    # The upstream bundle includes hundreds of Windows system DLLs copied
    # from its build machine. Keep a static runtime allowlist instead of
    # comparing with this machine's System32, which made previous ZIPs
    # machine-dependent and impossible to reproduce.
    $KeepRuntimeDlls = @(
        "brotlicommon.dll",
        "brotlidec.dll",
        "brotlienc.dll",
        "cares.dll",
        "cublas64_13.dll",
        "cublasLt64_13.dll",
        "cudart64_13.dll",
        "drogon.dll",
        "jsoncpp.dll",
        "libcrypto-3-x64.dll",
        "libssl-3-x64.dll",
        "concrt140.dll",
        "msvcp140.dll",
        "msvcp140_1.dll",
        "msvcp140_2.dll",
        "sqlite3.dll",
        "trantor.dll",
        "vcruntime140.dll",
        "vcruntime140_1.dll",
        "zlib1.dll"
    )
    $Removed = New-Object System.Collections.Generic.List[string]
    Get-ChildItem -LiteralPath (Join-Path $ResolvedBuildRoot "lib") -File -Filter "*.dll" |
        ForEach-Object {
            if ($KeepRuntimeDlls -notcontains $_.Name) {
                $Removed.Add($_.Name)
                Remove-Item -LiteralPath $_.FullName -Force
            }
        }

    # V1.0.0 hard-codes 0.0.0.0 and crashes on an unknown --host argument.
    # Patch both equal-length ASCII occurrences to IPv6 loopback. NUL padding
    # keeps the PE layout unchanged. The source archive and patched executable
    # hashes make this fail closed if upstream changes.
    $ServerExe = Join-Path $ResolvedBuildRoot "rwkv_lighting_cuda.exe"
    $Latin1 = [Text.Encoding]::GetEncoding(28591)
    $ServerText = $Latin1.GetString([IO.File]::ReadAllBytes($ServerExe))
    $BindLiteral = "0.0.0.0"
    $BindMatches = ([regex]::Matches($ServerText, [regex]::Escape($BindLiteral))).Count
    if ($BindMatches -ne 2) {
        throw "Expected exactly two upstream bind literals, found $BindMatches"
    }
    $ServerText = $ServerText.Replace($BindLiteral, "::1`0`0`0`0")
    [IO.File]::WriteAllBytes($ServerExe, $Latin1.GetBytes($ServerText))
    $PatchedServerSha256 = (Get-FileHash -LiteralPath $ServerExe -Algorithm SHA256).Hash.ToLowerInvariant()
    $ExpectedPatchedServerSha256 = "986f041d6f275faf3c3b502556a521e1a5ffb2ffe807a00c8482a638859860f7"
    if ($PatchedServerSha256 -ne $ExpectedPatchedServerSha256) {
        throw "Patched server SHA256 mismatch. Expected $ExpectedPatchedServerSha256, got $PatchedServerSha256"
    }

    $Manifest = [ordered]@{
        schemaVersion = 1
        sourceArchive = (Split-Path -Leaf $SourceArchive)
        sourceArchiveSha256 = $ActualSourceSha256
        bindAddress = "::1"
        patchedServerSha256 = $PatchedServerSha256
        keptRuntimeDlls = $KeepRuntimeDlls | Sort-Object
        removedSystemDlls = $Removed | Sort-Object
    }
    $Manifest | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath (Join-Path $ResolvedBuildRoot "rosetta-runtime-manifest.json") -Encoding utf8

    # Normalize archive entry timestamps so repeated builds of the same
    # pinned source produce byte-identical ZIP metadata.
    $PinnedTimestampUtc = [DateTime]::Parse(
        "2026-06-17T03:51:55Z",
        [Globalization.CultureInfo]::InvariantCulture,
        [Globalization.DateTimeStyles]::AdjustToUniversal
    )
    Get-ChildItem -LiteralPath $ResolvedBuildRoot -Recurse -Force |
        ForEach-Object { $_.LastWriteTimeUtc = $PinnedTimestampUtc }
    (Get-Item -LiteralPath $ResolvedBuildRoot).LastWriteTimeUtc = $PinnedTimestampUtc

    if (Test-Path -LiteralPath $OutputArchive) {
        Remove-Item -LiteralPath $OutputArchive -Force
    }
    Add-Type -AssemblyName System.IO.Compression
    $ArchiveStream = [IO.File]::Open(
        $OutputArchive,
        [IO.FileMode]::CreateNew,
        [IO.FileAccess]::ReadWrite,
        [IO.FileShare]::None
    )
    $Zip = $null
    try {
        $Zip = New-Object IO.Compression.ZipArchive(
            $ArchiveStream,
            [IO.Compression.ZipArchiveMode]::Create,
            $true
        )
        Get-ChildItem -LiteralPath $ResolvedBuildRoot -Recurse -File |
            Sort-Object FullName |
            ForEach-Object {
                $RelativePath = $_.FullName.Substring($ResolvedBuildRoot.Length).TrimStart('\')
                $EntryName = $RelativePath.Replace('\', '/')
                $Entry = $Zip.CreateEntry(
                    $EntryName,
                    [IO.Compression.CompressionLevel]::Optimal
                )
                $Entry.LastWriteTime = [DateTimeOffset]$PinnedTimestampUtc
                $Input = [IO.File]::OpenRead($_.FullName)
                $Output = $Entry.Open()
                try {
                    $Input.CopyTo($Output)
                } finally {
                    $Output.Dispose()
                    $Input.Dispose()
                }
            }
    } finally {
        if ($null -ne $Zip) { $Zip.Dispose() }
        $ArchiveStream.Dispose()
    }

    $File = Get-Item -LiteralPath $OutputArchive
    $Sha = (Get-FileHash -LiteralPath $OutputArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Host "Archive: $($File.FullName)"
    Write-Host "Size: $($File.Length)"
    Write-Host "SHA256: $Sha"
    Write-Host "Patched server SHA256: $PatchedServerSha256"
    Write-Host "Removed upstream system DLLs: $($Removed.Count)"
} finally {
    if (Test-Path -LiteralPath $ResolvedBuildRoot) {
        Remove-Item -LiteralPath $ResolvedBuildRoot -Recurse -Force
    }
}
