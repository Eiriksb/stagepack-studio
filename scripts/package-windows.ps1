# Builds Stagepack Studio's Windows installer on a Windows PC: package-windows.ps1 [-SkipTests] [-Bundle <dir>]
#
# Needs Rust (with the x86_64-pc-windows-msvc and wasm32-unknown-unknown targets) and Inno Setup 6.3 or newer.
# Runs the tests, stages the app folder from release builds, and makes
# target\package\stagepack-studio-<version>-windows-x86_64-setup.exe.
param(
    [switch]$SkipTests,
    # A compiler bundle already made by make-bundle.ps1, to save making it again.
    [string]$Bundle
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repo = (Resolve-Path "$PSScriptRoot/..").Path

# Inno Setup's compiler: the usual places, then the PATH.
$candidates = @(
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
)
$iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) {
    $found = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($found) { $iscc = $found.Source }
}
if (-not $iscc) {
    Write-Host 'Inno Setup 6 is not installed, so the installer cannot be made.'
    Write-Host 'Get it from https://jrsoftware.org/isdl.php (the free "Inno Setup 6" download), install it, and run this again.'
    exit 1
}

Push-Location $repo
try {
    # The tests come first: no installer from a build that fails them. A missing test script is an error, not a skip.
    if ($SkipTests) {
        Write-Host 'Tests skipped (-SkipTests).'
    } else {
        $testScript = Join-Path $PSScriptRoot 'test-all.ps1'
        if (-not (Test-Path $testScript)) { Write-Host "scripts\test-all.ps1 is missing, so the tests cannot run (use -SkipTests to build without them)."; exit 1 }
        $shell = if (Get-Command pwsh -ErrorAction SilentlyContinue) { 'pwsh' } else { 'powershell' }
        & $shell -NoProfile -File $testScript -Quick
        if ($LASTEXITCODE -ne 0) { Write-Host 'The tests failed, so no installer was made.'; exit 1 }
    }

    # The version comes from the app's own manifest; Inno's file version takes only its leading numbers.
    $manifest = Get-Content (Join-Path $repo 'crates/app/Cargo.toml')
    $line = $manifest | Where-Object { $_ -match '^version = "([^"]+)"' } | Select-Object -First 1
    if (-not $line) { throw 'No version in crates/app/Cargo.toml.' }
    $version = [regex]::Match($line, '^version = "([^"]+)"').Groups[1].Value
    $numeric = [regex]::Match($version, '^\d+(\.\d+){0,3}').Value

    # Stage the folder the installer wraps.
    $staged = Join-Path $repo 'target\package-stage'
    $stageArgs = @('run', '--release', '-q', '-p', 'xtask', '--bin', 'package', '--', 'windows-folder', $staged)
    if ($Bundle) { $stageArgs += @('--bundle', (Resolve-Path $Bundle).Path) }
    & cargo @stageArgs
    if ($LASTEXITCODE -ne 0) { Write-Host 'Staging the app folder failed.'; exit 1 }

    # Make the setup.
    $outDir = Join-Path $repo 'target\package'
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    & $iscc "/DAppVersion=$version" "/DAppVersionNumeric=$numeric" "/DStagedDir=$staged" "/DOutDir=$outDir" (Join-Path $repo 'packaging\windows\stagepack-studio.iss')
    if ($LASTEXITCODE -ne 0) { Write-Host 'Inno Setup failed.'; exit 1 }

    $setup = Join-Path $outDir "stagepack-studio-$version-windows-x86_64-setup.exe"
    if (-not (Test-Path $setup)) { throw "Inno Setup finished but $setup is not there." }
    $mb = [math]::Ceiling((Get-Item $setup).Length / 1MB)
    Write-Host "The installer is $setup ($mb MB)."
} finally {
    Pop-Location
}
