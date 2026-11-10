# Windows twin of make-bundle.sh: make-bundle.ps1 -Out <dir>
param([Parameter(Mandatory = $true)][string]$Out)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/..").Path
$sysroot = (& rustc --print sysroot).Trim()
$triple = 'x86_64-pc-windows-msvc'
$wasm = 'wasm32-unknown-unknown'

# The folder is replaced, so make sure it's one of ours.
if ((Test-Path $Out) -and (Get-ChildItem $Out -Force | Select-Object -First 1) -and -not (Test-Path (Join-Path $Out 'bin/rustc.exe'))) {
    throw "$Out has files in it and isn't a bundle, so it's been left alone"
}

if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
foreach ($dir in 'bin', "lib/rustlib/$triple/bin", "lib/rustlib/$wasm/lib", 'toolkit/deps', 'toolkit-src') {
    New-Item -ItemType Directory -Force -Path (Join-Path $Out $dir) | Out-Null
}
$Out = (Resolve-Path $Out).Path

Copy-Item "$sysroot/bin/rustc.exe", "$sysroot/bin/*.dll" "$Out/bin/"
Copy-Item "$sysroot/lib/rustlib/$triple/bin/rust-lld.exe" "$Out/lib/rustlib/$triple/bin/"
Copy-Item "$sysroot/lib/rustlib/$wasm/lib/*.rlib", "$sysroot/lib/rustlib/$wasm/lib/*.rmeta" "$Out/lib/rustlib/$wasm/lib/"

Set-Location $repo
# A target folder of its own and made afresh, so that only one variant of each library
# is copied.
$build = 'target/bundle-build'
if (Test-Path $build) { Remove-Item $build -Recurse -Force }
# The libraries' panic messages name their source files, and they end up in every build.
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$cargoHome=/cargo$([char]0x1f)--remap-path-prefix=$repo=/studio"
cargo build --release -q -p toolkit --target $wasm --target-dir $build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Copy-Item "$build/$wasm/release/libtoolkit.rlib" "$Out/toolkit/"
Copy-Item "$build/$wasm/release/deps/*.rlib", "$build/$wasm/release/deps/*.rmeta" "$Out/toolkit/deps/"
Copy-Item "$build/release/deps/toolkit_macros-*.dll", "$build/release/deps/serde_derive-*.dll" "$Out/toolkit/deps/"

# The source, for a code editor, with a workspace standing in for ours.
Copy-Item 'crates/toolkit', 'crates/toolkit-macros' "$Out/toolkit-src/" -Recurse
@'
[workspace]
resolver = "3"
members = ["toolkit", "toolkit-macros"]

[workspace.package]
edition = "2024"
license = "MIT"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
'@ | Set-Content -Encoding ascii -Path "$Out/toolkit-src/Cargo.toml"
