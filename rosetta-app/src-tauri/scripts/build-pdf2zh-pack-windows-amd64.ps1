param(
    [string]$Pdf2zhVersion = "1.7.9",
    [string]$PythonVersion = "3.13.13",
    [string]$PythonBuildRelease = "20260510",
    [string]$PipIndexUrl = "https://pypi.org/simple"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# Reset PATH to system-only so Git Bash's /usr/bin/tar doesn't shadow
# Windows tar.exe (Git tar can't parse Windows drive letters in paths).
$env:PATH = [System.Environment]::GetEnvironmentVariable('PATH', 'Machine')

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$SrcTauriDir = (Resolve-Path "$ScriptDir\\..").Path
$AppDir = (Resolve-Path "$SrcTauriDir\\..").Path
$DistDir = Join-Path $AppDir "dist\\pdf-layout"
$ArchiveName = "rosetta-pdf2zh-windows-amd64.zip"
$ArchivePath = Join-Path $DistDir $ArchiveName
$Requirements = Join-Path $ScriptDir "requirements-pdf2zh-windows-amd64.txt"
$ModelName = "doclayout_yolo_docstructbench_imgsz1024.pt"
$ModelUrl = "https://huggingface.co/juliozhao/DocLayout-YOLO-DocStructBench/resolve/main/$ModelName"
$PythonArchiveName = "cpython-$PythonVersion+$PythonBuildRelease-x86_64-pc-windows-msvc-install_only.tar.gz"
$PythonUrl = "https://github.com/astral-sh/python-build-standalone/releases/download/$PythonBuildRelease/$PythonArchiveName"

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

try {
    New-Item -ItemType Directory -Path $PackDir -Force | Out-Null
    New-Item -ItemType Directory -Path $DistDir -Force | Out-Null

    $LocalPythonArchive = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Downloads\\$PythonArchiveName"
    if (Test-Path -LiteralPath $LocalPythonArchive) {
        Copy-Item -LiteralPath $LocalPythonArchive -Destination $PythonArchive
    } else {
        Invoke-WebRequest -Uri $PythonUrl -OutFile $PythonArchive -UseBasicParsing
    }
    tar -xzf $PythonArchive -C $PackDir
    if ($LASTEXITCODE -ne 0) { throw "Failed to extract Python runtime" }

    $PythonExe = Join-Path $PythonDir "python.exe"
    if (-not (Test-Path -LiteralPath $PythonExe)) {
        throw "Python archive did not produce python\\python.exe"
    }

    & $PythonExe -m pip install --upgrade "pip==26.1.2" --index-url $PipIndexUrl
    if ($LASTEXITCODE -ne 0) { throw "Failed to install pinned pip" }
    & $PythonExe -m pip install --requirement $Requirements --index-url $PipIndexUrl
    if ($LASTEXITCODE -ne 0) { throw "Failed to install PDF runtime requirements" }
    & $PythonExe -m pip install "pdf2zh==$Pdf2zhVersion" --no-deps --index-url $PipIndexUrl
    if ($LASTEXITCODE -ne 0) { throw "Failed to install pdf2zh" }

    $ModelsDir = Join-Path $PackDir "models"
    New-Item -ItemType Directory -Path $ModelsDir -Force | Out-Null
    $ModelPath = Join-Path $ModelsDir $ModelName
    $LocalModel = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Downloads\\$ModelName"
    if (Test-Path -LiteralPath $LocalModel) {
        Copy-Item -LiteralPath $LocalModel -Destination $ModelPath
    } else {
        Invoke-WebRequest -Uri $ModelUrl -OutFile $ModelPath -UseBasicParsing
    }

    $PatchScript = Join-Path $ScriptDir "patch-pdf2zh-color-preservation.py"
    & $PythonExe $PatchScript
    if ($LASTEXITCODE -ne 0) { throw "Failed to apply pdf2zh color-preservation patch" }

    $DocLayoutPatch = @'
from pathlib import Path
import os, pdf2zh
target = Path(pdf2zh.__file__).resolve().parent / "pdf2zh.py"
text = target.read_text(encoding="utf-8")
old = '''    pth = hf_hub_download(
        repo_id="juliozhao/DocLayout-YOLO-DocStructBench",
        filename="doclayout_yolo_docstructbench_imgsz1024.pt",
    )
    model = doclayout_yolo.YOLOv10(pth)
'''
new = '''    pth = os.environ.get("ROSETTA_DOCLAYOUT_MODEL")
    if not pth or not os.path.isfile(pth):
        pth = hf_hub_download(
            repo_id="juliozhao/DocLayout-YOLO-DocStructBench",
            filename="doclayout_yolo_docstructbench_imgsz1024.pt",
        )
    model = doclayout_yolo.YOLOv10(pth)
'''
if old in text:
    target.write_text(text.replace(old, new), encoding="utf-8")
elif new not in text:
    raise SystemExit("DocLayout model patch target not found")
'@
    $DocLayoutPatch | & $PythonExe -
    if ($LASTEXITCODE -ne 0) { throw "Failed to apply bundled DocLayout model patch" }

    & $PythonExe -c "import fitz, numpy, torch, torchvision, cv2, doclayout_yolo, pdf2zh, tqdm"
    if ($LASTEXITCODE -ne 0) { throw "PDF runtime import smoke test failed" }
    & $PythonExe -m pdf2zh.pdf2zh --version
    if ($LASTEXITCODE -ne 0) { throw "pdf2zh CLI smoke test failed" }

    Get-ChildItem -LiteralPath $PackDir -Recurse -Directory -Filter "__pycache__" |
        Remove-Item -Recurse -Force
    Get-ChildItem -LiteralPath $PackDir -Recurse -File -Filter "*.pyc" |
        Remove-Item -Force
    foreach ($name in @("include", "libs", "tcl")) {
        $path = Join-Path $PythonDir $name
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
    }
    Get-ChildItem -LiteralPath $PythonDir -File -Filter "*.pdb" | Remove-Item -Force
    Get-ChildItem -LiteralPath (Join-Path $PythonDir "Lib\\site-packages") -Recurse -Directory |
        Where-Object { $_.Name -in @("tests", "test") } |
        Remove-Item -Recurse -Force

    # PowerShell does not turn a native executable's non-zero exit code into
    # a terminating error, even with $ErrorActionPreference = "Stop". Re-run
    # the import smoke test after pruning and inspect $LASTEXITCODE explicitly
    # so an incomplete pack can never be archived and uploaded.
    & $PythonExe -c "import fitz, numpy, torch, torchvision, cv2, doclayout_yolo, pdf2zh, tqdm; print('pdf-pack-imports-ok')"
    if ($LASTEXITCODE -ne 0) { throw "Pruned PDF runtime import smoke test failed" }

    if (Test-Path -LiteralPath $ArchivePath) { Remove-Item -LiteralPath $ArchivePath -Force }
    tar -a -cf $ArchivePath -C $ResolvedBuildRoot "windows-amd64"
    if ($LASTEXITCODE -ne 0) { throw "Failed to create ZIP" }

    $Size = (Get-Item -LiteralPath $ArchivePath).Length
    $Sha = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    [ordered]@{
        profileId = "windows-amd64-pdf2zh"
        packFilename = $ArchiveName
        pdf2zhVersion = $Pdf2zhVersion
        pythonVersion = $PythonVersion
        sizeBytes = $Size
        sha256 = $Sha
        builtAt = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $DistDir "windows-amd64-manifest.json") -Encoding utf8

    Write-Host "Archive: $ArchivePath"
    Write-Host "Size: $Size"
    Write-Host "SHA256: $Sha"
} finally {
    if (Test-Path -LiteralPath $ResolvedBuildRoot) {
        Remove-Item -LiteralPath $ResolvedBuildRoot -Recurse -Force
    }
}
