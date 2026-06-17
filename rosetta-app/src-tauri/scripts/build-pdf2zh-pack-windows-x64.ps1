# Build a release-ready PDF layout component pack for Windows x64.
#
# Usage from rosetta-app/:
#
#   powershell -ExecutionPolicy Bypass -File src-tauri/scripts/build-pdf2zh-pack-windows-x64.ps1
#
# Output:
#
#   dist/pdf-layout/rosetta-pdf2zh-windows-x64.tar.gz
#   dist/pdf-layout/manifest.json
#
# After the build, upload to a GitHub Release under LeoLin4258/rosetta-assets
# with tag pdf-layout-pack-windows-x64-vYYYY.MM.DD.N, then pin
# pack_download_urls / pack_sha256 / pack_size_bytes in profile.rs.

param(
    [string]$Pdf2zhVersion = "1.7.9",
    [string]$PbsRelease = "20260510",
    [string]$PbsPythonVersion = "3.13.13"
)

$ErrorActionPreference = "Stop"

$PbsUrl = "https://github.com/astral-sh/python-build-standalone/releases/download/${PbsRelease}/cpython-${PbsPythonVersion}+${PbsRelease}-x86_64-pc-windows-msvc-install_only.tar.gz"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path "$ScriptDir/../..").Path
$DistDir = Join-Path $RepoRoot "dist/pdf-layout"
$ArchiveName = "rosetta-pdf2zh-windows-x64.tar.gz"
$ArchivePath = Join-Path $DistDir $ArchiveName

$BuildRoot = Join-Path ([System.IO.Path]::GetTempPath()) "rosetta-pdf2zh-build-$([System.Diagnostics.Process]::GetCurrentProcess().Id)"
if (Test-Path $BuildRoot) { Remove-Item -Recurse -Force $BuildRoot }

$PackDir = Join-Path $BuildRoot "windows-x64"
$PythonDir = Join-Path $PackDir "python"
$PbsTarball = Join-Path $BuildRoot "pbs.tar.gz"

Write-Host "[pdf2zh-release] building pdf2zh==$Pdf2zhVersion for windows-x64"
Write-Host "[pdf2zh-release] PBS python: $PbsPythonVersion (release $PbsRelease)"
Write-Host "[pdf2zh-release] build root: $BuildRoot"

New-Item -ItemType Directory -Force -Path $PackDir | Out-Null

# Check for a local copy first (e.g. manually downloaded to ~/Downloads)
$PbsLocalName = "cpython-${PbsPythonVersion}+${PbsRelease}-x86_64-pc-windows-msvc-install_only.tar.gz"
$PbsLocalPath = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Downloads/$PbsLocalName"
if (Test-Path $PbsLocalPath) {
    Write-Host "[pdf2zh-release] using local PBS tarball: $PbsLocalPath"
    Copy-Item $PbsLocalPath $PbsTarball
} else {
    Write-Host "[pdf2zh-release] downloading python-build-standalone"
    Write-Host "  $PbsUrl"
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $PbsUrl -OutFile $PbsTarball -UseBasicParsing
}

Write-Host "[pdf2zh-release] extracting CPython into pack"
tar -xzf $PbsTarball -C $PackDir

$PythonExe = Join-Path $PythonDir "python.exe"
if (-not (Test-Path $PythonExe)) {
    Write-Error "PBS tarball did not produce expected python/python.exe under $PackDir"
    exit 1
}

$PbsReportedVersion = & $PythonExe -c "import sys; print('.'.join(map(str, sys.version_info[:3])))"
Write-Host "[pdf2zh-release] PBS python ready: $PbsReportedVersion"

