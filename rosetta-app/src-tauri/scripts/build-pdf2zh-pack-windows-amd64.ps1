param(
    [string]$Pdf2zhVersion = "1.9.11",
    [string]$Pdf2zhSourcePath = "",
    [string]$PythonVersion = "3.12.13",
    [string]$PythonBuildRelease = "20260602",
    [string]$PythonUrl = "",
    [string]$PipIndexUrl = "https://pypi.org/simple",
    [string]$ModelFile = "",
    [string]$ModelUrl = "https://huggingface.co/wybxc/DocLayout-YOLO-DocStructBench-onnx/resolve/main/doclayout_yolo_docstructbench_imgsz1024.onnx?download=true"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# Reset PATH to system-only so Git Bash's /usr/bin/tar doesn't shadow
# Windows tar.exe (Git tar can't parse Windows drive letters in paths).
$env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine")

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$SrcTauriDir = (Resolve-Path "$ScriptDir\..").Path
$AppDir = (Resolve-Path "$SrcTauriDir\..").Path
$RepoRoot = (Resolve-Path "$AppDir\..").Path
$DistDir = Join-Path $AppDir "dist\pdf-layout"
$ArchiveName = "rosetta-pdf2zh-windows-amd64.zip"
$ArchivePath = Join-Path $DistDir $ArchiveName
$Requirements = Join-Path $ScriptDir "requirements-pdf2zh-windows-amd64.txt"
$ModelName = "doclayout_yolo_docstructbench_imgsz1024.onnx"

if (-not $Pdf2zhSourcePath) {
    $Pdf2zhSourcePath = Join-Path (Split-Path -Parent $RepoRoot) "PDFMathTranslate"
}
$Pdf2zhSourcePath = [IO.Path]::GetFullPath($Pdf2zhSourcePath)
if (-not (Test-Path -LiteralPath (Join-Path $Pdf2zhSourcePath "pyproject.toml"))) {
    throw "PDFMathTranslate source checkout not found: $Pdf2zhSourcePath"
}

if (-not $PythonUrl) {
    $PythonArchiveName = "cpython-$PythonVersion+$PythonBuildRelease-x86_64-pc-windows-msvc-install_only.tar.gz"
    $PythonUrl = "https://github.com/astral-sh/python-build-standalone/releases/download/$PythonBuildRelease/$PythonArchiveName"
} else {
    $PythonArchiveName = Split-Path -Leaf $PythonUrl
}

$BuildRoot = Join-Path ([IO.Path]::GetTempPath()) "rosetta-pdf2zh-windows-amd64-$PID"
$ResolvedBuildRoot = [IO.Path]::GetFullPath($BuildRoot)
$ResolvedTempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
if (-not $ResolvedBuildRoot.StartsWith($ResolvedTempRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Unsafe build root: $ResolvedBuildRoot"
}
if (Test-Path -LiteralPath $ResolvedBuildRoot) {
    Remove-Item -LiteralPath $ResolvedBuildRoot -Recurse -Force
}

$PackDir = Join-Path $ResolvedBuildRoot "windows-amd64"
$PythonDir = Join-Path $PackDir "python"
$PythonArchive = Join-Path $ResolvedBuildRoot $PythonArchiveName

function Invoke-NativeChecked {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments
    )
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $FilePath $($Arguments -join ' ')"
    }
}

