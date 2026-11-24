# Plan F1: the Slidev add-on

The run was on the `plan-f` branch (from `main`), on a Linux laptop, on 2026-11-24. The branch hasn't been pushed or merged. **Nothing here has run on Windows.** The export has also never been opened in a running Slidev or a running Stagepack; everything below was checked by tests that read the output.

## What was built

A sandboxed wasm translator for Slidev export and import, installed from a signed `.spaddon` file. The target is Slidev **53.0.0**; any other version is refused.

- **The add-on ABI and host runner** (task 1, `245aa00`, `7d61ef8`). The app runs an add-on in its own wasmtime engine, with no imports, on a thread with a 128 MiB native stack and a 64 MiB wasm stack.
  - Limits: **60 s** for a run, **1 GiB** of memory, 64 MiB for the module, 10 000 files and a 512 MiB bundle in the ABI. Loading has its own, shorter time limit.
  - The add-on's own stack is 32 MiB (set in `build.rs`; the real need measured under 16 KiB).
- **Export to Slidev** (task 2, `81c6f98`, `196daa8`, `f257bc2`). Slides, clicks as Slidev counts them, transitions, fonts, notes, hidden slides and styled layouts, with parts the app draws baked to pictures and a notice for each animated one. Notes are written with `&`, `<`, `>` and `[` as entities, so they can't end their comment or put HTML in the presenter view.
- **Import from Slidev** (task 3, `3d6b2b1`, `a389dc5`, `cd9b76b`). Front matter, layouts, clicks, notes, pictures, and `src:` imports. The `src:` budget: at most 1000 imports followed, at most 64 MiB of imported content, at most 256 range parts; past that, a placeholder and a notice.
- **The signed package** (task 4, `70c3efa`, `c9c4b88`). `slidev-0.1.0.spaddon` holds `addon.json`, `addon.wasm` and `addon.sig`, signed with ed25519 (`verify_strict`, checked before the manifest is read). Names, entry counts (exactly 3, no duplicates) and sizes are checked before anything is unpacked. `package addon --key FILE` builds it; `addon-keygen FILE` makes a key.
- **The app** (task 5, `9b5f521`, `55c69f0`). *File ▸ Add-ons…* (start screen and talk windows), *Export ▸ Slidev…* and import. The add-on installs from the package that was verified, never from a second unzip, into `addons/slidev`, and is checked again at each load. Names are compared in NFC and lower case. The export budget is **320 MiB**, so the add-on can hold the bundle about three times over in its 1 GiB beside its stack; a compile-time assertion keeps it there, and videos are left out first.

## The add-on file and the owner's key

- File: `target/package/slidev-0.1.0.spaddon` (under the ignored `target/`, not committed).
- Size: 393323 bytes.
- sha256: `1a45ed851cab1c78daf669cacdf1dc0455f043424111cf7ff49c07a0ca0de194`.
- **The owner's key is `~/stagepack-studio-signing/addon-signing.key`** (mode 600, in a folder of mode 700, outside the repository). **Back it up now, somewhere safe, and never share it.** Anyone with it can sign an add-on that every installed app will trust. If it is lost, the key built into the app (`crates/app/src/addons/key.rs`) can only be changed by updating every installed app, and add-ons signed with a new key are refused until then. The key file was never read or printed while this was built.

## Tests on Linux

All through `scripts/cargo-safe.sh`. Counts are from each report's own command output.

