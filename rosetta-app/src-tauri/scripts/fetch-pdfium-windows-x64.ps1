param(
    [string]$PdfiumTag = "chromium/7834",
    [string]$ExpectedSha256 = "0abfacf8aacc919f98eff2c3efa2927c3dc9faf07e31f22558a1f1cf93809612"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$env:PATH = [System.Environment]::GetEnvironmentVariable('PATH', 'Machine')
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$SrcTauriDir = (Resolve-Path "$ScriptDir\\..").Path
$TargetDir = Join-Path $SrcTauriDir "resources\\pdf-sidecar\\pdfium\\win-x64"
$EscapedTag = $PdfiumTag.Replace("/", "%2F")
$Url = "https://github.com/bblanchon/pdfium-binaries/releases/download/$EscapedTag/pdfium-win-x64.tgz"
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) "rosetta-pdfium-$PID"
$Archive = Join-Path $TempRoot "pdfium-win-x64.tgz"
$Extracted = Join-Path $TempRoot "extracted"

try {
    New-Item -ItemType Directory -Path $Extracted -Force | Out-Null
    Invoke-WebRequest -Uri $Url -OutFile $Archive -UseBasicParsing
    $ActualSha = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($ActualSha -ne $ExpectedSha256) {
        throw "pdfium archive SHA256 mismatch: expected $ExpectedSha256, got $ActualSha"
    }
    tar -xzf $Archive -C $Extracted
    if ($LASTEXITCODE -ne 0) { throw "Failed to extract pdfium archive" }

    $Dll = Get-ChildItem -LiteralPath $Extracted -Recurse -File -Filter "pdfium.dll" |
        Select-Object -First 1
    if (-not $Dll) { throw "pdfium.dll not found in archive" }
    $License = Get-ChildItem -LiteralPath $Extracted -Recurse -File |
        Where-Object { $_.Name -like "LICENSE*" } |
        Select-Object -First 1

    New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
    Copy-Item -LiteralPath $Dll.FullName -Destination (Join-Path $TargetDir "pdfium.dll") -Force
    if ($License) {
        Copy-Item -LiteralPath $License.FullName -Destination (Join-Path $TargetDir "LICENSE.pdfium") -Force
    }
    Set-Content -LiteralPath (Join-Path $TargetDir "VERSION") -Value $PdfiumTag -NoNewline
    Write-Host "Installed pdfium.dll to $TargetDir"
} finally {
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force
    }
}
