param(
  [string]$PythonRoot,
  [string]$OutputArchive = "$env:TEMP\rosetta-pdf2zh-windows-amd64.zip",
  [string]$WorkDir = "$env:TEMP\rosetta-pdf2zh-windows-amd64-pack",
  [string]$Pdf2zhVersion = "1.7.9",
  [string]$DoclayoutModelFile = "",
  [string]$DoclayoutModelUrl = "https://huggingface.co/juliozhao/DocLayout-YOLO-DocStructBench/resolve/main/doclayout_yolo_docstructbench_imgsz1024.pt"
)

$ErrorActionPreference = "Stop"

if (-not $PythonRoot) {
  throw "Pass -PythonRoot pointing to a redistributable Windows Python directory."
}

$pythonExe = Join-Path $PythonRoot "python.exe"
if (-not (Test-Path $pythonExe)) {
  throw "Python executable not found: $pythonExe"
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$packRoot = Join-Path $WorkDir "windows-amd64"
$packPython = Join-Path $packRoot "python"
$modelsDir = Join-Path $packRoot "models"
$modelName = "doclayout_yolo_docstructbench_imgsz1024.pt"
$modelPath = Join-Path $modelsDir $modelName

Remove-Item -Recurse -Force $WorkDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $packPython, $modelsDir | Out-Null

Write-Host "[pdf2zh-pack] copying Python root"
Copy-Item -Recurse -Force (Join-Path $PythonRoot "*") $packPython
$packPythonExe = Join-Path $packPython "python.exe"

Write-Host "[pdf2zh-pack] installing pdf2zh==$Pdf2zhVersion"
& $packPythonExe -m pip install --upgrade pip
& $packPythonExe -m pip install "pdf2zh==$Pdf2zhVersion"

$patchScript = Join-Path $scriptDir "patch-pdf2zh-color-preservation.py"
if (Test-Path $patchScript) {
  Write-Host "[pdf2zh-pack] applying Rosetta pdf2zh patch"
  & $packPythonExe $patchScript
}

if ($DoclayoutModelFile) {
  Write-Host "[pdf2zh-pack] copying DocLayout model"
  Copy-Item -Force $DoclayoutModelFile $modelPath
} else {
  Write-Host "[pdf2zh-pack] downloading DocLayout model"
  Invoke-WebRequest -Uri $DoclayoutModelUrl -OutFile $modelPath
}

if (-not (Test-Path $modelPath)) {
  throw "DocLayout model missing after staging: $modelPath"
}

Write-Host "[pdf2zh-pack] removing Python bytecode caches"
Get-ChildItem -Path $packRoot -Recurse -Directory -Filter "__pycache__" | Remove-Item -Recurse -Force
Get-ChildItem -Path $packRoot -Recurse -File -Filter "*.pyc" | Remove-Item -Force

Remove-Item -Force $OutputArchive -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Split-Path -Parent $OutputArchive) | Out-Null

Write-Host "[pdf2zh-pack] archiving $OutputArchive"
Compress-Archive -Path $packRoot -DestinationPath $OutputArchive -Force

$item = Get-Item $OutputArchive
$sha = (Get-FileHash -Algorithm SHA256 $OutputArchive).Hash.ToLowerInvariant()
Write-Host "[pdf2zh-pack] archive ready: $OutputArchive"
Write-Host "[pdf2zh-pack] size bytes: $($item.Length)"
Write-Host "[pdf2zh-pack] sha256: $sha"
