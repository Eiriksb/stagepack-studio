# Building the packages

How the two install packages are made. Installing them is in the [README](../../README.md#install).

Run the tests first with `scripts/test-all.sh` (or `scripts\test-all.ps1` on Windows; `--quick` / `-Quick` skips the slow steps). Each package is built on its own system, since nothing for Windows can be built on Linux.

Linux, with Docker (one you can run without sudo: the `docker` group, or rootless Docker), Rust and `binutils`:

```bash
scripts/package-linux.sh                # add --bundle-fresh to make the compiler bundle again
```

This builds the release, the compiler bundle and the package in an Ubuntu 22.04 container (the image `stagepack-studio-build:22.04`, with cargo's downloads kept in the volume `stagepack-studio-cargo`), so that the package needs no glibc newer than 2.35. It makes `target/package/stagepack-studio-<version>-linux-x86_64.tar.zst`, then checks every program and library in it with `package glibc-check`; if one needs more, it deletes the tarball and fails. A tarball that passes gets the marker `target/package/.built-in-ubuntu-22.04` beside it, holding the commit it was built from, and `test-all.sh` installs it on Ubuntu 22.04 and Debian 12 only when that marker is there (otherwise it says why it skipped that test). The script refuses to run as root, mounts the repo read-only with only `target/` writable, and Ctrl-C stops its container. The folder it packs is made by `package stage` from release builds, and fails if it is over 600 MB. A package built straight on the host (`package linux --bundle target/bundle`) needs the host's glibc, so it is for trying things out only; it removes the marker.

Windows, in PowerShell, with Rust (the `x86_64-pc-windows-msvc` and `wasm32-unknown-unknown` targets) and [Inno Setup](https://jrsoftware.org/isdl.php) 6.3 or newer:

```powershell
scripts\package-windows.ps1
```

This runs `scripts\test-all.ps1 -Quick` (skip with `-SkipTests`), stages the folder, and makes `target\package\stagepack-studio-<version>-windows-x86_64-setup.exe`. Pass `-Bundle <dir>` to reuse a bundle made by `scripts\make-bundle.ps1`.
