# Install / update the hybrid stream tool (yt-dlp) for Venuta.
# Default: official standalone binary into %LOCALAPPDATA%\Venuta\bin (same path the app uses).
# No Python required. Optional -Python still uses pip.
#
# Usage:
#   pwsh -File scripts/install-stream-tool.ps1
#   pwsh -File scripts/install-stream-tool.ps1 -Check
#   pwsh -File scripts/install-stream-tool.ps1 -Bundle   # also app/src-tauri/resources for local installer builds
#   pwsh -File scripts/install-stream-tool.ps1 -Python   # pip module instead of standalone

param(
  [switch]$Check,
  [switch]$Bundle,
  [switch]$Python
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

function Find-Python {
  foreach ($name in @("python3", "python", "py")) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
  }
  $local = Join-Path $env:LOCALAPPDATA "Python\bin\python3.exe"
  if (Test-Path $local) { return $local }
  return $null
}

function Managed-Binary {
  Join-Path $env:LOCALAPPDATA "Venuta\bin\yt-dlp.exe"
}

if ($Check) {
  $bin = Managed-Binary
  if (Test-Path $bin) {
    Write-Host "Standalone OK: $bin"
    exit 0
  }
  $pathBin = Get-Command yt-dlp -ErrorAction SilentlyContinue
  if ($pathBin) {
    Write-Host "PATH OK: $($pathBin.Source)"
    exit 0
  }
  $py = Find-Python
  if ($py) {
    & $py -c "import yt_dlp" 2>$null
    if ($LASTEXITCODE -eq 0) {
      Write-Host "Python module OK: $py -m yt_dlp"
      exit 0
    }
  }
  Write-Host "Stream tool missing. Run: pwsh -File scripts/install-stream-tool.ps1"
  exit 2
}

if ($Python) {
  $py = Find-Python
  if (-not $py) {
    Write-Host "No Python found. Omit -Python to download the standalone binary instead."
    exit 1
  }
  Write-Host "Using Python: $py"
  Write-Host "Installing/updating yt-dlp…"
  & $py -m pip install -U yt-dlp
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} else {
  $fetchArgs = @()
  if ($Bundle) { $fetchArgs += '--bundle' }
  Write-Host "Fetching standalone yt-dlp (same as app first-run / Settings)…"
  node (Join-Path $Root "scripts/fetch-stream-tool.mjs") @fetchArgs
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host ""
Write-Host "Done. Live catalog is on by default in the app (see scripts/hybrid-dev.ps1)."
Write-Host "Optional pot helper: bgutil on http://127.0.0.1:4416"
Write-Host "Optional binary override: `$env:VENUTA_STREAM_RESOLVER = 'C:\path\to\yt-dlp.exe'"
