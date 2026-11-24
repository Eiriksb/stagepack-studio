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

- File: `target/package/slidev-0.1.0.spaddon` (under the ignored `target/`, not committed), rebuilt with the owner's key after the final fix wave, so it holds every fix. To make it again: `package addon --key ~/stagepack-studio-signing/addon-signing.key`.
- Size: 394313 bytes.
- sha256: `b6365436804b7ed481ceedc101c6210fc29989a04ceac8427ad48705a1ff376e`.
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
| After the final fix wave: `test --no-fail-fast -p addon-abi -p slidev-addon -p slides-components -p xtask` | 238 passed, 1 failed, 12 ignored. The one failure is the import timing test `unclosed_constructs_take_time_in_line_with_their_length` (4 MB of `<a b="` took 8.7 times 1 MB, past its bound of 8), under load. Alone, and with its whole test binary run twice, it passed |
| After the final fix wave: `test -p stagepack-studio --test addons --test ui_addons` | 25 and 7 passed |
| After the final fix wave: `test -p slidev-addon --release --test wasm -- --ignored`; `--test slidev_round -- --ignored` | 3 passed; 3 passed (with a talk naming `./photo.png`) |
| After the final fix wave: `test --no-fail-fast -p stagepack-studio` | 1504 passed, 0 failed, 42 ignored. A first run, which stops at a failure, had two `ai_tool` tests fail under load (a model picture's process ended with signal 11); both passed alone |

- `clippy --workspace --all-targets -- -D warnings`: clean after task 5's fix round, and again after the final fix wave, with the wasm32 clippy of the add-on. `clippy -p slidev-addon --release --target wasm32-unknown-unknown -D warnings`: clean.
- Every task but task 4 went through at least one fix round and a re-review that approved it. Task 4's fix (one finding, the duplicate-name check) was read in full by the controller and not re-reviewed separately.

## What ran

- `245aa00`, `7d61ef8`: task 1
- `81c6f98`, `196daa8`, `f257bc2`: task 2
- `3d6b2b1`, `a389dc5`, `cd9b76b`: task 3
- `70c3efa`, `c9c4b88`: task 4
- `9b5f521`, `55c69f0`: task 5
- `ae2ced8`: the dev profile keeps line tables only, so test programs stay small (the owner's request).
- `b0d4740`, `cc81264`, `ab43934`, `3309f61`: the final fix wave (I1–I5, M1–M9, the parked task 2 and task 5 minors, the zip64 check).

## Security findings

- **Fixed:** notes escaping (task 2, `196daa8`). `escape::notes` first replaced only `-->`; `--!>` also ends a comment, and Slidev shows notes with `v-html`. Notes now escape `&`, `<`, `>` and `[` (the last because Slidev turns `[click]` in notes into click marks). Pinned by a unit test and an export test.
- **Fixed in the final fix wave: the zip64 locator.** `read_package` looks at every end record a zip reader might take, before the zip is read. It refuses one with a zip64 locator (`0x07064b50`) in the 20 bytes before it, and one whose central directory says it takes more than 8 × (46 + 64 + 64 KiB) bytes. Both are tested. The import of a Slidev talk does the same with its own bounds (200 000 entries, a 64 MiB directory), reading the zip64 record where there is one, since a talk with large videos can be zip64.

## Not done, and for the owner

Known limits:

- **The export has never been seen in a running Slidev or a running Stagepack.** Task 2 was checked against Slidev 53's source by a reviewer, and the tests read the text, but no one has run `slidev` on the output. The hand checks below are the first time.
- **Windows hasn't been run.** The 128 MiB thread stack is reserved, not used, and should commit only as touched; that needs checking on your PC.
- **The round trip loses things.** On import: the export's shapes come back as placeholders in positioned boxes; the stepped pictures the app drew come back as placeholders; a region the export moved comes back as a positioned box, not a `region.*` line. Every picture of a part the app drew gets a *Frozen* notice. `vendor/` is left out with a notice, and so are files the add-on doesn't use (such as `components/*.vue`). A relative v-click range stays open with a notice.
- **A panic in the parser stops the whole export or import.** Natively, `slides_format` catches a panic on one slide and keeps the rest. In the add-on, on wasm32, a panic aborts, so the whole export or import fails with "The add-on stopped working." How often the `markdown` crate panics hasn't been measured.
- **Parked as minor, and still open:** the funcref test and a setup panic (task 1). The task 2 and task 5 minors were fixed in the final fix wave.

## Hand checks for the owner

Do each on **Linux and Windows**. Use a scratch copy of anything you care about.

1. **Install the add-on from the file.** *File ▸ Add-ons…*, choose `slidev-0.1.0.spaddon`. It should show Slidev 0.1.0 as installed. Check the size and sha256 first against the figures above.
2. **Export the showcase talk.** Open it, then *Export ▸ Slidev…*. A report lists what couldn't be carried over as it was, by slide, and the status line says how many notes there are. The export is one `.sptx` file; open it in Stagepack. Check:
   - no code question is asked;
   - it presents;
   - the clicks, transitions and fonts look right.
   If you have Slidev 53.0.0, also run it on the export.
3. **Import one of your own Stagepack talks** through the add-on: a packed `.sptx` that holds `stagepack.json` (a folder isn't imported). Read the report: every loss should have a notice, and nothing should be lost without one.
4. **Export the same talk again, to the same file,** and open it in Stagepack again. It should be the same talk to Stagepack: the export keeps the first one's `id` and `created`, so Stagepack knows it as the same talk. Its slides and notes are the new export's; notes you wrote in Stagepack since the first export are not kept. (Exporting over any Stagepack talk takes on that talk's `id` and `created` the same way.)
5. **Open a Stagepack talk without the add-on** (remove it first if installed) and look for the Add-ons offer.
6. **Remove the add-on** from the Add-ons dialog, and check that the app's other files are untouched.
7. On Windows only: watch that the first export doesn't stall or run out of memory.

## The final review

A review of the whole branch, `db98b1b..52cd887` (15 commits, 92 files), on the most capable model said it could merge with fixes. It found 5 important and 9 minor issues:
- **Pictures and videos written with a relative path** (`./photo.png`) were left out of the export. The app and the add-on now share one path rule, `addon_abi::normalise`.
- **Export ▸ Slidev… could write over the open talk.** Every export now refuses a file that is open in a window.
- **Import refused Stagepack talks with many bundled packages.** Only the files actually given to the add-on now count.
- **The export's notices went onto one status line.** They now go to the same capped report dialog that import uses.
- **The add-on's own crates had no licence notice.** They are now listed under their own heading in NOTICES.txt.

One fix wave (`b0d4740`, `cc81264`, `ab43934`, `3309f61`, `2727915`) fixed all of them. It also fixed the minor items parked from the task reviews:
- clicks on tables and lists now match the engine;
- *Remove* can no longer be undone by a leftover folder;
- a linked add-ons folder is refused;
- the security review's point: an add-on file with a zip64 record, or with an oversized directory, is refused before it is read.

The app suite gave 1504 passed and 0 failed. The add-on and round-trip tests passed, and clippy is clean.

A re-review on the same model approved the merge. Of its four new minor points, the controller fixed two: Remove's busy sentence, and hand check 4, which no longer promises that notes written in Stagepack survive a re-export. Two are left:
- the end-record check could refuse a valid file about once in 100 000, when an end-record signature happens to sit in its last 64 KiB;
- the import timing test runs about n^1.5 on 4 MB of unclosed tags, which is probably the import parsing its own output. The 60 s limit bounds it.

Not checked here:
- the export in a running Slidev or Stagepack;
- anything on Windows.
