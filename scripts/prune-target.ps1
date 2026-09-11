<#
.SYNOPSIS
    Prune the cargo `target/` directory of artifacts cargo never reclaims itself.

.DESCRIPTION
    Cargo garbage-collects its global registry cache, but nothing in `target/`:
    the incremental cache grows without bound, and every rebuild of a workspace
    crate leaves the previous `lib<crate>-<hash>.rlib` behind forever. On
    2026-09-11 that was 53 GB of `debug/incremental` and twelve copies of
    `libschemaic_ui-<hash>.rlib` at ~475 MB each, in a 105 GB target/.

    The debug-info axis is already handled by the dev profile in the root
    Cargo.toml (`line-tables-only`, and off entirely for dependencies); this
    script handles the accumulation axis. Three things go:

      * `debug/incremental` — pure scratch, but only once it exceeds
        -IncrementalMaxGB, so a normal working week keeps its speedup.
      * superseded hashed artifacts in `{debug,release}/deps` — the newest
        -KeepPerStem copies of each crate stem survive (two by default: a crate
        commonly has both a lib and a test-harness hash live at once). A
        dependency built once has a single copy and is never a candidate.
      * `llvm-cov-target` — a coverage run's private target dir, once stale.

    Nothing here can corrupt the tree: cargo rebuilds any output it cannot find.
    The cost of over-pruning is compile time, never correctness.

.PARAMETER KeepPerStem
    Hashed copies of each crate artifact to keep in deps/. Default 2.

.PARAMETER IncrementalMaxGB
    Leave debug/incremental alone below this size. Default 10.

.PARAMETER CoverageMaxAgeDays
    Leave llvm-cov-target alone if touched more recently. Default 7.

.PARAMETER DryRun
    Report what would be freed and delete nothing.

.PARAMETER Force
    Prune even while a build is running. Don't.

.EXAMPLE
    pwsh scripts/prune-target.ps1 -DryRun
#>
[CmdletBinding()]
param(
    [int]$KeepPerStem = 2,
    [double]$IncrementalMaxGB = 10,
    [int]$CoverageMaxAgeDays = 7,
    [switch]$DryRun,
    [switch]$Force,
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'

$root   = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$target = Join-Path $root 'target'
$log    = Join-Path $env:LOCALAPPDATA 'schemaic-prune.log'

function Say([string]$m) { if (-not $Quiet) { Write-Host $m } }
function GB($bytes) { if ($null -eq $bytes) { return 0.0 } return [math]::Round($bytes / 1GB, 2) }

function Measure-Tree($path) {
    if (-not (Test-Path $path)) { return 0 }
    $m = Get-ChildItem $path -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum
    if ($null -eq $m.Sum) { return 0 }
    return $m.Sum
}

if (-not (Test-Path $target)) { Say 'target/ does not exist; nothing to do.'; return }

# Never delete anything in a directory that is not demonstrably a cargo target dir.
if (-not (Test-Path (Join-Path $target 'CACHEDIR.TAG'))) {
    throw "refusing to prune '$target': no CACHEDIR.TAG, so this is not a cargo target directory"
}

# A build in flight owns these files. rust-analyzer is deliberately not in this
# list -- it is always running, and would mean the task never fires.
if (-not $Force) {
    $busy = Get-Process -Name cargo, rustc, link -ErrorAction SilentlyContinue
    if ($busy) {
        Say ('skipped: a build is running ({0})' -f (($busy.Name | Sort-Object -Unique) -join ', '))
        return
    }
}

$before = Measure-Tree $target
$freed  = 0
$notes  = @()

function Remove-Tree($rel, $why) {
    $p = Join-Path $target $rel
    if (-not (Test-Path $p)) { return 0 }
    $size = Measure-Tree $p
    if ($DryRun) {
        Say ('would remove  {0,-22} {1,7:N2} GB  ({2})' -f $rel, (GB $size), $why)
    } else {
        Remove-Item $p -Recurse -Force -Confirm:$false -ErrorAction Continue
        Say ('removed       {0,-22} {1,7:N2} GB  ({2})' -f $rel, (GB $size), $why)
    }
    return $size
}

# 1. The incremental cache, once it has outgrown its usefulness.
$incr = Join-Path $target 'debug\incremental'
$incrSize = Measure-Tree $incr
if ($incrSize -gt ($IncrementalMaxGB * 1GB)) {
    $freed += Remove-Tree 'debug\incremental' ('over {0} GB' -f $IncrementalMaxGB)
    $notes += 'incremental'
} elseif ($incrSize -gt 0) {
    Say ('kept          debug\incremental      {0,7:N2} GB  (under the {1} GB threshold)' -f (GB $incrSize), $IncrementalMaxGB)
}

# 2. A stale coverage run's private target dir.
$cov = Join-Path $target 'llvm-cov-target'
if (Test-Path $cov) {
    $age = (Get-Date) - (Get-Item $cov).LastWriteTime
    if ($age.TotalDays -gt $CoverageMaxAgeDays) {
        $freed += Remove-Tree 'llvm-cov-target' ('{0:N0} days stale' -f $age.TotalDays)
        $notes += 'coverage'
    } else {
        Say ('kept          llvm-cov-target        {0,7:N2} GB  ({1:N0} days old)' -f (GB (Measure-Tree $cov)), $age.TotalDays)
    }
}

# 3. Superseded hashed artifacts. A crate rebuilt N times leaves N-1 dead copies
#    of every output; group by the stem with the trailing -<16 hex> stripped and
#    drop everything past the newest $KeepPerStem.
$exts = '.exe', '.rlib', '.rmeta', '.pdb', '.d', '.o', '.lib', '.exp', '.dll'
foreach ($rel in 'debug\deps', 'release\deps') {
    $dir = Join-Path $target $rel
    if (-not (Test-Path $dir)) { continue }
    $files = Get-ChildItem $dir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -in $exts -and $_.BaseName -match '-[0-9a-f]{16}$' }
    $drop = $files |
        Group-Object { ($_.BaseName -replace '-[0-9a-f]{16}$', '') + $_.Extension } |
        ForEach-Object { $_.Group | Sort-Object LastWriteTime -Descending | Select-Object -Skip $KeepPerStem }
    if (-not $drop) { Say ('nothing superseded in {0}' -f $rel); continue }
    $size = ($drop | Measure-Object -Property Length -Sum).Sum
    if ($DryRun) {
        Say ('would prune   {0,-22} {1,7:N2} GB  ({2} superseded files)' -f $rel, (GB $size), $drop.Count)
    } else {
        $drop | Remove-Item -Force -Confirm:$false -ErrorAction Continue
        Say ('pruned        {0,-22} {1,7:N2} GB  ({2} superseded files)' -f $rel, (GB $size), $drop.Count)
    }
    $freed += $size
    $notes += $rel
}

$after = $before - $freed
$verb  = 'freed'
if ($DryRun) { $verb = 'would free' }
Say ''
Say ('{0} {1:N2} GB -- target/ {2:N2} GB -> {3:N2} GB' -f $verb, (GB $freed), (GB $before), (GB $after))

if (-not $DryRun -and $freed -gt 0) {
    $line = '{0}  freed {1,6:N2} GB  {2,6:N2} -> {3,6:N2} GB  [{4}]' -f (Get-Date -Format 'yyyy-MM-dd HH:mm'), (GB $freed), (GB $before), (GB $after), ($notes -join ' ')
    Add-Content -Path $log -Value $line -Encoding utf8
}
