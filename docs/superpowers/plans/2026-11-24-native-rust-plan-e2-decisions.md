# Plan E2: decisions made while building

The calls made during plan E2 without asking the owner, and the smaller findings left for later. Each decision says what it costs if it's wrong. `docs/checks/plan-e2.md` has what ran, what's left and the hand checks.

## The owner's answers (2026-11-24)

The Windows installer adds the app's folder to the user `PATH`, and uninstalling makes the earlier `.sptx` program the default again. The Linux release runs on Ubuntu 22.04 and Debian 12, built in Docker on `ubuntu:22.04`. 561 MB installed is fine.

## Questions for the owner

- **The Windows `[Code]` has never been compiled.** It was reviewed line by line against Inno 6.3. The hand checks in `docs/checks/plan-e.md` (numbered 3 to 15) are its real test.
- **Opening the window on Ubuntu 22.04 and Debian 12 is untried**, since the containers have no display. Try one real machine or VM.

## Rulings

- **Task 1.**
  - `scripts/package-linux.sh` builds the image `stagepack-studio-build:22.04`. It runs the stage and `package linux` inside it as the caller, with the repository mounted read-only and `target/` writable.
  - Cargo's registry lives in the volume `stagepack-studio-cargo`. The bundle is made in the container as `target/bundle-22.04`.
  - `package glibc-check` refuses anything above 2.35. The ignored test `old_systems` installs and runs the package in `ubuntu:22.04` and `debian:12`.
  - `install.sh`'s refusal names glibc 2.35. Cost if wrong: none found. The app needs exactly 2.35, so the base image must stay 22.04.
- **Task 2.**
  - `[Code]` adds `{app}` to `HKCU\Environment\Path` once, comparing without case or a trailing `\`, and writes it back as `REG_EXPAND_SZ`. Uninstall removes exactly that entry.
  - The default it replaces is read at `ssInstall` and saved at `ssPostInstall` in `HKCU\Software\Stagepack Studio`. On uninstall it is put back if its ProgID still exists under `HKCU` or `HKCR`.
  - Cost if wrong: the hand checks show it. Empty `Path` entries are dropped, which is stated in the hand check.
- **Task 2's hand check wording.** The controller corrected it: the user `Path` comes back with the same entries and order, but not byte for byte.
- **Task 3's review** was folded into the final review, since the task is docs only. The final review checked every figure.
- **Final review.**
  - The old-systems test runs only on a container-built package, after a marker check and `docker info`.
  - Under rootless Docker the container runs as `0:0`, which maps to the caller.
  - The script refuses root, and Ctrl-C stops its container.
  - The controller's earlier ruling to park "one tarball name" as harmless was partly wrong, and was fixed by the marker.

## Smaller findings and notes

- `test-all.sh` doesn't warn when the marker's commit is older than `HEAD`.
- With Podman as `docker`, the rootless check fails, and the message wrongly says the daemon isn't running.
- The container's fixed name means two builds at once collide.
- Under a second host user the cargo volume isn't re-owned recursively.
- An earlier default only in `HKLM` isn't saved, but shows through again.
- `CompareText` folds ASCII case only.
