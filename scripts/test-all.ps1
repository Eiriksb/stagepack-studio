# Runs every test on Windows: clippy, the workspace's tests, the components' tests with the
# compiler bundle, the release-only timing tests and the packaging tests.
#
#   scripts\test-all.ps1            everything
#   scripts\test-all.ps1 -Quick     skips the release timing step and the slow packaging tests
#   scripts\test-all.ps1 -List      prints the steps and stops
#   scripts\test-all.ps1 -Lax       clippy warnings are only counted, not errors
#
# A failed step doesn't stop the later ones. The summary lists each step as ok or FAILED and
# the script exits 1 if any failed. A step that fails on a test is rerun once on those tests
# alone, because a loaded machine can fail a timed test; both results are printed.
param(
    [switch]$Quick,
    [switch]$List,
    [switch]$Lax
)
Set-StrictMode -Version Latest
# Each step checks $LASTEXITCODE itself; a native command's stderr must not stop the script.
$ErrorActionPreference = 'Continue'
# Read after each native command; set now so that StrictMode never meets it unset.
$global:LASTEXITCODE = 0
# The components' script runs under the PowerShell that runs this one (Windows PowerShell or PowerShell 7).
$shell = (Get-Process -Id $PID).Path
if (-not $shell) { $shell = if (Get-Command pwsh -ErrorAction SilentlyContinue) { 'pwsh' } else { 'powershell' } }

# The steps, in order. The test reads the names from here.
$steps = @(
    @{ Name = 'clippy';     Text = 'clippy on the workspace, warnings are errors (-Lax only counts them)' },
    @{ Name = 'workspace';  Text = "the workspace's tests" },
    @{ Name = 'components'; Text = "the components' tests, with the compiler bundle" },
    @{ Name = 'release';    Text = 'the release-only timing tests' },
    @{ Name = 'packaging';  Text = 'the packaging tests (xtask)' }
)

if ($List) {
    foreach ($s in $steps) { Write-Host ("{0}`t{1}" -f $s.Name, $s.Text) }
    exit 0
}

$repo = (Resolve-Path "$PSScriptRoot/..").Path
$logDir = Join-Path $repo 'target/test-all'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# The release-only timing tests: package, test file, tests.
$releaseTests = @(
    @{ Pkg = 'slides-terminal';  File = 'player';      Tests = @('costly_output_between_stops_is_quick_both_ways', 'a_recording_too_costly_to_play_says_so', 'a_long_recording_is_stepped_quickly') },
    @{ Pkg = 'slides-terminal';  File = 'screen';      Tests = @('costly_output_is_refused_or_quick', 'ordinary_output_is_not_too_costly') },
    @{ Pkg = 'slides-engine';    File = 'terminal';    Tests = @('a_screen_of_letters_shaped_one_at_a_time_is_laid_out_aside') },
    @{ Pkg = 'stagepack-studio'; File = 'ui_terminal'; Tests = @('going_back_to_a_mark_is_quick') },
    @{ Pkg = 'stagepack-studio'; File = 'ui_video';    Tests = @('a_turned_full_size_video_costs_the_window_little') },
    # Tests marked "timing; run in release": a whole file's (no names), or single ones.
    @{ Pkg = 'stagepack-studio'; File = 'edit_speed';  Tests = @() },
    @{ Pkg = 'slides-engine';    File = 'text_geometry'; Tests = @('big_tables_are_read_and_laid_out_quickly') },
    @{ Pkg = 'slides-video';     File = 'decode';      Tests = @('decoding_1080p_takes') }
)

