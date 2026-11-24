# Plan E: decisions made while building

The calls made during plan E (packaging) without asking the owner, and the smaller findings left for later. Each decision says what it costs if it's wrong. `docs/checks/plan-e.md` has what ran, what's left and the hand checks.

## Questions for the owner

- **Nothing was compiled or run on Windows.** The installer script, `package-windows.ps1`, `test-all.ps1`, the console twin, the C runtime flags and the registry entries are only read and text-tested. The first build on the owner's PC is their real test; begin with `scripts\test-all.ps1 -List`, then `-Quick`.
- **Add the app's folder to the user `PATH` on Windows?** It needs a `[Code]` step to undo on uninstall.
- **Give the earlier `.sptx` default back on Windows uninstall?** It also needs `[Code]`, so decide it with `PATH`.
- **Lower the Linux glibc floor (2.39 today)?** That means building the release in an older base.
- **Is 561 MB installed, 150 MiB to download acceptable?** Nearly all of it is the compiler bundle.

## Rulings

- **Owner answers (2026-11-23).** Inno Setup on Windows; a tarball with `install.sh` on Linux; the live-terminal helper ships, off until asked.
- **Task 1, the build tool.** The icon is drawn in code. The notices carry the toolchain's `COPYRIGHT.html` as `notices/rust-COPYRIGHT.html`, plus standard SPDX texts for crates that ship no licence file.
- **Task 2, staging.** Release builds go to `target/package-build` with `--remap-path-prefix` for the cargo and rustup homes and the repository. A test checks that no builder path is left in a shipped program. The folder is 561 MB against a 600 MB budget. Cost if wrong: none found.
- **Task 3, the Linux scripts (three review rounds).**
  - The app folder is `dirname(XDG_DATA_HOME)/lib/stagepack-studio`.
  - Install and uninstall share `paths.sh`. The manifest records settings only, and uninstall recomputes every path and deletes a file only if it is provably ours.
  - Every `XDG_*` value must be absolute and inside the home. Nothing may land in the data folder; links and files that aren't ours are refused.
  - The swap is atomic. The leftover sweep touches only our own `.new-`/`.old-` folders.
- **Task 4, Windows (three rounds).**
  - Per user, `HKCU` only, no `[Code]`.
  - `.gitattributes`: LF everywhere, CRLF for `.ps1` and `.iss`.
  - The shipped `.exe` is a windows-subsystem program, so a double-click shows no terminal. A console twin, `stagepack-studio.com`, is what a terminal runs, so commands wait and keep their exit status.
  - The icon isn't embedded in the exe yet.
- **Task 5, test scripts.**
  - `test-all` reruns a failed step's failing tests alone and reports both results; the script still exits 1.
  - Clippy treats warnings as errors, with `CLIPPY_LAX=1`/`-Lax` to opt out.
  - `test-components` runs the tests that need the bundle, not the whole workspace's ignored tests.
- **Final review, `.sptx` on Linux.** Claim Stagepack's type `application/x-stagepack` as well, record and restore each type's default per list, and keep our glob at the default weight. Weight 90 was the controller's ruling and was wrong: the tie dropped Stagepack from *Open with*. The re-review caught it. Cost if wrong now: none found against a faithful copy of Stagepack's registration.
- **Final review, Windows.**
  - `[InstallDelete]` of `{app}\bundle` before an upgrade.
  - `CREATE_NO_WINDOW` for `rustc`.
  - `+crt-static` for the programs and the proc-macro DLLs. The DLLs need `RUSTC_BOOTSTRAP` with `-Zhost-config`, the only way cargo passes flags to host builds under `--target`.
  - The earlier ProgID is kept under `OpenWithProgids` with a declarative `{reg:…}` entry.
  - Cost if wrong: the first Windows build or the `dumpbin` hand check shows it, and shipping the VC runtime DLLs in `bundle\bin` is the fallback.
- **Final review, Linux.** `install.sh` runs the new copy's `help` before the swap and refuses where it can't start. The glibc floor is documented.

## Smaller findings and notes

The full list is in `docs/checks/plan-e.md`, under "Not done, and for the owner" and "The final review". The ones most likely to matter:
- the `RUSTC_BOOTSTRAP` block in `make-bundle.ps1` (see the record);
- the `{reg:…}` entry reads only `HKCU`;
- the exe has no embedded icon;
- a `SIGKILL` mid-copy leaves a `.new-<pid>` folder;
- `test-all.ps1` has never run.