| Command | Result |
|---|---|
| `test -p addon-abi -p slides-components -p slides-format` (after task 1's fix round) | 429 passed, 0 failed, 6 ignored |
| `test -p slidev-addon -p addon-abi` (after task 2's last round) | addon-abi 9; slidev-addon unit 7, export 28; wasm 2 ignored |
| `test -p slidev-addon --release --test wasm -- --ignored` | export 2 passed |
| slidev-addon native, after task 3's last round | 71 passed (18 unit, 28 export, 25 import) |
| slidev-addon wasm, same | 3 passed |
| `test -p addon-abi --test package`, `test -p xtask --test addon -- --include-ignored`, `test -p stagepack-studio --test addon_key` | 9 passed; 4 of 4 passed; 1 passed |
| `test -p stagepack-studio` (after task 5's fix round) | 1491 passed, 0 failed, 41 ignored |
| `--test slidev_round -- --ignored` | 2 passed, 12.2 s (the round trip, and a bundle 8 MiB under the budget) |

- `clippy --workspace --all-targets -- -D warnings`: clean after task 5's fix round. `clippy -p slidev-addon --release --target wasm32-unknown-unknown -D warnings`: clean.
- Every task but task 4 went through at least one fix round and a re-review that approved it. Task 4's fix (one finding, the duplicate-name check) was read in full by the controller and not re-reviewed separately.

## What ran

- `245aa00`, `7d61ef8`: task 1
- `81c6f98`, `196daa8`, `f257bc2`: task 2
- `3d6b2b1`, `a389dc5`, `cd9b76b`: task 3
- `70c3efa`, `c9c4b88`: task 4
- `9b5f521`, `55c69f0`: task 5
- `ae2ced8`: the dev profile keeps line tables only, so test programs stay small (the owner's request).

## Security findings

- **Fixed:** notes escaping (task 2, `196daa8`). `escape::notes` first replaced only `-->`; `--!>` also ends a comment, and Slidev shows notes with `v-html`. Notes now escape `&`, `<`, `>` and `[` (the last because Slidev turns `[click]` in notes into click marks). Pinned by a unit test and an export test.
- **Still to fix (the final fix wave): the zip64 locator.** `package.rs` reads only the classic end-record counts, so a zip64 locator can make the reader see a much larger central directory than the 3-entry check allows. The fix: refuse any archive with a zip64 locator (`0x07064b50` in the 20 bytes before the end record), cap the central-directory size before `ZipArchive::new`, and test both. The file is signed by the owner and a package that fails the size check never gets that far, but the check isn't yet as strict as it should be.

## Not done, and for the owner

Known limits:

- **The export has never been seen in a running Slidev or a running Stagepack.** Task 2 was checked against Slidev 53's source by a reviewer, and the tests read the text, but no one has run `slidev` on the output. The hand checks below are the first time.
- **Windows hasn't been run.** The 128 MiB thread stack is reserved, not used, and should commit only as touched; that needs checking on your PC.
- **The round trip loses things.** On import: the export's shapes come back as placeholders in positioned boxes; the stepped pictures the app drew come back as placeholders; a region the export moved comes back as a positioned box, not a `region.*` line. Every picture of a part the app drew gets a *Frozen* notice. `vendor/` is left out with a notice, and so are files the add-on doesn't use (such as `components/*.vue`). A relative v-click range stays open with a notice.
- **Parked as minor:** `Remove` is undone by a leftover `.slidev-old-*` folder; `Remove` waits for the lock on the window thread; a `BufWriter` for the writes; status and load read through a symlinked `addons` folder; a lone table as one click, and lists among other blocks, rotated each-regions and a U+FEFF before a fence (task 2); the funcref test and a setup panic (task 1).

## Hand checks for the owner

Do each on **Linux and Windows**. Use a scratch copy of anything you care about.

1. **Install the add-on from the file.** *File ▸ Add-ons…*, choose `slidev-0.1.0.spaddon`. It should show Slidev 0.1.0 as installed. Check the size and sha256 first against the figures above.
2. **Export the showcase talk.** Open it, then *Export ▸ Slidev…*. Read the report. Open the exported folder in Stagepack. Check:
   - no code question is asked;
   - it presents;
   - the clicks, transitions and fonts look right.
   If you have Slidev 53.0.0, also run it on the export.
3. **Import one of your own Stagepack talks** through the add-on (a Slidev folder or zip), and read the report: every loss should have a notice, and nothing should be lost without one.
4. **Open a Stagepack talk without the add-on** (remove it first if installed) and look for the Add-ons offer.
5. **Remove the add-on** from the Add-ons dialog, and check that the app's other files are untouched.
6. On Windows only: watch that the first export doesn't stall or run out of memory.

## The final review

*To be written by the controller.*