$PipMirror = "https://mirrors.tuna.tsinghua.edu.cn/pypi/web/simple"
Write-Host "[pdf2zh-release] installing pdf2zh==$Pdf2zhVersion into pack python (mirror: $PipMirror)"
& $PythonExe -m pip install --upgrade pip --quiet -i $PipMirror --trusted-host mirrors.tuna.tsinghua.edu.cn
& $PythonExe -m pip install "pdf2zh==$Pdf2zhVersion" --quiet -i $PipMirror --trusted-host mirrors.tuna.tsinghua.edu.cn

Write-Host "[pdf2zh-release] applying NumPy 2 compatibility patch"
$NumpyPatch = @'
from pathlib import Path
import pdf2zh
root = Path(pdf2zh.__file__).resolve().parent
target = root / "high_level.py"
text = target.read_text()
old = "np.fromstring(pix.samples, np.uint8)"
new = "np.frombuffer(pix.samples, np.uint8)"
if old in text:
    target.write_text(text.replace(old, new))
    print(f"[pdf2zh-release] patched {target}")
elif new in text:
    print(f"[pdf2zh-release] patch already present in {target}")
else:
    raise SystemExit(f"could not find expected NumPy call in {target}")
'@
$NumpyPatch | & $PythonExe -

Write-Host "[pdf2zh-release] applying PDF color preservation patch"
$ColorPatchScript = Join-Path $ScriptDir "patch-pdf2zh-color-preservation.py"
& $PythonExe $ColorPatchScript

# --- DocLayout-YOLO model ---
$DocLayoutModelFilename = "doclayout_yolo_docstructbench_imgsz1024.pt"
$DocLayoutModelUrl = "https://huggingface.co/juliozhao/DocLayout-YOLO-DocStructBench/resolve/main/$DocLayoutModelFilename"
$DocLayoutModelMirror = "https://hf-mirror.com/juliozhao/DocLayout-YOLO-DocStructBench/resolve/main/$DocLayoutModelFilename"
$ModelsDir = Join-Path $PackDir "models"
New-Item -ItemType Directory -Force -Path $ModelsDir | Out-Null
$DocLayoutModelPath = Join-Path $ModelsDir $DocLayoutModelFilename

$DocLayoutLocalPath = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Downloads/$DocLayoutModelFilename"
if (Test-Path $DocLayoutLocalPath) {
    Write-Host "[pdf2zh-release] using local DocLayout-YOLO model: $DocLayoutLocalPath"
    Copy-Item $DocLayoutLocalPath $DocLayoutModelPath
} else {
    Write-Host "[pdf2zh-release] downloading DocLayout-YOLO model"
    Write-Host "  $DocLayoutModelUrl"
    $ProgressPreference = 'SilentlyContinue'
    try {
        Invoke-WebRequest -Uri $DocLayoutModelUrl -OutFile $DocLayoutModelPath -UseBasicParsing
    } catch {
        Write-Host "[pdf2zh-release] primary URL failed, trying hf-mirror"
        Write-Host "  $DocLayoutModelMirror"
        Invoke-WebRequest -Uri $DocLayoutModelMirror -OutFile $DocLayoutModelPath -UseBasicParsing
    }
}
if (-not (Test-Path $DocLayoutModelPath) -or (Get-Item $DocLayoutModelPath).Length -eq 0) {
    Write-Error "DocLayout-YOLO model was not staged at $DocLayoutModelPath"
    exit 1
}
Write-Host "[pdf2zh-release] DocLayout-YOLO model ready: $((Get-Item $DocLayoutModelPath).Length / 1MB) MB"

Write-Host "[pdf2zh-release] patching pdf2zh to prefer bundled DocLayout-YOLO model"
$DocLayoutPatch = @'
from pathlib import Path
import os, pdf2zh

