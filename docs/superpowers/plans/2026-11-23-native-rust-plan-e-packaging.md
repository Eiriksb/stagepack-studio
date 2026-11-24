# Plan E: Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One per-user installer for each platform — an Inno Setup `setup.exe` for Windows and a `.tar.zst` with `install.sh` for Linux — holding the app, the compiler bundle, the live terminal's helper, the AI tool's commands, fonts, the icon and every licence notice, registering `.sptx` as Stagepack Studio's with Stagepack kept under *Open with*; plus one script per platform that runs every test.

**Architecture:** A dev-only crate, `crates/xtask` (binary `package`), does the work that is the same on both systems: it draws the app's icon in code, gathers the licence notices from `cargo metadata`, the fonts and the toolchain, and stages the installed app's folder from release builds and the compiler bundle, checking its size. Each platform then wraps that folder: on Linux the xtask packs it with the install scripts and tests them against a scratch home here; on Windows an Inno Setup script and a PowerShell script build `setup.exe` on the owner's PC, since nothing for Windows can be built on this machine. A `shipped` feature of the app makes a release build look for the bundle and the helper beside itself only.

**Tech Stack:** Rust 1.98.0; `cargo metadata` (JSON, read with serde_json); tiny-skia 0.11.4 for the icon; `zstd` and `tar` on Linux; Inno Setup 6 on the owner's Windows PC; the existing `scripts/make-bundle.sh` and `make-bundle.ps1`.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md` ("Packaging", "Constraints": size and install, the success tests); `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md` (stage E); `docs/superpowers/specs/2026-11-18-live-content-design.md` ("Then stage E … gains the decoder, the add-on's helper, and the AI tool's command"). **Owner's answers (2026-11-23):** Windows uses Inno Setup; Linux uses a tarball with `install.sh`; the live terminal's helper ships inside both, off until a talk asks and the owner says yes.

## Global Constraints

