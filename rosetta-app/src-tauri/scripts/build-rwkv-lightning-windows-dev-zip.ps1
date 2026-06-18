param(
    [string]$SourceArchive = "$env:USERPROFILE\Downloads\RWKV_lightning_CUDA_sm75+_Win_MSVC.7z",
    [string]$OutputArchive = "$env:USERPROFILE\Downloads\RWKV_lightning_CUDA_sm75+_Win_MSVC.zip"
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

    $System32 = Join-Path $env:WINDIR "System32"
    $KeepMicrosoftRuntime = @(
        "concrt140.dll",
        "msvcp140.dll",
        "msvcp140_1.dll",
        "msvcp140_2.dll",
        "vcruntime140.dll",
        "vcruntime140_1.dll"
    )
    $Removed = New-Object System.Collections.Generic.List[string]
    Get-ChildItem -LiteralPath (Join-Path $ResolvedBuildRoot "lib") -File -Filter "*.dll" |
        ForEach-Object {
            $SystemCopy = Join-Path $System32 $_.Name
            if ((Test-Path -LiteralPath $SystemCopy) -and
                ($KeepMicrosoftRuntime -notcontains $_.Name.ToLowerInvariant())) {
                $Removed.Add($_.Name)
                Remove-Item -LiteralPath $_.FullName -Force
            }
        }

    $Manifest = [ordered]@{
        schemaVersion = 1
        sourceArchive = (Split-Path -Leaf $SourceArchive)
        removedSystemDlls = $Removed | Sort-Object
        createdAt = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
    }
    $Manifest | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath (Join-Path $ResolvedBuildRoot "rosetta-runtime-manifest.json") -Encoding utf8

    if (Test-Path -LiteralPath $OutputArchive) {
        Remove-Item -LiteralPath $OutputArchive -Force
    }
    tar -a -cf $OutputArchive -C $ResolvedBuildRoot .
    if ($LASTEXITCODE -ne 0) { throw "Failed to create ZIP" }

    $File = Get-Item -LiteralPath $OutputArchive
    $Sha = (Get-FileHash -LiteralPath $OutputArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Host "Archive: $($File.FullName)"
    Write-Host "Size: $($File.Length)"
    Write-Host "SHA256: $Sha"
    Write-Host "Removed System32 DLLs: $($Removed.Count)"
} finally {
    if (Test-Path -LiteralPath $ResolvedBuildRoot) {
        Remove-Item -LiteralPath $ResolvedBuildRoot -Recurse -Force
    }
}
