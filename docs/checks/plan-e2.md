# Plan E2: the owner's answers to packaging's questions

The run was on the `plan-e2` branch (from `plan-e`'s merge), on a 16-core Linux laptop, with Rust 1.98.0, on 2026-11-24. The branch hasn't been pushed or merged. **Nothing here has run on Windows.** The new Pascal in the installer has never been compiled, and Inno Setup isn't available on this machine; it is checked only by tests that read the script as text.

## The owner's answers (2026-11-24)

- Add the app's folder to the Windows user `Path`: **yes**.
- Give the earlier `.sptx` default back when uninstalling on Windows: **yes**.
- Run on Ubuntu 22.04 and Debian 12: **yes** (glibc 2.35).
- A 561 MB install is **fine**.
- Build the Linux release in Docker on `ubuntu:22.04`.

## What was built

- **The Linux release is built on Ubuntu 22.04** (`fa68844`).
  - `packaging/linux/build.Dockerfile` makes the image `stagepack-studio-build:22.04`. `scripts/package-linux.sh [--bundle-fresh]` runs it with `--rm`, as the host user, with the repo mounted at `/studio` and a cargo volume `stagepack-studio-cargo`. Inside, it builds the compiler bundle (`target/bundle-22.04`), stages the app (`target/package-stage-22.04`) and packs the tarball. On the host it then runs `package glibc-check`.
  - `crates/xtask/src/elf.rs` and `package glibc-check DIR`: reads each ELF file's `objdump -T` output, counts only `GLIBC_<digits>`, prints the highest need per file and `highest: x.y`, and exits 1 if any file is above 2.35 or the folder holds no ELF files.
  - `STUDIO_PACKAGE_TARGET` (stage) and `STUDIO_BUNDLE_BUILD` (`make-bundle.sh`) pick the build folders, which must be real subfolders of `target/`, so the container never reuses the host's builds.
  - The ignored test `old_systems` installs the tarball in `ubuntu:22.04` and `debian:12` containers (as a made user, since `install.sh` refuses root), runs `help` and `check` on a talk with components, builds the component, starts the helper to the point the loader accepts it, and uninstalls.
  - `install.sh`'s refusal now says glibc 2.35 or later; the README's *Install* and *Build the packages* sections say the same and list the runtime libraries.
  - `test-all.sh`'s packaging step runs `old_systems` when docker is found and says so when it skips it.
- **The Windows installer adds the app to `Path` and gives the earlier default back** (`464b9fc`, wording fixed in `2161a37`).
  - `packaging/windows/stagepack-studio.iss` has `ChangesEnvironment=yes` and a `[Code]` section.
  - `Path`: `{app}` is added once to the user `Path` (compared without case and without one trailing `\`), written back as `REG_EXPAND_SZ`. A user with no `Path` gets just the folder, with no leading `;`. Uninstalling takes off only that entry; if nothing else is left, the value goes.
  - Earlier default: at install, the `.sptx` default's ProgID is saved in `HKCU\Software\Stagepack Studio` (`PreviousSptxDefault`; never our own `StagepackStudio.Talk`, so a reinstall keeps the earlier one). It is read before the files go in and written only once they are in, so a cancelled or failed install leaves no such key. After uninstalling, if `.sptx` has no default and that ProgID still exists under `HKCU\Software\Classes` or `HKCR`, it becomes the default again. The key is then deleted either way.
  - Everything is `HKCU`; `HKCR` is only read.
- README (*Install*, Windows), the console twin's comment and `plan-e.md` carry the new facts. In `plan-e.md` the three answered questions point here, and the size question is marked accepted.

## Tests on Linux

All through `scripts/cargo-safe.sh`. Logs are in `target/e2/`.

- **The highest glibc found** (`scripts/package-linux.sh`, cold, exit 0, `target/e2/package-linux-1.log`): `highest: 2.35`.

  | File | Highest glibc |
  |---|---|
  | `stagepack-studio` | 2.35 |
  | `stagepack-terminal` | 2.34 |
  | the bundle's proc-macro libraries (`libserde_derive-*`, `libtoolkit_macros-*`) | 2.34 |
  | `bundle/bin/rustc` | 2.17 |
  | `librustc_driver-*`, `libLLVM` | 2.16 |
  | `rust-lld` | 2.14 |

  The app folder was 559 MB and the tarball 151 MB (`target/package/stagepack-studio-0.1.0-linux-x86_64.tar.zst`). The host-built package measured 561 MB (plan E), which the owner accepted.
- **Both old systems** (`old_systems`, ignored test, `target/e2/old-systems-2.log`): 1 passed, 25 s. On `ubuntu:22.04` and on `debian:12` every step exited 0: libraries, unpack, install, help, check, built, helper-loads, uninstall. The first run (`old-systems.log`) failed on both because `install.sh` refuses root; the test now runs as a made user.
- **The container-built folder** (`STUDIO_STAGED=target/package-stage-22.04`, `the_staged_app_needs_nothing_else`, `target/e2/stage-22.04.log`): 1 passed. It also passed on host staging (192 s). No builder `$HOME` or repo path is in the container-built programs.
- **The tarball** (`linux` ignored test, `target/e2/linux-ignored.log`): 1 passed, 54 s.
- **The xtask tests** (`cargo test -p xtask`, after the Windows change): elf 6, linux 14 (+1 ignored), stage 5 (+1 ignored), test_all 3, windows 17, xtask 8; 0 failed. After the Linux task alone it was 49 passed, 3 ignored (`target/e2/xtask-all.log`, windows 13 then).
- **Windows tests** (`--test windows`): 10 passed and 7 failed before the `.iss` change (the four new tests and three changed ones), 17 passed after.
- `clippy -p xtask --all-targets -- -D warnings`: clean after each task.
- Review: both tasks passed review. Task 1: no Critical or Important findings, six Minor. Task 2: one Important, docs only (hand check 14 said "byte for byte"), fixed in `2161a37`; two Minor.

## What ran

Commits on `plan-e2` since `plan-g`:

- `268650e` Plan E2: the owner's answers to packaging's questions
- `fa68844` Packaging: the Linux release is built on Ubuntu 22.04, so it runs on 22.04 and Debian 12
- `464b9fc` Packaging: the Windows installer adds the app to PATH and gives the earlier .sptx default back
- `2161a37` Docs: the Windows uninstall check says what of the user Path comes back, not byte for byte

Docker images left on the machine: `stagepack-studio-build:22.04`, `ubuntu:22.04`, `debian:12`, and the volume `stagepack-studio-cargo`. No container is left running.

## Not done, and for the owner

Known limits:

- **Opening the window was not tried on Ubuntu 22.04 or Debian 12.** The containers have no display. The app loads `libwayland-client`, `libwayland-egl`, `libEGL` and Vulkan by name for its window, and the README lists the libraries the binary needs, but window start-up there is unchecked.
- **The helper was only load-tested there.** The app has no command that starts it, so on the old systems the test runs `stagepack-terminal` with its input closed, which shows the loader accepts it. The full run (`sh -c 'exit 0'`) was done on the 2.39 host's container-built folder only.
- **The Pascal was never compiled.** Inno Setup isn't available here. Review read it line by line against the Inno 6.3 documentation and found no compile error, and the tests check its text, but a compile error or a wrong step is expected to show first on your PC.
- **The `Path` after uninstall is not byte for byte.** Empty entries are dropped whenever the installer rewrites it (such as the trailing `;` a new user's `Path` often has), and the type is `REG_EXPAND_SZ` even if it was `REG_SZ`. The entries, their order and their text (`%USERPROFILE%` unexpanded) come back as they were. A user with no `Path` has none again. Also, `CompareText` compares ASCII case only.

Other points, from the reports and reviews:

- `stagepack-studio` needs exactly glibc 2.35, so the build depends on the base image staying 22.04. RHEL 9 (2.34) is still out.
- The bundle in `target/bundle-22.04` is reused unless `--bundle-fresh` is given; a changed toolkit needs that flag. The host's `package linux` writes the same tarball name as the container build and would overwrite it.
- Parked for the final fix wave: a cancelled Windows install leaves `HKCU\Software\Stagepack Studio` behind (harmless; the value is right for a later install); `--locked` or a read-only repo mount for the container; saying when the bundle is reused; deleting the tarball if `glibc-check` fails; `libvulkan1` in the old-systems apt line. Parked as harmless: the fixed container name, and a spaced path not run.
- An earlier default that is only in `HKLM` isn't saved, but shows through again once ours is removed.
- `shellcheck` isn't installed; `bash -n` on the script is fine.

## Hand checks for the owner

All on Windows, and all new in this plan. They are numbered as in `plan-e.md`; the steps are written there. Run its check 0 and check 1 first (the scripts and the build), since the installer can't be tried until the setup builds.

- **4.** Before installing, keep the output of `reg query HKCU\Environment /v Path`.
- **5.** After installing, a new terminal runs `stagepack-studio help`, and `Path` shows the app's folder once, at the end, as `REG_EXPAND_SZ`, with the other entries as before. Also try a scratch user with no `Path` value: afterwards `Path` is just the folder, with no leading `;`.
- **6.** `where stagepack-studio` lists the `.com` first.
- **9 and 10.** Commands and the AI tool's `mcp` now use the bare name `stagepack-studio` from a terminal.
- **3 and 13.** Before installing, keep `reg query HKCU\Software\Classes\.sptx /s`. A reinstall over itself leaves the folder in `Path` once, and `reg query "HKCU\Software\Stagepack Studio"` still holds the earlier ProgID as `PreviousSptxDefault`, not `StagepackStudio.Talk`.
- **14.** Uninstalling takes the app's folder off `Path` (the other entries are the same and in order; empty entries may be gone and the type may be `REG_EXPAND_SZ`), makes the earlier program the default for `.sptx` again if it is still installed (and has none if it isn't), leaves it under *Open with*, removes `StagepackStudio.Talk` and `HKCU\Software\Stagepack Studio`, and leaves `%APPDATA%\stagepack-studio`.

On Linux (not in `plan-e.md`'s numbering; its Linux list is unchanged), the one new thing is to open the window on a real Ubuntu 22.04 or Debian 12 desktop, which the containers couldn't do.

## The final review

*To be written by the controller.*