try {
    New-Item -ItemType Directory -Path $PackDir -Force | Out-Null
    New-Item -ItemType Directory -Path $DistDir -Force | Out-Null

    $LocalPythonArchive = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Downloads\$PythonArchiveName"
    if (Test-Path -LiteralPath $LocalPythonArchive) {
        Write-Host "[pdf2zh-pack] using cached Python runtime: $LocalPythonArchive"
        Copy-Item -LiteralPath $LocalPythonArchive -Destination $PythonArchive
    } else {
        Write-Host "[pdf2zh-pack] downloading Python runtime: $PythonUrl"
        Invoke-WebRequest -Uri $PythonUrl -OutFile $PythonArchive -UseBasicParsing
        Copy-Item -LiteralPath $PythonArchive -Destination $LocalPythonArchive -Force
    }
    Invoke-NativeChecked tar -xzf $PythonArchive -C $PackDir

    $PythonExe = Join-Path $PythonDir "python.exe"
    if (-not (Test-Path -LiteralPath $PythonExe)) {
        throw "Python archive did not produce python\python.exe"
    }
    $ReportedPython = & $PythonExe -c "import sys; print('.'.join(map(str, sys.version_info[:3])))"
    Write-Host "[pdf2zh-pack] Python ready: $ReportedPython"

    Invoke-NativeChecked $PythonExe -m pip install --upgrade "pip==26.1.2" --index-url $PipIndexUrl
    Invoke-NativeChecked $PythonExe -m pip install --requirement $Requirements --index-url $PipIndexUrl
    Invoke-NativeChecked $PythonExe -m pip install $Pdf2zhSourcePath --no-deps --index-url $PipIndexUrl

    $ModelsDir = Join-Path $PackDir "models"
    New-Item -ItemType Directory -Path $ModelsDir -Force | Out-Null
    $ModelPath = Join-Path $ModelsDir $ModelName
    $DefaultBabeldocCacheModel = Join-Path ([Environment]::GetFolderPath("UserProfile")) ".cache\babeldoc\models\$ModelName"
    $LocalDownloadModel = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Downloads\$ModelName"
    if ($ModelFile -and (Test-Path -LiteralPath $ModelFile)) {
        Write-Host "[pdf2zh-pack] copying ONNX layout model: $ModelFile"
        Copy-Item -LiteralPath $ModelFile -Destination $ModelPath
    } elseif (Test-Path -LiteralPath $DefaultBabeldocCacheModel) {
        Write-Host "[pdf2zh-pack] copying cached ONNX layout model: $DefaultBabeldocCacheModel"
        Copy-Item -LiteralPath $DefaultBabeldocCacheModel -Destination $ModelPath
    } elseif (Test-Path -LiteralPath $LocalDownloadModel) {
        Write-Host "[pdf2zh-pack] copying downloaded ONNX layout model: $LocalDownloadModel"
        Copy-Item -LiteralPath $LocalDownloadModel -Destination $ModelPath
    } else {
        Write-Host "[pdf2zh-pack] downloading ONNX layout model: $ModelUrl"
        Invoke-WebRequest -Uri $ModelUrl -OutFile $ModelPath -UseBasicParsing
    }
    if (-not (Test-Path -LiteralPath $ModelPath) -or (Get-Item -LiteralPath $ModelPath).Length -le 0) {
        throw "ONNX layout model was not staged at $ModelPath"
    }

    $Smoke = @'
import os
import tempfile

import numpy
import pymupdf
import pdfminer
import pdf2zh
from pdf2zh.converter import TranslateConverter
from pdf2zh.doclayout import OnnxModel
from pdf2zh.translator import RosettaBatchTranslator

model_path = os.environ["ROSETTA_DOCLAYOUT_MODEL"]
model = OnnxModel(model_path)
providers = ",".join(model.model.get_providers())
print(f"pdf-pack-imports-ok pdf2zh={pdf2zh.__version__} providers={providers}")
'@
    $env:ROSETTA_DOCLAYOUT_MODEL = $ModelPath
    $Smoke | & $PythonExe -
    if ($LASTEXITCODE -ne 0) {
        throw "PDF runtime import smoke test failed"
    }
    Invoke-NativeChecked $PythonExe -m pdf2zh.pdf2zh --version

    Get-ChildItem -LiteralPath $PackDir -Recurse -Directory -Filter "__pycache__" |
        Remove-Item -Recurse -Force
    Get-ChildItem -LiteralPath $PackDir -Recurse -File -Filter "*.pyc" |
        Remove-Item -Force
    foreach ($name in @("include", "libs", "tcl")) {
        $path = Join-Path $PythonDir $name
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Recurse -Force
        }
    }
    Get-ChildItem -LiteralPath $PythonDir -File -Filter "*.pdb" | Remove-Item -Force
    $SitePackages = Join-Path $PythonDir "Lib\site-packages"
    if (Test-Path -LiteralPath $SitePackages) {
        Get-ChildItem -LiteralPath $SitePackages -Recurse -Directory |
            Where-Object { $_.Name -in @("tests", "test", "__pycache__") } |
            Remove-Item -Recurse -Force
    }

    $env:ROSETTA_DOCLAYOUT_MODEL = $ModelPath
    $Smoke | & $PythonExe -
    if ($LASTEXITCODE -ne 0) {
        throw "Pruned PDF runtime import smoke test failed"
    }
    Remove-Item -LiteralPath "$ModelPath.optimized" -Force -ErrorAction SilentlyContinue

    if (Test-Path -LiteralPath $ArchivePath) {
        Remove-Item -LiteralPath $ArchivePath -Force
    }
    & tar -a -cf $ArchivePath -C $ResolvedBuildRoot "windows-amd64"
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to create ZIP"
    }

    $Size = (Get-Item -LiteralPath $ArchivePath).Length
    $Sha = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    $ModelSha = (Get-FileHash -LiteralPath $ModelPath -Algorithm SHA256).Hash.ToLowerInvariant()
    [ordered]@{
        profileId = "windows-amd64-pdf2zh"
        packFilename = $ArchiveName
        pdf2zhVersion = $Pdf2zhVersion
        pdf2zhSourcePath = $Pdf2zhSourcePath
        pythonVersion = $PythonVersion
        pythonBuildRelease = $PythonBuildRelease
        layoutModel = $ModelName
        layoutModelSha256 = $ModelSha
        sizeBytes = $Size
        sha256 = $Sha
        builtAt = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $DistDir "windows-amd64-manifest.json") -Encoding utf8

    Write-Host "Archive: $ArchivePath"
    Write-Host "Size: $Size"
    Write-Host "SHA256: $Sha"
} finally {
    Remove-Item Env:\ROSETTA_DOCLAYOUT_MODEL -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $ResolvedBuildRoot) {
        Remove-Item -LiteralPath $ResolvedBuildRoot -Recurse -Force
    }
}