- Work on the branch `plan-e`, cut from `plan-g`.
- No GitHub Actions, ever (owner, 2026-11-10). Don't push.
- Run cargo through `scripts/cargo-safe.sh`. Never run `cargo fmt`.
- **Per user, no administrator rights.** Linux installs under the user's home only; Windows installs with `PrivilegesRequired=lowest` and writes only `HKCU`. Nothing writes outside the user's own folders, ever asks for a password, or needs the internet.
- **Where things go.** Linux: the app folder at `$XDG_DATA_HOME/../lib/stagepack-studio` — that is `~/.local/lib/stagepack-studio` — a link `~/.local/bin/stagepack-studio`, the menu entry, the file type and the icons in their XDG places. Never in `~/.local/share/stagepack-studio`, which is the app's own data folder (permissions, recovery): installing, upgrading and uninstalling never touch it. Windows: `{localappdata}\Programs\Stagepack Studio`; the app's data folder (`%APPDATA%\stagepack-studio`) is never touched.
- **The installed folder** holds `stagepack-studio[.exe]`, `stagepack-terminal[.exe]`, `bundle/` (as `make-bundle` makes it), `guide/` (`components.md` and `writing-talks-for-ai.md`), `LICENSE`, `NOTICES.txt` and `icons/`. The app finds the bundle and the helper beside itself, as now. **At most 600 MB** on disk (the design's budget); the build fails above it.
- **`.sptx` is Stagepack Studio's by default, and Stagepack stays under *Open with*.** Linux: the type `application/vnd.stagepack.talk` with glob `*.sptx`, our desktop entry set as its default; another program's desktop entry listing the type is left alone. Windows: ProgID `StagepackStudio.Talk`, `HKCU\Software\Classes\.sptx` default set to it, and our ProgID added to `.sptx\OpenWithProgids` without removing any other. Uninstalling removes only what we added.
- **A shipped build** (`--features shipped`, which both packages use) ignores `STUDIO_BUNDLE` and `STUDIO_TERMINAL_HELPER` and looks beside its own executable only. Without the feature (development and tests) nothing changes.
- **The notices are complete:** every crate in the app's and the helper's dependency graph for Linux x86_64 and Windows x86_64 (normal dependencies, not dev or build-only ones), each with its licence expression and its licence files' text; the fonts' licences; the Rust toolchain's and LLVM's (from the toolchain's own files); OpenH264's. A crate with no licence file gets its expression and a line saying no file came with it. The build fails if any crate in the graph has neither.
- **Nothing in a test touches the owner's real home.** Every test that installs sets `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_BIN_HOME` (our own variable for `~/.local/bin`) and `XDG_CACHE_HOME` to a temporary folder. The install scripts refuse to run with `HOME` unset or `/`.
- Never stage `docs/superpowers/plans/2026-11-09-roadmap.md`. Don't name real decks, clients or organisations. No secrets or signing keys in the repo (code signing is out of this plan).

Not in this plan: code signing; automatic updates (manual in the first release, per the design); macOS; a package for app stores, Flatpak or `.deb`; the Slidev add-on (stage F).

Rulings made in the plan:
- **The icon** (my call) is drawn in code by the xtask: a rounded square in the app's accent colour with a white slide shape and a small play triangle — an original mark, not any product's. It's written as PNGs at 16, 24, 32, 48, 64, 128, 256 and 512 pixels and as an `.ico` holding 16–256 (PNG-in-ICO), so no drawing tool is needed and every platform's icon comes from one place.
- **Version** comes from `crates/app/Cargo.toml` (`0.1.0`); files are named `stagepack-studio-0.1.0-linux-x86_64.tar.zst` and `stagepack-studio-0.1.0-windows-x86_64-setup.exe`.
- **Upgrading** is installing over: the Linux script replaces the app folder atomically (unpack beside, then rename) and Inno Setup replaces in place; data is kept either way. Uninstalling never asks to delete data.
- **The test scripts:** `scripts/test-all.sh` and `scripts/test-all.ps1` run, in order, clippy on the workspace, the workspace's tests, the components' tests with the bundle (`scripts/test-components.sh` / its PowerShell twin), the release-only timing tests, and the packaging tests; each step's result is printed and the script ends non-zero if any failed, listing them. `--quick` skips the release timing step.

## Review Focus

- **Installing into someone's real home by mistake.** `install.sh` run with `HOME` unset, `HOME=/`, an `XDG_DATA_HOME` that's relative, a path with spaces, an existing `~/.local/lib/stagepack-studio` that's a file or a link elsewhere, a `~/.local/bin` that doesn't exist, or run as root. It refuses or does the right thing in plain words, and never writes outside the user's folders. Owned by Task 3's `install_refuses_what_it_shouldnt_touch`.
- **Losing the owner's data or work.** Installing over a running copy, upgrading from an older version, uninstalling, and reinstalling: `~/.local/share/stagepack-studio` (permissions, working copies) and `%APPDATA%\stagepack-studio` are never removed or changed. Owned by Task 3's `upgrade_and_uninstall_keep_the_data` and Task 4's `the_windows_script_never_touches_the_data`.
- **Taking `.sptx` away from Stagepack.** With Stagepack's desktop entry or ProgID present, ours becomes the default, theirs stays offered under *Open with*, and uninstalling ours leaves theirs exactly as it was. Owned by Task 3's `stagepack_stays_under_open_with` and Task 4's script test.
- **A shipped copy that doesn't work without Rust.** The staged folder, run with no Rust on `PATH`, no `STUDIO_BUNDLE` and a scratch home: builds a component in a talk, opens a pack, runs `check`, starts the helper. Owned by Task 2's `the_staged_app_needs_nothing_else`.
- **Missing licences.** A dependency added later without updating anything: the notices still list it, or the build fails saying which. Owned by Task 1's `every_dependency_has_its_notice`.

---

### Task 1: The packaging tool, the icon and the notices

**Files:**
- Create: `crates/xtask/{Cargo.toml,src/main.rs,src/icon.rs,src/notices.rs}`
- Modify: `Cargo.toml` (workspace member; `publish = false`)
- Test: `crates/xtask/tests/xtask.rs`

**Interfaces:**
- Produces: the binary `package` with subcommands `icon OUT_DIR`, `notices OUT_FILE`, `stage OUT_DIR`, `linux`, and `windows-folder OUT_DIR` (the last three added by Tasks 2–4); `pub fn draw_icon(size: u32) -> tiny_skia::Pixmap`; `pub fn write_ico(pngs: &[(u32, Vec<u8>)]) -> Vec<u8>`; `pub fn notices(metadata_json: &[u8], extra: &[Notice]) -> Result<String, String>` where `pub struct Notice { pub name: String, pub licence: String, pub texts: Vec<String> }`. The tool runs `cargo metadata --format-version 1 --filter-platform x86_64-unknown-linux-gnu` and again for `x86_64-pc-windows-msvc`, walks the resolve graph from `stagepack-studio` and `slides-terminal-helper` over normal dependencies only, and reads each package's `license` field and the files in its folder named `LICENSE*`, `LICENCE*`, `COPYING*`, `NOTICE*` (any case).
- [ ] **Step 1: Write the failing tests.** `the_icon_is_drawn_at_every_size` (each size's PNG decodes to that size, isn't one colour, and the 16 px one has an opaque centre); `the_ico_holds_each_size` (the ICO header lists 16, 24, 32, 48, 64, 128, 256 with PNG data that decodes); `every_dependency_has_its_notice` (on the real workspace: every package in both platforms' graphs appears in the notices by `name version`; the fonts' three licence files, the toolchain's and OpenH264's appear); `a_package_with_no_licence_fails` (a small metadata JSON fixture with one package lacking both `license` and files → `Err` naming it).
- [ ] **Step 2: Run them to see them fail.** `scripts/cargo-safe.sh test -p xtask`
- [ ] **Step 3: Implement.** The toolchain's licences come from `rustc --print sysroot`'s `share/doc/rust/` (`LICENSE-APACHE`, `LICENSE-MIT`, `COPYRIGHT`); if absent, from the `rust-src` component's copies; failing both, the build fails.
- [ ] **Step 4: Run the tests** — PASS; clippy clean.
- [ ] **Step 5: Commit** `Packaging: a tool that draws the icon and gathers every licence notice`.

### Task 2: The installed folder, and shipped builds

**Files:**
- Create: `crates/xtask/src/stage.rs`
- Modify: `crates/app/Cargo.toml` (feature `shipped`), `crates/components/src/compile.rs` (`Bundle::find`), `crates/app/src/live_terminal/helper.rs` (`find_helper`), `crates/terminal-helper/Cargo.toml` if needed, `Cargo.toml` (`[profile.release]`: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`)
- Test: `crates/xtask/tests/stage.rs`, `crates/app/tests/shipped.rs`

**Interfaces:**
- Produces: `package stage OUT_DIR` builds `stagepack-studio` and `stagepack-terminal` with `--release --features shipped` (the helper without the feature if it has none), runs `scripts/make-bundle.sh` into `OUT_DIR/bundle` unless `--bundle DIR` names one already made, copies the guides, `LICENSE`, writes `NOTICES.txt` and `icons/`, and prints the folder's size; fails over 600 MB with *The app folder is N MB, over the 600 MB budget.* `slides_components::Bundle::find` and `live_terminal::find_helper` skip their environment variables when the app's `shipped` feature is on (pass the choice down as a function argument or a cfg from the app, so `slides-components` gains no feature of its own unless needed).
- [ ] **Step 1: Write the failing tests.**
  - `shipped.rs`: `a_shipped_build_ignores_the_variables` (with the feature, `find_helper` and `Bundle::find` don't read the variables — unit-level through the functions, gated `#[cfg(feature = "shipped")]`, plus a test without it that they still do).
  - `stage.rs` (marked `#[ignore = "builds a release and the bundle; run by test-all"]`, run explicitly): `the_staged_app_needs_nothing_else` — stage into a temp folder reusing `target/bundle` via `--bundle`; check the layout and size; then with `PATH=/usr/bin:/bin` (no cargo or rustc), no `STUDIO_*` variables and a scratch home: run `stagepack-studio check` on a copy of `fixtures/components` with its `build` folder removed and the folder listed in a scratch `build-permissions.json` — it builds and checks clean; run `check` on a `.sptx` of a fixture; and have a tiny test talk's live terminal helper found beside the app (`stagepack-studio` can't open a window here, so check the helper path through a `--print-helper` debug flag only if one exists — otherwise run `stagepack-terminal` directly with a `Start` frame of `sh -c 'exit 0'` and see `Exited`).
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `scripts/cargo-safe.sh test -p stagepack-studio --test shipped`, `--features shipped --test shipped`, and `-p xtask --test stage -- --ignored` — PASS. Record the staged size.
- [ ] **Step 5: Commit** `Packaging: stage the installed app folder from shipped release builds`.

### Task 3: The Linux package

**Files:**
- Create: `packaging/linux/{install.sh,uninstall.sh,stagepack-studio.desktop.in,stagepack-studio-mime.xml}`, `crates/xtask/src/linux.rs`
- Test: `crates/xtask/tests/linux.rs`

**Interfaces:**
- Produces: `package linux` stages (Task 2), adds `install.sh`, `uninstall.sh`, the desktop entry template and the MIME file, and writes `target/package/stagepack-studio-<ver>-linux-x86_64.tar.zst` (`tar --zstd` at level 19, one top folder). `install.sh` (POSIX `sh`, `set -eu`):
  - refuses with a sentence: `HOME` unset, empty or `/`; run as root (uid 0); a relative `XDG_*` path;
  - installs the folder to `${XDG_DATA_HOME:-$HOME/.local/share}/../lib/stagepack-studio` normalised (i.e. `~/.local/lib/stagepack-studio`) by unpacking beside it and renaming over; links `${XDG_BIN_HOME:-$HOME/.local/bin}/stagepack-studio`; writes the desktop entry (`Exec=<abs path> %f`, `MimeType=application/vnd.stagepack.talk;`, `Icon=stagepack-studio`, `Categories=Office;Presentation;`, `Terminal=false`) to `$XDG_DATA_HOME/applications/`; the MIME file to `$XDG_DATA_HOME/mime/packages/` and runs `update-mime-database "$XDG_DATA_HOME/mime"`; icons to `$XDG_DATA_HOME/icons/hicolor/<n>x<n>/apps/stagepack-studio.png` and as the type's icon under `mimetypes/`; runs `update-desktop-database` and `gtk-update-icon-cache` on those folders if present; sets the default with `xdg-mime default stagepack-studio.desktop application/vnd.stagepack.talk` (with `XDG_CONFIG_HOME` honoured); says where it put things and that `~/.local/bin` must be on `PATH` if it isn't.
  - `uninstall.sh` (copied into the app folder) removes exactly those, and the `mimeapps.list` default line only if it still names ours; never the data folder.
- [ ] **Step 1: Write the failing tests** (each runs the scripts with a scratch `HOME` and `XDG_*`, on a staged folder made with `--bundle target/bundle`; the slow staging is shared across tests once):
  - `install_puts_everything_in_its_place`: every file listed above exists; `desktop-file-validate` passes on the entry; `xdg-mime query default application/vnd.stagepack.talk` (scratch dirs) answers `stagepack-studio.desktop`; `xdg-mime query filetype` of a `.sptx` file answers our type; the link runs `stagepack-studio help`.
  - `install_refuses_what_it_shouldnt_touch`: each refusal case from the Review Focus gives non-zero and a sentence, and writes nothing.
  - `upgrade_and_uninstall_keep_the_data`: a `~/.local/share/stagepack-studio/` with files; install, install again (a newer fake version string), uninstall: the data folder is byte-for-byte the same; after uninstall nothing of ours remains (list the scratch home).
  - `stagepack_stays_under_open_with`: a fake `stagepack.desktop` listing the same type; after install ours is default and theirs is still in `applications/` and still lists the type; after uninstall theirs is untouched.
  - `the_tarball_unpacks_to_one_folder`: `package linux` (ignored by default; run by test-all) makes a `.tar.zst` whose entries share one top folder and include `install.sh` executable.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests** — PASS.
- [ ] **Step 5: Commit** `Packaging: a Linux tarball that installs per user, with the file type and the menu entry`.

### Task 4: The Windows installer script

**Files:**
- Create: `packaging/windows/stagepack-studio.iss`, `scripts/package-windows.ps1`, `crates/xtask/src/windows.rs`
- Test: `crates/xtask/tests/windows.rs`

**Interfaces:**
- Produces: `package windows-folder OUT_DIR` (on Windows: stages as Task 2 with `.exe` names and `make-bundle.ps1`; on Linux: refuses with *The Windows folder is staged on Windows.*). `stagepack-studio.iss`: `PrivilegesRequired=lowest`, `PrivilegesRequiredOverridesAllowed=` empty, `DefaultDirName={localappdata}\Programs\Stagepack Studio`, `DisableProgramGroupPage=yes`, `ArchitecturesAllowed=x64compatible`, `ArchitecturesInstallIn64BitMode=x64compatible`, the version from a `#define` the script passes, `SetupIconFile` the staged `.ico`, `[Files]` the whole staged folder recursively, `[Icons]` a Start-menu entry, `[Registry]` under `HKCU\Software\Classes` only: `.sptx` (Default) `StagepackStudio.Talk` with `uninsdeletevalue`, `.sptx\OpenWithProgids\StagepackStudio.Talk` empty string with `uninsdeletevalue`, the ProgID's `DefaultIcon` and `shell\open\command` `"{app}\stagepack-studio.exe" "%1"` with `uninsdeletekey`; `ChangesAssociations=yes`; no `[UninstallDelete]` of anything outside `{app}`. `package-windows.ps1`: checks for `ISCC.exe` (Inno Setup 6) and says where to get it if missing, runs the tests (`scripts/test-all.ps1 -Quick`) unless `-SkipTests`, stages the folder, runs ISCC, prints the setup's path and size.
- [ ] **Step 1: Write the failing tests** (`windows.rs`, on Linux, reading the files as text): `the_installer_needs_no_administrator` (the privilege lines exactly; no `HKLM`, `HKCR` or `{commonpf}`, `{commonappdata}`, `{sys}` anywhere); `the_windows_script_never_touches_the_data` (no `{userappdata}` or `%APPDATA%` in `[UninstallDelete]`, `[InstallDelete]` or `[Code]`); `the_file_type_keeps_stagepack` (the three registry lines above with their flags; nothing deletes `OpenWithProgids` itself); `the_installer_ships_the_helper_and_the_bundle` (`[Files]` takes the staged folder recursively and the staging code lists `stagepack-terminal.exe` and `bundle`); `the_powershell_script_runs_the_tests_first`.
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement.** Write the Inno and PowerShell files carefully: they can't be run here. Keep `[Code]` empty unless something can't be done declaratively.
- [ ] **Step 4: Run the tests** — PASS.
- [ ] **Step 5: Commit** `Packaging: the Windows installer's script, built on the owner's PC`.

### Task 5: Scripts that run every test, and the record

**Files:**
- Create: `scripts/test-all.sh`, `scripts/test-all.ps1`, `scripts/test-components.ps1` if it doesn't exist, `docs/checks/plan-e.md`
- Modify: `README.md` (how to install from each package, how to build them)
- Test: `crates/xtask/tests/test_all.rs`

**Interfaces:**
- Produces: the two scripts per the ruling, each with `--quick`/`-Quick` and `--list`/`-List` (print the steps and exit 0).
- [ ] **Step 1: Write the failing tests:** `test_all_lists_its_steps` (runs `scripts/test-all.sh --list`: clippy, workspace, components, release timing, packaging, in that order); `test_all_ps1_names_the_same_steps` (parses the PowerShell file's step names and compares).
- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `scripts/test-all.sh` once in full (in the background; it's long) and record each step's result; then `package linux` once, record the tarball's size and the unpacked folder's size; install it into a scratch home and run `stagepack-studio help`, `check` on a fixture and the helper there.
- [ ] **Step 5: Write `docs/checks/plan-e.md`** in the form of `docs/checks/plan-g5.md`: what was built; what ran and its totals; sizes against the 600 MB budget; and the owner's hand checks: on Windows from a fresh clone — install Inno Setup 6, run `scripts/package-windows.ps1`, install the setup under a **fresh Windows user with no Rust**, double-click a `.sptx`, see Stagepack under *Open with* if installed, then both success tests from the design step by step (the no-code one with desktop PowerPoint for the export, the coder one with a component from the guide), the AI tool's `mcp` from PowerShell, the live terminal with `powershell.exe`, uninstall and see the data folder kept; on Linux the same with the tarball under a fresh user (or a scratch home) on a real desktop. Leave `## The final review` with *To be written by the controller.*
- [ ] **Step 6: Commit** `Packaging: scripts that run every test, and plan E's record`.
