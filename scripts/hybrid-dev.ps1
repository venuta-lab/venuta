# Hybrid online+local UI test launcher for Venuta.
# Isolated library + live catalog. Safe to exercise the real UI.
#
# Usage (from repo root):
#   pwsh -File scripts/hybrid-dev.ps1
#   pwsh -File scripts/hybrid-dev.ps1 -Fake   # fixture catalog only (no network)
#
# Live catalog is the app default. This script mainly sets an isolated test library.
# Stop with Ctrl+C in this terminal.

param(
  [switch]$Fake
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$Lib = "D:\UIImprovement\_venuta-hybrid-test-library"
New-Item -ItemType Directory -Force -Path (Join-Path $Lib "Music"), (Join-Path $Lib ".venuta") | Out-Null
$marker = Join-Path $Lib ".venuta-library"
if (-not (Test-Path $marker)) {
  Set-Content -Path $marker -Value "A library built by Venuta.`n" -NoNewline
}

$env:VENUTA_HOME = $Lib

# The Opus decoder (libopus) builds with CMake. Visual Studio's C++ build tools include one;
# use it when no CMake is on PATH.
if (-not (Get-Command cmake -ErrorAction SilentlyContinue) -and -not $env:CMAKE) {
  $vsCmake = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft Visual Studio", "$env:ProgramFiles\Microsoft Visual Studio" -Recurse -Filter cmake.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -like "*CMake\bin\cmake.exe" } | Select-Object -First 1
  if ($vsCmake) { $env:CMAKE = $vsCmake.FullName; Write-Host "CMake: $($vsCmake.FullName)" }
}

if ($Fake) {
  $env:VENUTA_FAKE_CATALOG = "1"
  Remove-Item Env:VENUTA_LIVE_CATALOG -ErrorAction SilentlyContinue
  Write-Host "Catalog: FakeMediaAccess (fixture)"
} else {
  Remove-Item Env:VENUTA_FAKE_CATALOG -ErrorAction SilentlyContinue
  $env:VENUTA_LIVE_CATALOG = "1"
  Write-Host "Catalog: LIVE (default; VENUTA_LIVE_CATALOG=1)"
}

# Prefetch stream tool into the same managed path the app uses (test == release behavior).
if (-not $Fake) {
  try {
    node (Join-Path $Root "scripts/fetch-stream-tool.mjs")
  } catch {
    Write-Host "Stream tool prefetch skipped: $_ (app will try again on start)"
  }
}

Write-Host "Library:  $Lib"
# Free Vite default port if a leftover dev server is holding it.
foreach ($port in 5173) {
  $conns = Get-NetTCPConnection -LocalPort $port -ErrorAction SilentlyContinue |
    Where-Object { $_.State -eq 'Listen' }
  foreach ($c in $conns) {
    $proc = Get-Process -Id $c.OwningProcess -ErrorAction SilentlyContinue
    if ($proc -and $proc.ProcessName -match 'node|vite') {
      Write-Host "Stopping leftover $($proc.ProcessName) PID $($proc.Id) on port $port"
      Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    }
  }
}
Start-Sleep -Seconds 1
Write-Host "Starting tauri dev in app/ ..."
Set-Location (Join-Path $Root "app")
pnpm tauri dev
