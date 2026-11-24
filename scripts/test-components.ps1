# Windows twin of test-components.sh: builds the compiler bundle and runs the tests that need it.
# (The timing tests and the packaging tests have their own steps in test-all.ps1.)
$ErrorActionPreference = 'Continue'
$repo = (Resolve-Path "$PSScriptRoot/..").Path
Push-Location $repo
$status = 0
try {
    & (Join-Path $PSScriptRoot 'make-bundle.ps1') -Out (Join-Path $repo 'target/bundle')
    if (-not $?) { $status = 1 }
    $env:STUDIO_BUNDLE = Join-Path $repo 'target/bundle'
    & cargo test -p slides-components --test compile --test examples --no-fail-fast -- --ignored
    if ($LASTEXITCODE -ne 0) { $status = 1 }
    & cargo test -p stagepack-studio --test builds --no-fail-fast -- --ignored
    if ($LASTEXITCODE -ne 0) { $status = 1 }
    & cargo test -p stagepack-studio --test ai_tool --no-fail-fast
    if ($LASTEXITCODE -ne 0) { $status = 1 }
    & cargo test -p slides-format --test wasm_build --no-fail-fast -- --ignored
    if ($LASTEXITCODE -ne 0) { $status = 1 }
    # The Slidev add-on, built for WebAssembly: run as the app runs it, signed as it's shipped,
    # and through the app from one end to the other.
    & cargo test -p slidev-addon --test wasm --no-fail-fast -- --ignored
    if ($LASTEXITCODE -ne 0) { $status = 1 }
    & cargo test -p xtask --test addon --no-fail-fast -- --ignored package_addon_signs_what_read_package_accepts
    if ($LASTEXITCODE -ne 0) { $status = 1 }
    & cargo test -p stagepack-studio --test slidev_round --no-fail-fast -- --ignored
    if ($LASTEXITCODE -ne 0) { $status = 1 }
} finally {
    Pop-Location
}
exit $status
