# CPU of a test copy (the app and every WebView2 process under it), as % of one core, over N s.
# The W2 gates (docs/plans/PLAN-REVIEW-FIXES.md): idle with the window shown under 3%, playing
# shown under 10%, minimised under 1% (playing minimised: the page's own part under 1%; the rest
# is the engine decoding). -Split also shows each process.
#   pwsh scripts/qa/cpu.ps1 -Seconds 20 -Exe '*\venuta-qa-target\debug\app.exe' -Split
param(
  [int]$Seconds = 10,
  # Which app.exe is the test copy (a wildcard on its path).
  [string]$Exe = '*\target\debug\app.exe',
  [switch]$Split
)
$all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, ExecutablePath
$root = $all | Where-Object { $_.Name -eq 'app.exe' -and $_.ExecutablePath -like $Exe } | Select-Object -First 1
if (-not $root) { "no test copy at $Exe"; exit 1 }
$ids = New-Object System.Collections.Generic.HashSet[int]
[void]$ids.Add([int]$root.ProcessId)
do {
  $added = 0
  foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added++ } }
} while ($added -gt 0)

$before = @{}
foreach ($i in $ids) { $p = Get-Process -Id $i -ErrorAction SilentlyContinue; if ($p) { $before[$i] = $p.TotalProcessorTime.TotalMilliseconds } }
Start-Sleep -Seconds $Seconds
$total = 0.0
$rows = @()
foreach ($i in $ids) {
  $p = Get-Process -Id $i -ErrorAction SilentlyContinue
  if (-not ($p -and $before.ContainsKey($i))) { continue }
  $used = $p.TotalProcessorTime.TotalMilliseconds - $before[$i]
  $total += $used
  if ($Split) {
    $type = (Get-CimInstance Win32_Process -Filter "ProcessId=$i").CommandLine -replace '.*--type=([a-z-]+).*', '$1'
    if ($type.Length -gt 40) { $type = 'main' }
    $rows += '{0,-22} {1,-14} {2,6:N1}%' -f $p.ProcessName, $type, ($used / ($Seconds * 10))
  }
}
'{0:N1}% of a core over {1}s ({2} processes)' -f ($total / ($Seconds * 10)), $Seconds, $ids.Count
$rows
