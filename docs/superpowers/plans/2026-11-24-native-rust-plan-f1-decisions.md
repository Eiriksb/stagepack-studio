# Plan F1: decisions made while building

The owner said "Just dive in" on 2026-11-24, so every call below is the controller's. `docs/checks/plan-f1.md` records what ran, what's left and the hand checks.

## Questions for the owner

- **Back up the signing key** at `~/stagepack-studio-signing/addon-signing.key`. If it is lost, every installed app has to be updated before a new key works.
- **The export has never been seen in a running Slidev or Stagepack.** Hand checks 1 to 5 in the record cover this.
- **Windows hasn't been tried.** This includes the 128 MiB thread stack for add-ons and the key file's permissions.

## Rulings

- **Shape.** The add-on is a wasm32 cdylib (`crates/slidev-addon`). The host has no imports, and both sides speak ABI v1, a byte bundle (`crates/addon-abi`). The app does all reading, writing and zipping. It also bakes the things the add-on can't draw into PNG files.
- **Sandbox.** Add-ons get their own wasmtime engine:
  - a 64 MiB wasm stack, run on a thread with a 128 MiB stack, and a 32 MiB guest stack;
  - 1 GiB of memory;
  - 60 s to run and 5 s to load;
  - the unneeded wasm proposals turned off.
  A deep slide needed far more than the components' 256 KiB.
- **Slidev target: 53.0.0**, which is what Stagepack bundles. Every claim about Slidev's behaviour was checked against its installed source:
  - clicks are counted as Slidev counts them, and `clicks: N` is set when a fly-up needs it;
  - transitions sit on the slide before;
  - the style block is on every slide;
  - `fonts.provider: none`;
  - notes escape `& < > [`.
- **Import.** Untrusted input:
  - single-pass scans;
  - a `src:` budget of 1 000 loads and 64 MiB;
  - ranges read as intervals;
  - shared front matter, capped at 256 keys;
  - placeholders for anything that isn't converted.
- **Package.** A signed `.spaddon`: ed25519 `verify_strict` over sha256(addon.json) || sha256(addon.wasm). It holds exactly 3 entries, with no duplicates, no zip64, and a capped directory. The owner's key was generated here, and only its public half is in the code.
- **App.**
  - Install writes only what was verified, and load verifies again.
  - Names are compared as NFC and lower case.
  - The export budget is 320 MiB, which is a third of the add-on's memory.
  - No export may overwrite a file that is open.
  - Stagepack's manifest is written by the app, with exactly its 8 fields.
- **Dev builds** keep line tables only (the owner's request, from a 255 GB `target/`).

## Smaller findings and notes

Open:
- a relative `v-click` range counts as one click;
- the end-record chance match (about 1 in 100 000);
- the import timing exponent;
- the Task 1 minors (a funcref test, a panic during setup, and the TookTooLong wording);
- export and import round trips lose some things: shapes and stepped pictures come back as placeholders.