# Each step takes the names of failed tests (none on the first run), runs, and returns its
# worst exit code. Output goes to the screen and to the step's log.
function Invoke-Step([string]$Name, [string[]]$Only, [string]$Log) {
    $script:worst = 0
    $run = {
        param([string[]]$CargoArgs)
        & cargo @CargoArgs 2>&1 | Tee-Object -FilePath $Log -Append | Out-Host
        if ($LASTEXITCODE -ne 0) { $script:worst = $LASTEXITCODE }
    }
    $exact = @()
    if ($Only.Count -gt 0) { $exact = @('--exact') + $Only }
    switch ($Name) {
        'clippy' {
            # Warnings are errors; -Lax only counts them.
            if ($Lax) {
                & $run @('clippy', '--workspace', '--all-targets')
            } else {
                & $run @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
            }
        }
        'workspace' {
            & $run (@('test', '--workspace', '--no-fail-fast', '--') + $exact)
        }
        'components' {
            if ($Only.Count -eq 0) {
                & $shell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'test-components.ps1') 2>&1 | Tee-Object -FilePath $Log -Append | Out-Host
                if ($LASTEXITCODE -ne 0) { $script:worst = $LASTEXITCODE }
            } else {
                # For this one command only, as in test-all.sh: the later steps must not inherit it.
                $saved = $env:STUDIO_BUNDLE
                $env:STUDIO_BUNDLE = Join-Path $repo 'target/bundle'
                try {
                    & $run (@('test', '--workspace', '--no-fail-fast', '--', '--include-ignored') + $exact)
                } finally {
                    $env:STUDIO_BUNDLE = $saved
                }
            }
        }
        'release' {
            if ($Only.Count -gt 0) {
                & $run (@('test', '--release', '--workspace', '--no-fail-fast', '--', '--include-ignored') + $exact)
            } else {
                foreach ($t in $releaseTests) {
                    if ($t.Tests.Count -eq 0) {
                        & $run @('test', '--release', '-p', $t.Pkg, '--test', $t.File, '--no-fail-fast', '--', '--include-ignored')
                    } else {
                        # --include-ignored: in a release build a test marked cfg_attr(debug_assertions, ignore) isn't ignored.
                        & $run (@('test', '--release', '-p', $t.Pkg, '--test', $t.File, '--', '--include-ignored', '--exact') + $t.Tests)
                    }
                }
            }
        }
        'packaging' {
            if ($Only.Count -gt 0) {
                & $run (@('test', '-p', 'xtask', '--no-fail-fast', '--', '--include-ignored') + $exact)
            } else {
                # The Linux package's tests (linux.rs) need bash and zstd, so Windows runs the others.
                # test_all's bash test skips itself where bash can't run the script (none, or WSL's launcher).
                $pk = @('test', '-p', 'xtask', '--no-fail-fast', '--lib', '--test', 'xtask', '--test', 'stage', '--test', 'windows', '--test', 'test_all')
                & $run $pk
                if (-not $Quick) {
                    & $run @('test', '-p', 'xtask', '--test', 'stage', '--no-fail-fast', '--', '--ignored')
                }
            }
        }
    }
    return $script:worst
}

Push-Location $repo
$results = @()
$failed = $false
try {
    foreach ($s in $steps) {
        $name = $s.Name
        if ($Quick -and $name -eq 'release') { continue }
        $log = Join-Path $logDir "$name.log"
        if (Test-Path $log) { Remove-Item $log }
        Write-Host ''
        Write-Host "=== step: $name ==="
        $code = Invoke-Step $name @() $log
        if ($code -eq 0) { $results += "${name}: ok"; continue }
        Write-Host "--- step $name FAILED (exit $code)"
        $failed = $true
        $names = @()
        if (Test-Path $log) {
            $names = @(Select-String -Path $log -Pattern '^---- (.*) stdout ----$' | ForEach-Object { $_.Matches[0].Groups[1].Value -replace ' - should panic$', '' } | Sort-Object -Unique)
        }
        if ($name -ne 'clippy' -and $names.Count -gt 0) {
            Write-Host "--- A loaded machine can fail a test that is timed. Rerunning only: $($names -join ' ')"
            $rerun = Join-Path $logDir "$name-rerun.log"
            if (Test-Path $rerun) { Remove-Item $rerun }
            $again = Invoke-Step $name $names $rerun
            # Passed only if cargo said ok and the log shows at least one test passed (a name that matches nothing exits 0).
            if ($again -eq 0 -and (Select-String -Path $rerun -Pattern 'test result: ok\. [1-9][0-9]* passed' -Quiet)) {
                $results += "${name}: FAILED the first time ($($names.Count) tests), ok on the rerun alone: $($names -join ' ')"
                Write-Host '--- the rerun passed; both results are in the summary and the step still counts as FAILED'
            } else {
                $results += "${name}: FAILED, and not shown to pass alone (failed, or ran no test): $($names -join ' ')"
            }
        } else {
            $results += "${name}: FAILED"
        }
    }
} finally {
    Pop-Location
}

Write-Host ''
Write-Host '=== summary ==='
foreach ($r in $results) { Write-Host $r }
if ($Quick) { Write-Host '(-Quick: the release timing step and the slow packaging tests were skipped)' }
if ($failed) {
    Write-Host "SOME STEPS FAILED (logs in $logDir)"
    exit 1
}
Write-Host 'all steps ok'
exit 0
