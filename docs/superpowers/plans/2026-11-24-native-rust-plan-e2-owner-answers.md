# Plan E2: the owner's answers to packaging's questions — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put into effect the owner's answers to plan E's open questions:
- the Linux release runs on Ubuntu 22.04 and Debian 12;
- the Windows installer adds the app folder to the user `PATH`;
- uninstalling on Windows makes the earlier `.sptx` program the default again.

**Architecture:**
- **Linux.** A new `scripts/package-linux.sh` builds an `ubuntu:22.04` image (`packaging/linux/build.Dockerfile`) with the repository's toolchain, and runs `package linux` inside it. Everything shipped is therefore linked against glibc 2.35: the release programs and the bundle's proc-macro libraries. A pure-Rust check, `max_glibc`, reads a staged ELF's highest `GLIBC_x.y` need through `objdump -T`. An ignored test runs the packed app in plain `ubuntu:22.04` and `debian:12` containers.
- **Windows.** The installer gains a small `[Code]` section that does two things. It adds `{app}` to `HKCU\Environment\Path` once and takes out exactly that entry on uninstall. It saves the `.sptx` default it replaces, and gives it back on uninstall if that program is still registered.

**Tech Stack:** Rust 1.98 (xtask), POSIX sh / bash, Docker 29 (the owner's), Inno Setup 6.3 Pascal Script.

**Spec:** the owner's answers, given 2026-11-24 in conversation:
- "yes it should add windows to PATH";
- "yes it should default" (give the earlier default back on uninstall);
- "Yes it should at least run on 22.04 and debian 12";
- "the 561mb is completely fine";
- build in Docker, on `ubuntu:22.04`.

The plan amends plan E: `docs/superpowers/plans/2026-11-23-native-rust-plan-e-packaging.md`, with its record in `docs/checks/plan-e.md`.

## Global Constraints

- Branch `plan-e2` cut from `plan-g`; merge back into `plan-g` with `--no-ff`.
- **glibc ceiling 2.35** for every ELF file the Linux package ships that is built here: `stagepack-studio`, `stagepack-terminal`, and every `.so` under `bundle/toolkit/deps/`. The bundle's `rustc` and its `librustc_driver`/`libLLVM` come from the official toolchain (built for glibc 2.17) and are checked too.
- The container image: `FROM ubuntu:22.04`. The toolchain inside is rustup's `1.98.0` with the `wasm32-unknown-unknown` target. The image tag is `stagepack-studio-build:22.04`.
- The container runs as the caller's uid:gid, with the repository mounted at `/studio`. Cargo's registry is cached in a named volume, `stagepack-studio-cargo`. Only `target/` in the repository is written to. Use `--memory 18g`.
- Nothing in the container or the tests touches `~/.local/share/stagepack-studio`, `~/.cache/stagepack-studio`, `~/Documents`, the real `~/.local/lib` or `~/.local/bin`, or any docker image or container that isn't ours. Never `docker system prune`, never `docker rmi` of anything but `stagepack-studio-*` tags. Containers are started with `--rm`.
- The installer stays per user: `HKCU` only, no administrator rights, nothing written outside the user's own folders. `ChangesEnvironment=yes`.
- The PATH entry is exactly `{app}` with no trailing backslash. It is added only if no entry of the user `Path` already equals it, compared case-insensitively and ignoring a trailing `\`. The value is written back as `REG_EXPAND_SZ`. Uninstall removes only entries equal to `{app}` and leaves every other entry, and their order, untouched.
- The saved earlier default lives in `HKCU\Software\Stagepack Studio`, value `PreviousSptxDefault`. That key is deleted on uninstall.
- No real decks, clients or organisations in commits or docs. Stage files by name, never `git add -A`, never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Use `scripts/cargo-safe.sh` on the host, never `cargo fmt`, never `pkill`. Commit trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Docker image pulls (`ubuntu:22.04`, `debian:12`) are approved by the owner.

## Review Focus

1. **A user `Path` that is unset, empty, or a `REG_SZ` rather than `REG_EXPAND_SZ`.** Install must still add `{app}`, never leave a leading `;`, and never turn `%USERPROFILE%`-style entries into fixed text. This is pinned by a text test that the code reads with `RegQueryStringValue` and writes with `RegWriteExpandStringValue`, and by hand check 2 in Task 2.
2. **Reinstall or upgrade over an existing install.** The saved earlier default must not be overwritten with our own ProgID, and `Path` must not get a second entry. This is pinned by Task 2's text test `an_upgrade_keeps_the_saved_default`, and by hand check 4.
3. **The earlier program was uninstalled before Stagepack Studio.** Uninstall must not write back a default that points at nothing; it removes ours and leaves the default empty. This is pinned by Task 2's text test (it checks `RegKeyExists` on the ProgID under `HKCU\Software\Classes` and `HKCR`) and by hand check 5.
4. **The repository path or the host user inside the container.** A staged binary must contain neither the host's `$HOME` nor the repository path. The existing `the_staged_app_needs_nothing_else` assertion runs on the container-built folder in Task 1.
5. **A bundle `.so` built on the host slipping into the package.** `make-bundle.sh` run inside the container must build its proc-macro libraries there, never copy `target/bundle` from the host. This is pinned by Task 1's `max_glibc` check, which runs over every ELF file in the package.

---

### Task 1: The Linux release built on Ubuntu 22.04

**Files:**
- Create: `packaging/linux/build.Dockerfile`
- Create: `scripts/package-linux.sh`
- Create: `crates/xtask/src/elf.rs` (`pub mod elf;` in `crates/xtask/src/lib.rs`)
- Modify: `scripts/test-all.sh` (the packaging step), `README.md` ("Build the packages", "Install"), `docs/checks/plan-e.md` (the Linux facts, the glibc question marked answered)
- Test: `crates/xtask/tests/elf.rs`, and `crates/xtask/tests/old_systems.rs` (ignored)

**Interfaces:**
- Produces:
  - `xtask::elf::max_glibc(path: &Path) -> Result<Option<(u32, u32)>, String>`: the highest `GLIBC_x.y` version in `objdump -T` output for `path`, or `None` if the file needs none.
  - `xtask::elf::parse_glibc_needs(objdump_output: &str) -> Option<(u32, u32)>`: the pure parser it uses.
  - `xtask::elf::elf_files(folder: &Path) -> Vec<PathBuf>`: every regular file under `folder` that starts with `\x7fELF`, without following links.
  - `GLIBC_CEILING: (u32, u32) = (2, 35)`.
  - `scripts/package-linux.sh [--bundle-fresh]`: leaves `target/package/stagepack-studio-<version>-linux-x86_64.tar.zst`, and prints its path and the largest glibc need it found.

- [ ] **Step 1: Write the failing parser tests in `crates/xtask/tests/elf.rs`**

```rust
#[test]
fn the_highest_glibc_need_is_found() {
    let out = "0000 DF *UND* 0 (GLIBC_2.2.5) malloc\n0000 DF *UND* 0 (GLIBC_2.34) __libc_start_main\n0000 DF *UND* 0 (GLIBC_2.17) clock_gettime\n0000 DF *UND* 0 (GLIBCXX_3.4.30) _ZSt4cout\n";
    assert_eq!(xtask::elf::parse_glibc_needs(out), Some((2, 34)));
}
#[test]
fn no_glibc_need_is_none() {
    assert_eq!(xtask::elf::parse_glibc_needs("0000 DF *UND* 0 (GLIBCXX_3.4) x\n"), None);
}
#[test]
fn two_point_ten_is_above_two_point_nine() {
    assert_eq!(xtask::elf::parse_glibc_needs("(GLIBC_2.9) a\n(GLIBC_2.10) b\n"), Some((2, 10)));
}
#[test]
fn elf_files_finds_only_elf_and_skips_links() { /* a temp folder: an ELF copied from /bin/true, a text file, a symlink to the ELF → exactly one path */ }
#[test]
fn this_machines_true_needs_some_glibc() {
    assert!(xtask::elf::max_glibc(std::path::Path::new("/bin/true")).unwrap().is_some());
}
```

- [ ] **Step 2: Run them to see them fail.** Run `scripts/cargo-safe.sh test -p xtask --test elf`. Expected: compile error, because `elf` is missing.

- [ ] **Step 3: Implement `elf.rs`.** `GLIBCXX_` must not match: match the `GLIBC_` token followed by digits. Compare versions as numbers, not strings. If `objdump` isn't installed, `max_glibc` returns an `Err` that names binutils.

- [ ] **Step 4: Run the tests to see them pass.** Run `scripts/cargo-safe.sh test -p xtask --test elf`. Expected: 5 passed.

- [ ] **Step 5: Write `packaging/linux/build.Dockerfile`.**
  - `FROM ubuntu:22.04`.
  - apt installs:
    - `ca-certificates curl build-essential pkg-config cmake nasm zstd binutils`;
    - the X and Wayland development packages the app links against: `libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev libwayland-dev libvulkan-dev libfontconfig-dev libfreetype-dev`.
    - Confirm the list against a host `ldd target/package-build/release/stagepack-studio`, and against what the build asks for. Add whatever a failed build names, and record it in the report.
  - rustup is installed to `/opt/rust` (`RUSTUP_HOME=/opt/rust/rustup`, `CARGO_HOME=/opt/rust/cargo`, which is world-readable) with `--default-toolchain 1.98.0 --profile minimal`. Then `rustup target add wasm32-unknown-unknown`, plus `rust-src` if `make-bundle.sh` needs it.
  - `ENV PATH=/opt/rust/cargo/bin:$PATH`, `CARGO_HOME=/cargo-cache` (the named volume), `HOME=/tmp/home`.

- [ ] **Step 6: Write `scripts/package-linux.sh` (bash, `set -euo pipefail`).**
  - Build the image with `docker build -t stagepack-studio-build:22.04 -f packaging/linux/build.Dockerfile packaging/linux`.
  - Then `docker run --rm --memory 18g --user "$(id -u):$(id -g)" -v "$repo:/studio" -v stagepack-studio-cargo:/cargo-cache -w /studio stagepack-studio-build:22.04 bash -c '<inside>'`.
  - `<inside>` does three things:
    - makes the bundle afresh in `target/bundle-22.04` with `scripts/make-bundle.sh`;
    - runs `cargo run -q -p xtask --bin package -- linux --bundle target/bundle-22.04`;
    - runs `package stage`'s own builds inside, with `--target-dir target/package-build-22.04`, so the host's `target/package-build` is never mixed in.
  - Expose the target dir as a `STUDIO_PACKAGE_TARGET` env read by `stage()`. It defaults to `target/package-build`, and the value must stay inside `target/`.
  - The named volume is created owned by root. Make it writable by the uid: run one `docker run --rm -v stagepack-studio-cargo:/c ubuntu:22.04 chown "$(id -u):$(id -g)" /c` before the build.
  - After the build, on the host, check every ELF file in the staged folder with `elf_files` and `max_glibc`, through `cargo run -p xtask --bin package -- glibc-check <staged dir>`, a new subcommand. It exits 1 and names each file above 2.35.
  - Print the tarball path.

- [ ] **Step 7: Add `package glibc-check DIR` to `crates/xtask/src/main.rs`.**
  - It prints one line per ELF file, `<need> <relative path>`, then `highest: 2.xx`.
  - It exits 1 if any file is above `GLIBC_CEILING`.
  - A test in `crates/xtask/tests/xtask.rs` runs it on a temp folder holding a copy of this machine's `/bin/true` (2.34 or above on the 2.39 host). If the copy is above 2.35 it expects exit 1; otherwise exit 0. Assert on the actual `max_glibc` value, not on the host.

- [ ] **Step 8: Write the ignored test `crates/xtask/tests/old_systems.rs::the_package_runs_on_ubuntu_22_04_and_debian_12`.**
  - It needs the tarball from Step 6. If it is missing, the test fails with "run scripts/package-linux.sh first".
  - For each of `ubuntu:22.04` and `debian:12`, it runs this in `docker run --rm` with the tarball mounted read-only:
    - `apt-get install -y --no-install-recommends zstd libxkbcommon0 libxkbcommon-x11-0 libxcb1 libstdc++6 libfontconfig1 libfreetype6 libwayland-client0` (the runtime twins of Step 5's list);
    - unpack the tarball, `HOME=/tmp/h sh install.sh`;
    - `~/.local/bin/stagepack-studio help` (exit 0);
    - `~/.local/bin/stagepack-studio check /fixtures/components` (exit 0, with `build-permissions.json` set as in `the_staged_app_needs_nothing_else`, and `fixtures/components` mounted read-only and copied to a temp folder first);
    - the helper starting `sh -c 'exit 0'`, through a tiny `stagepack-studio` subcommand if one exists; if not, skip this item and say so in the report;
    - `sh ~/.local/lib/stagepack-studio/uninstall.sh` (exit 0).
  - The container runs as root inside, which is fine: nothing on the host is written.
  - Assert on each exit code, and print the container's output if one fails.

- [ ] **Step 9: Run it.**
  - Run `bash scripts/package-linux.sh` in the background: a cold image plus a release build takes a long while.
  - Expected: a tarball, and `highest: 2.35` or lower.
  - Then run `scripts/cargo-safe.sh test -p xtask --test old_systems -- --ignored`. Expected: 1 passed.
  - Also run `the_staged_app_needs_nothing_else` against the container-built folder: add a `STUDIO_STAGED` env to that test that skips its own staging, if that is simple. Otherwise run the path assertions inside `old_systems.rs` instead, with the same `HOME`/repo assertion.

- [ ] **Step 10: Wire it into `test-all.sh` and the docs.**
  - The packaging step also runs `--test elf`, and `--test old_systems -- --ignored` when `docker` is found. If docker is missing it says so and skips.
  - `README.md`:
    - "Build the packages" says the Linux package is built by `scripts/package-linux.sh` in Docker;
    - "Install" says it runs on Ubuntu 22.04, Debian 12 or newer (glibc 2.35), and lists the runtime libraries.
  - `docs/checks/plan-e.md`: the glibc question is marked answered, with the result.

- [ ] **Step 11: Commit.** Stage the new and changed files by name, in a commit with a message like "Packaging: the Linux release is built on Ubuntu 22.04, so it runs on 22.04 and Debian 12".

### Task 2: The Windows installer adds the app to PATH and gives the earlier default back

**Files:**
- Modify: `packaging/windows/stagepack-studio.iss`
- Modify: `crates/xtask/tests/windows.rs` (its "no `[Code]`" assertions become assertions on the code below)
- Modify: `README.md` ("Install", Windows), `docs/checks/plan-e.md` (the two questions marked answered, Windows hand checks), `crates/app/src/bin/stagepack-studio-console.rs` (its comment says the installer doesn't add the folder to PATH)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces, as Pascal functions in the `.iss` `[Code]` section:
  - `function PathHasEntry(Path, Entry: string): Boolean`;
  - `function PathWithout(Path, Entry: string): string`;
  - `procedure AddAppToPath()`;
  - `procedure RemoveAppFromPath()`;
  - `procedure SavePreviousDefault()`;
  - `procedure RestorePreviousDefault()`.

- [ ] **Step 1: Write the failing text tests in `crates/xtask/tests/windows.rs`.**
  - `the_installer_adds_the_app_to_the_user_path`: `[Setup]` has `ChangesEnvironment=yes`. `[Code]` reads `HKCU`, `Environment`, `Path` with `RegQueryStringValue` and writes with `RegWriteExpandStringValue`. `CurStepChanged` calls `AddAppToPath` on `ssPostInstall`. `CurUninstallStepChanged` calls `RemoveAppFromPath` on `usUninstall`. No `HKLM` anywhere.
  - `an_upgrade_keeps_the_saved_default`: `SavePreviousDefault` is called on `ssInstall`. It writes `PreviousSptxDefault` only when the current `.sptx` default is non-empty and not `StagepackStudio.Talk`.
  - `the_earlier_default_comes_back_only_if_it_still_exists`: `RestorePreviousDefault` runs on `usPostUninstall`. It checks `RegKeyExists` for the ProgID under `HKCU\Software\Classes\` and `HKCR\`, writes the `.sptx` default only if the current default is empty or missing, and deletes `Software\Stagepack Studio` with `RegDeleteKeyIncludingSubkeys(HKCU, ...)`.
  - `the_path_code_compares_without_case_or_trailing_slash`: `PathHasEntry` and `PathWithout` use `Lowercase` (or `CompareText`) and strip one trailing `\`.
  - Update `the_windows_script_never_touches_the_data`, and any test that asserts no `[Code]`, so they allow this section. Keep the data-folder bans: no `{userappdata}\stagepack-studio`, no `DelTree` outside `{app}`.

- [ ] **Step 2: Run them to see them fail.** Run `scripts/cargo-safe.sh test -p xtask --test windows`. Expected: the 4 new tests fail.

- [ ] **Step 3: Write the `[Code]` section and `ChangesEnvironment=yes`.**
  - The PATH helpers split on `;`, drop empty parts on rewrite, and keep the other entries' order and text exactly.
  - `AddAppToPath` uses `ExpandConstant('{app}')`. If `Path` is missing, it writes just `{app}`.
  - The existing `{reg:…}` `OpenWithProgids` line stays.
  - The comment above `[Registry]` drops "Uninstalling doesn't put that default back".
  - The header comment says that the user `Path` and `HKCU\Software\Stagepack Studio` are written too.
  - Inno 6.3 names to use: `RegQueryStringValue`, `RegWriteExpandStringValue`, `RegWriteStringValue`, `RegKeyExists`, `RegDeleteKeyIncludingSubkeys`, `RegDeleteValue`, `CurStepChanged(CurStep: TSetupStep)`, `CurUninstallStepChanged(CurUninstallStep: TUninstallStep)`.

- [ ] **Step 4: Run the tests to see them pass.** Run `scripts/cargo-safe.sh test -p xtask --test windows`. Expected: all pass.

- [ ] **Step 5: Docs.**
  - The README's Windows install text says the folder is added to the user `PATH`, so a new terminal runs `stagepack-studio` (which finds the `.com`). An assistant's MCP command can be `stagepack-studio.com`. It also says uninstalling makes the earlier program the default again, if it is still installed.
  - Correct the console binary's comment.
  - `docs/checks/plan-e.md` Windows hand checks gain these, numbered within the existing list:
    1. Before installing, `reg query HKCU\Environment /v Path` and keep the output.
    2. After installing, a new terminal runs `stagepack-studio help`, and `Path` has the folder once. Also try a user with no `Path` value at all (`reg delete HKCU\Environment /v Path` on a scratch user).
    3. `where stagepack-studio` lists the `.com` first.
    4. Reinstall: still once, and `reg query "HKCU\Software\Stagepack Studio"` still holds the earlier ProgID, not ours.
    5. Uninstall: `Path` is as in item 1, byte for byte. The earlier program is the default again if it is still installed. If it was uninstalled first, `.sptx` has no default.
  - Mark the two questions answered.

- [ ] **Step 6: Commit.** Stage the changed files by name, in a commit with a message like "Packaging: the Windows installer adds the app to PATH and gives the earlier .sptx default back".

### Task 3: The record

**Files:**
- Create: `docs/checks/plan-e2.md`. It records:
  - what was built;
  - the commands run with their results: the highest glibc found, both old systems, and the test counts;
  - the hand checks that are new for the owner;
  - a "## The final review" section left for the controller.
- Modify: `docs/checks/plan-e.md`: its "Questions for you" list points to `plan-e2.md` for the three answered questions, and the size question is marked accepted ("561 MB is fine", 2026-11-24).

- [ ] **Step 1:** Write the record. Use facts from the reports and the commands' own output only.
- [ ] **Step 2:** Commit, with a message like "Docs: plan E2's record".