target = Path(pdf2zh.__file__).resolve().parent / "pdf2zh.py"
text = target.read_text()
old = """    pth = hf_hub_download(
        repo_id="juliozhao/DocLayout-YOLO-DocStructBench",
        filename="doclayout_yolo_docstructbench_imgsz1024.pt",
    )
    model = doclayout_yolo.YOLOv10(pth)
"""
new = """    pth = os.environ.get("ROSETTA_DOCLAYOUT_MODEL")
    if not pth or not os.path.isfile(pth):
        pth = hf_hub_download(
            repo_id="juliozhao/DocLayout-YOLO-DocStructBench",
            filename="doclayout_yolo_docstructbench_imgsz1024.pt",
        )
    model = doclayout_yolo.YOLOv10(pth)
"""
if old in text:
    target.write_text(text.replace(old, new))
    print(f"[pdf2zh-release] patched {target}")
elif new in text:
    print(f"[pdf2zh-release] patch already present in {target}")
else:
    raise SystemExit(f"could not patch DocLayout-YOLO download in {target}")
'@
$DocLayoutPatch | & $PythonExe -

Write-Host "[pdf2zh-release] smoke test:"
& $PythonExe -m pdf2zh.pdf2zh --version

Write-Host "[pdf2zh-release] removing Python bytecode caches (stdlib + site-packages + smoke-test generated)"
do {
    $Dirs = Get-ChildItem -Path $PackDir -Recurse -Directory -Filter "__pycache__" -ErrorAction SilentlyContinue
    $Dirs | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    $Files = Get-ChildItem -Path $PackDir -Recurse -File -Filter "*.pyc" -ErrorAction SilentlyContinue
    $Files | Remove-Item -Force -ErrorAction SilentlyContinue
} while ($Dirs -or $Files)

Write-Host "[pdf2zh-release] verifying no stale bytecode:"
$Stale = Get-ChildItem -Path $PackDir -Recurse -Include "__pycache__","*.pyc" -ErrorAction SilentlyContinue
if ($Stale) {
    Write-Error "stale Python bytecode found after scrub: $($Stale | Select-Object -First 5)"
    exit 1
}

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
if (Test-Path $ArchivePath) { Remove-Item -Force $ArchivePath }

Write-Host "[pdf2zh-release] archiving to: $ArchivePath"
tar -czf $ArchivePath -C $BuildRoot "windows-x64"

$SizeBytes = (Get-Item $ArchivePath).Length
$SHA256 = (Get-FileHash -Path $ArchivePath -Algorithm SHA256).Hash.ToLower()

$Manifest = @{
    profile_id     = "windows-x64-pdf2zh"
    pack_filename  = $ArchiveName
    pdf2zh_version = $Pdf2zhVersion
    python_runtime = "python-build-standalone $PbsPythonVersion (release $PbsRelease)"
    sha256         = $SHA256
    size_bytes     = $SizeBytes
    built_at       = ([DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ"))
} | ConvertTo-Json
Set-Content -Path (Join-Path $DistDir "manifest.json") -Value $Manifest -Encoding UTF8

Write-Host ""
Write-Host "[pdf2zh-release] done."
Write-Host "[pdf2zh-release] archive:     $ArchivePath"
Write-Host "[pdf2zh-release] size bytes:  $SizeBytes"
Write-Host "[pdf2zh-release] sha256:      $SHA256"
Write-Host ""
Write-Host "[pdf2zh-release] next steps:"
$Tag = "pdf-layout-pack-windows-x64-v$(Get-Date -Format 'yyyy.MM.dd').1"
Write-Host "  1. Create a GitHub Release under LeoLin4258/rosetta-assets"
Write-Host "     tag: $Tag"
Write-Host "  2. Upload: $ArchivePath"
Write-Host "  3. Pin in src-tauri/src/managed_pdf2zh/profile.rs:"
Write-Host "     pack_size_bytes: Some($SizeBytes),"
Write-Host "     pack_sha256: Some(`"$SHA256`"),"
Write-Host "     pack_download_urls: &[`"https://github.com/LeoLin4258/rosetta-assets/releases/download/$Tag/$ArchiveName`"],"

# Cleanup
Remove-Item -Recurse -Force $BuildRoot -ErrorAction SilentlyContinue
