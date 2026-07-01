param(
    [string]$SourceRoot = "$env:USERPROFILE\Documents\GitHub\rwkv_lightning_cuda",
    [string]$BuildDir = "$env:USERPROFILE\Documents\GitHub\rwkv_lightning_cuda\build_rosetta_sm75plus",
    [string]$DistDir = "$PSScriptRoot\..\..\dist\rwkv-runtime",
    [string]$ArchiveName = "RWKV_lightning_CUDA_sm75+_Win_MSVC_V1.0.2_rosetta-loopback.zip",
    [string]$CudaArchitectures = "75;80;86;87;89;90;100;120",
    [string]$VcpkgToolchain = "",
    [string]$CudaToolkitDir = $env:CudaToolkitDir,
    [string]$ReleaseTag = "",
    [switch]$SkipConfigure
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# Keep a Visual Studio developer environment when the caller has already loaded
# one through vcvars64.bat. Otherwise fall back to the machine PATH so Git
# Bash/MSYS tools do not shadow Windows-native build tools.
if ($null -eq (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine")
}

function Resolve-AbsolutePath([string]$PathValue) {
    if ([string]::IsNullOrWhiteSpace($PathValue)) {
        throw "Path must not be empty."
    }
    [IO.Path]::GetFullPath($PathValue)
}

function Assert-Directory([string]$PathValue, [string]$Label) {
    if (-not (Test-Path -LiteralPath $PathValue -PathType Container)) {
        throw "$Label directory not found: $PathValue"
    }
}

function Invoke-Native([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

$ResolvedSourceRoot = Resolve-AbsolutePath $SourceRoot
$ResolvedBuildDir = Resolve-AbsolutePath $BuildDir
$ResolvedDistDir = Resolve-AbsolutePath $DistDir
$ArchivePath = Join-Path $ResolvedDistDir $ArchiveName
$BundleDir = Join-Path $ResolvedBuildDir "bundle\rwkv_lighting_cuda"
$LibDir = Join-Path $BundleDir "lib"
$VocabSource = Join-Path $ResolvedSourceRoot "src\rwkv_vocab_v20230424.txt"

Assert-Directory $ResolvedSourceRoot "Source"
if (-not (Test-Path -LiteralPath (Join-Path $ResolvedSourceRoot "CMakeLists.txt") -PathType Leaf)) {
    throw "Source root does not look like rwkv_lightning_cuda: $ResolvedSourceRoot"
}
if (-not (Test-Path -LiteralPath $VocabSource -PathType Leaf)) {
    throw "Vocab file not found: $VocabSource"
}

if ([string]::IsNullOrWhiteSpace($CudaToolkitDir)) {
    $CudaToolkitDir = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2"
}
$CudaToolkitDir = Resolve-AbsolutePath $CudaToolkitDir
Assert-Directory $CudaToolkitDir "CUDA toolkit"
if (-not $CudaToolkitDir.EndsWith("\")) {
    $env:CudaToolkitDir = "$CudaToolkitDir\"
} else {
    $env:CudaToolkitDir = $CudaToolkitDir
}

New-Item -ItemType Directory -Path $ResolvedDistDir -Force | Out-Null

if (-not $SkipConfigure) {
    $ConfigureArgs = @(
        "-S", $ResolvedSourceRoot,
        "-B", $ResolvedBuildDir,
        "-DCMAKE_BUILD_TYPE=Release",
        "-DCMAKE_CUDA_ARCHITECTURES=$CudaArchitectures",
        "-DCUDAToolkit_ROOT=$CudaToolkitDir",
        "-DCMAKE_CXX_FLAGS=/Zc:preprocessor",
        "-DCMAKE_CUDA_FLAGS=-Xcompiler=/Zc:preprocessor"
    )
    if (-not [string]::IsNullOrWhiteSpace($VcpkgToolchain)) {
        $ConfigureArgs += "-DCMAKE_TOOLCHAIN_FILE=$VcpkgToolchain"
    }
    Invoke-Native "cmake" $ConfigureArgs
}

Invoke-Native "cmake" @(
    "--build", $ResolvedBuildDir,
    "--config", "Release",
    "-j",
    "--target", "bundle_rwkv_lighting_cuda"
)

Assert-Directory $BundleDir "Bundle"
Assert-Directory $LibDir "Bundle lib"

$ServerExe = Join-Path $BundleDir "rwkv_lighting_cuda.exe"
if (-not (Test-Path -LiteralPath $ServerExe -PathType Leaf)) {
    throw "Bundle is missing rwkv_lighting_cuda.exe: $ServerExe"
}
Copy-Item -LiteralPath $VocabSource -Destination (Join-Path $BundleDir "rwkv_vocab_v20230424.txt") -Force

$CudaBin = Join-Path $CudaToolkitDir "bin"
if (-not (Test-Path -LiteralPath (Join-Path $CudaBin "cudart64_13.dll") -PathType Leaf)) {
    $CudaBinX64 = Join-Path $CudaBin "x64"
    if (Test-Path -LiteralPath $CudaBinX64 -PathType Container) {
        $CudaBin = $CudaBinX64
    }
}
Assert-Directory $CudaBin "CUDA bin"

$CudaDllPatterns = @("cudart64_*.dll", "cublas64_*.dll", "cublasLt64_*.dll")
foreach ($Pattern in $CudaDllPatterns) {
    $Matches = @(Get-ChildItem -LiteralPath $CudaBin -File -Filter $Pattern | Sort-Object Name)
    if ($Matches.Count -eq 0) {
        throw "CUDA runtime DLL not found in $CudaBin with pattern $Pattern"
    }
    foreach ($Dll in $Matches) {
        Copy-Item -LiteralPath $Dll.FullName -Destination $LibDir -Force
    }
}

function Resolve-VcRuntimeDll([string]$DllName) {
    $CandidateDirs = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrWhiteSpace($env:VCToolsRedistDir)) {
        $CandidateDirs.Add((Join-Path $env:VCToolsRedistDir "x64\Microsoft.VC143.CRT"))
    }
    $VcRedistRoot = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Redist\MSVC"
    if (Test-Path -LiteralPath $VcRedistRoot -PathType Container) {
        Get-ChildItem -LiteralPath $VcRedistRoot -Directory |
            Sort-Object Name -Descending |
            ForEach-Object {
                $CandidateDirs.Add((Join-Path $_.FullName "x64\Microsoft.VC143.CRT"))
            }
    }
    $CandidateDirs.Add("C:\Windows\System32")

    foreach ($Dir in $CandidateDirs) {
        $Candidate = Join-Path $Dir $DllName
        if (Test-Path -LiteralPath $Candidate -PathType Leaf) {
            return $Candidate
        }
    }
    return $null
}

foreach ($VcRuntimeDll in @("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll")) {
    $RuntimePath = Resolve-VcRuntimeDll $VcRuntimeDll
    if ($null -ne $RuntimePath) {
        Copy-Item -LiteralPath $RuntimePath -Destination $LibDir -Force
    }
}

$KeepRuntimeDlls = @(
    "brotlicommon.dll",
    "brotlidec.dll",
    "brotlienc.dll",
    "cares.dll",
    "cublas64_12.dll",
    "cublas64_13.dll",
    "cublasLt64_12.dll",
    "cublasLt64_13.dll",
    "cudart64_12.dll",
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
    "z.dll",
    "zlib1.dll"
)

$Removed = New-Object System.Collections.Generic.List[string]
Get-ChildItem -LiteralPath $LibDir -File -Filter "*.dll" |
    ForEach-Object {
        if ($KeepRuntimeDlls -notcontains $_.Name) {
            $Removed.Add($_.Name)
            Remove-Item -LiteralPath $_.FullName -Force
        }
    }

foreach ($Required in @(
    "rwkv_lighting_cuda.exe",
    "rwkv_vocab_v20230424.txt",
    "lib"
)) {
    if (-not (Test-Path -LiteralPath (Join-Path $BundleDir $Required))) {
        throw "Bundle is missing required entry: $Required"
    }
}

$ServerSha256 = (Get-FileHash -LiteralPath $ServerExe -Algorithm SHA256).Hash.ToLowerInvariant()

$ManifestPath = Join-Path $BundleDir "rosetta-runtime-manifest.json"
$Manifest = [ordered]@{
    schemaVersion = 1
    profileId = "windows-amd64-rwkv-lightning-cuda"
    upstreamSource = "rwkv_lightning_cuda"
    buildKind = "rosetta-source-build"
    cudaArchitectures = $CudaArchitectures
    bindContract = "Rosetta launches this runtime with an explicit --host loopback value."
    serverSha256 = $ServerSha256
    keptRuntimeDlls = $KeepRuntimeDlls | Sort-Object
    removedRuntimeDlls = $Removed | Sort-Object
    packagedAt = "2026-07-01T00:00:00Z"
}
if (-not [string]::IsNullOrWhiteSpace($ReleaseTag)) {
    $Manifest["releaseTag"] = $ReleaseTag
    $Manifest["downloadUrls"] = @(
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/$ReleaseTag/$ArchiveName",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/$ReleaseTag/$ArchiveName"
    )
}
$Manifest | ConvertTo-Json -Depth 5 |
    Set-Content -LiteralPath $ManifestPath -Encoding utf8

$PinnedTimestampUtc = [DateTime]::Parse(
    "2026-07-01T00:00:00Z",
    [Globalization.CultureInfo]::InvariantCulture,
    [Globalization.DateTimeStyles]::AdjustToUniversal
)
Get-ChildItem -LiteralPath $BundleDir -Recurse -Force |
    ForEach-Object { $_.LastWriteTimeUtc = $PinnedTimestampUtc }
(Get-Item -LiteralPath $BundleDir).LastWriteTimeUtc = $PinnedTimestampUtc

if (Test-Path -LiteralPath $ArchivePath) {
    Remove-Item -LiteralPath $ArchivePath -Force
}

Add-Type -AssemblyName System.IO.Compression
$ArchiveStream = [IO.File]::Open(
    $ArchivePath,
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
    Get-ChildItem -LiteralPath $BundleDir -Recurse -File |
        Sort-Object FullName |
        ForEach-Object {
            $RelativePath = $_.FullName.Substring($BundleDir.Length).TrimStart("\")
            $EntryName = $RelativePath.Replace("\", "/")
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

$Archive = Get-Item -LiteralPath $ArchivePath
$ArchiveSha256 = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash.ToLowerInvariant()

$DistManifest = [ordered]@{
    profileId = "windows-amd64-rwkv-lightning-cuda"
    archiveFilename = $ArchiveName
    archivePath = $Archive.FullName
    sizeBytes = $Archive.Length
    sha256 = $ArchiveSha256
    serverSha256 = $ServerSha256
    cudaArchitectures = $CudaArchitectures
    releaseTag = $ReleaseTag
}
$DistManifest | ConvertTo-Json -Depth 4 |
    Set-Content -LiteralPath (Join-Path $ResolvedDistDir "rwkv-lightning-cuda-windows-amd64-manifest.json") -Encoding utf8

Write-Host "Archive: $($Archive.FullName)"
Write-Host "Size: $($Archive.Length)"
Write-Host "SHA256: $ArchiveSha256"
Write-Host "Server SHA256: $ServerSha256"
Write-Host "Removed runtime DLLs: $($Removed.Count)"
if (-not [string]::IsNullOrWhiteSpace($ReleaseTag)) {
    Write-Host "Primary URL: https://github.com/LeoLin4258/rosetta-assets/releases/download/$ReleaseTag/$ArchiveName"
    Write-Host "Mirror URL: https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/$ReleaseTag/$ArchiveName"
}
