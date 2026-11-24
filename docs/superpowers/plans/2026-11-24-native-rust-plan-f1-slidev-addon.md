# Plan F1: the Slidev add-on, installed from a file — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Slidev add-on works end to end without the network. You install it from a signed `.spaddon` file on an *Add-ons* dialog. With it, *Export ▸ Slidev…* writes a `.sptx` that Stagepack opens, and opening a Stagepack `.sptx` imports a new, unsaved copy with a report.

**Architecture:**
- **The translator** is a Rust crate, `slidev-addon`. It builds as a native library (so it can be tested) and as a `wasm32-unknown-unknown` cdylib, which is the add-on itself. It depends only on `slides-format` (made wasm-ready), a small YAML parser and the new ABI crate. It never touches files.
- **The host** (`slides_components::addon`) runs the add-on in wasmtime with the component sandbox's engine pattern: epoch deadline, memory limiter and no imports. It passes bytes in and gets bytes out.
- **The app** does all reading, writing and zipping. For an export it also bakes the things the add-on can't draw into PNG pictures, using the engine the PowerPoint export already uses: custom components, charts, icons, models, terminals and video posters.
- **The package and its key.** The add-on ships as a `.spaddon` zip (manifest, wasm and an ed25519 signature). It is checked against a public key built into the app. The `package addon` xtask command builds and signs it with a key file kept outside the repository.

**Tech Stack:**
- Rust 1.98;
- wasmtime =50.0.1 (as components);
- `ed25519-dalek` =2.2.0, with `default-features = false` and `features = ["std","zeroize","rand_core"]` for keygen in xtask only;
- `sha2` =0.10.9;
- `zip` =9.5.0 (the engine's);
- `yaml-rust2` =0.10.4 (pure Rust, wasm-ready; check the exact current version on crates.io and pin it);
- gpui-kit 0.9.0.

**Spec:** `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md`, "The Slidev add-on" and the item-16 answer. The roadmap is `docs/superpowers/plans/2026-11-09-native-rust-roadmap.md`, stage F. The owner said "Just dive in" on 2026-11-24, so the calls below are the controller's. Plan F2 adds *Get*, signed download over HTTPS, and updates.

## Global Constraints

- **No network in F1.** Nothing in this plan opens a socket.
- **The core app knows nothing about Slidev's syntax.**
  - Everything about Slidev's Markdown, layouts and headmatter lives in `crates/slidev-addon`.
  - The app knows only five things: the add-on's name `slidev`; the two operations `export` and `import`; that a file holding `stagepack.json` is a Slidev talk (it does today); and the names `stagepack.json` and `slides.md`.
- **Slidev target: 53.0.0.** This is the `@slidev/cli` version Stagepack bundles. The exported manifest's `slidev` field is `"53.0.0"`.
- **Stagepack's manifest** is `stagepack.json`, written by the app from fields the add-on returns. Exactly these keys:
  - `format` (1);
  - `id` (a UUID v4 the app makes);
  - `title`;
  - `created` and `updated`, both RFC 3339 UTC from the app's clock;
  - `slidev` (`"53.0.0"`);
  - `entry` (`"slides.md"`);
  - `meta` (`{}`).
- **What an export may hold**, so that Stagepack asks no code question. It may hold only:
  - `slides.md`;
  - `public/**` pictures, video and fonts;
  - `stagepack.json`.

  It must not hold `.vue .js .mjs .cjs .ts .mts .cts .jsx .tsx .node .wasm` files, `setup/`, `index.html`, `vite.config*`, `uno.config*`, `unocss.config*`, `postcss.config*`, `.postcssrc*` or `package.json`.

  The headmatter's `theme` is `default`, and it has no `addons` and no `pwa`. A test checks every exported file name and the headmatter against this list.
- **Canvas:** the headmatter has `canvasWidth: 1920` and `aspectRatio: 16/9`. Our slide is 1920×1080 units, so positions map 1:1 to pixels.
- **Add-on ABI v1** (a new crate, `crates/addon-abi`, which builds for both native and wasm32):
  - Exports: `addon_abi() -> u32` (= 1), `alloc(len: u32) -> u32`, `info() -> u64`, `run(ptr: u32, len: u32) -> u64`, and `memory`.
  - **No imports at all.** A u64 return is `(ptr << 32) | len`.
  - `info` returns UTF-8 JSON: `{"name":"slidev","version":"<semver>","operations":["export","import"]}`.
  - `run`'s input and output are both a **bundle**: magic `b"SPB1"`, a u32 LE count, then per file a u32 LE name length, the UTF-8 name, a u32 LE data length and the data.
  - The input bundle has `.op` (`export` or `import`), `.args.json` (operation arguments) and the files.
  - The output bundle has the files plus `.report.json`. On failure the output bundle is just `.error` (a UTF-8 sentence).
  - Names in a bundle must pass the same safe-name rule as `.sptx` entries (no `..`, no absolute path, no `\`, no NUL); the host checks them.
- **Host limits for one run:**
  - an epoch deadline of 60 s, with the add-on's own clock thread or the components clock (`TICK` 10 ms);
  - 1 GiB of memory, with one memory and one table;
  - a 512 MiB input bundle;
  - a 512 MiB output bundle;
  - 10 000 files.

  Each refusal is one sentence that names the limit.
- **Signature:** ed25519 over `sha256(addon.json bytes) || sha256(addon.wasm bytes)`, stored raw (64 bytes) as `addon.sig` in the `.spaddon` zip.
  - The public key lives in `crates/app/src/addons/key.rs` as `pub const ADDON_PUBLIC_KEY: [u8; 32]`.
  - Tests use a key generated inside the test and the `test-hooks` feature's `set_addon_key_for_test`. Release builds never accept another key.
- **The installed add-on** lives at `<app data folder>/addons/slidev/`, holding `addon.json`, `addon.wasm` and `addon.sig`, written atomically (temp folder, then rename).
  - The app data folder is `BuildPermissions::app_folder()` (`dirs::data_dir()/stagepack-studio`). Tests set `XDG_DATA_HOME` to a scratch folder.
  - The signature is checked again on every load. A bad one makes the add-on show as damaged, with *Remove*.
- **The private key** is never in the repository, a test or a log. `package addon --key FILE` reads it.
- **Import never overwrites the original** and never runs anything from the Slidev talk.
- **Rules:**
  - No real decks, clients or organisations in commits, fixtures or docs. Fixtures are made up.
  - Stage files by name; never `git add -A`; never stage `docs/superpowers/plans/2026-11-09-roadmap.md`, `.superpowers/`, `.claude/` or `target/`.
  - Run cargo through `scripts/cargo-safe.sh`; never `cargo fmt`; never `pkill`; never run the GUI on the real display (the app's UI tests use the headless test harness the existing `crates/app/tests/ui_*.rs` use).
  - Never touch `~/.local/share/stagepack-studio`, `~/.cache/stagepack-studio` or `~/Documents`.
  - Commit trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A hostile add-on file.** This covers a zip with `../` names, a wasm with imports, a wrong signature, a huge `addon.wasm`, and a wasm that loops or eats memory. Each is refused with a sentence, and nothing is written outside `addons/`. Task 4 and Task 1 pin it with tests that cover each case.
2. **A Slidev talk the importer can't fully read.** This covers a Vue component, raw HTML, `<script setup>`, custom themes, `src:` pointing outside the talk, and a missing picture. Each becomes a labelled placeholder holding the original text, and is listed in the report under its slide. Nothing is dropped silently. Task 3 tests this with a fixture that contains each case.
3. **An export that Stagepack would refuse.** If any file or headmatter key is on the code list, Stagepack asks a code question. A test checks the export of every fixture talk against the full list in Global Constraints. Task 2 owns this.
4. **Text that breaks Slidev's Markdown.** This covers text with `---`, `<`, `{{ }}` (Vue interpolation), `::name::` and `<!--`. The export escapes it so it reads as text, with `v-pre` on the containing block for `{{`. Task 2 has a test for each case.
5. **A big talk.** A talk with 200 slides and 300 MB of video goes through the 512 MiB bundle limit. Video above the limit is linked by name and left out, with a notice. A refusal never truncates silently. Task 5 has a test with a synthetic large file.

---

### Task 1: The ABI, a wasm-ready parser and the host runner

**Files:**
- Create: `crates/addon-abi/{Cargo.toml,src/lib.rs}`. It holds bundle encode and decode, `ABI_VERSION`, the safe-name check, `Report` and the `Notice` types (serde).
- Modify: `crates/format/src/parse.rs`. On `cfg(target_arch = "wasm32")`, `read_slides` reads on the calling thread, with no `thread::scope`.
- Modify: `crates/format/Cargo.toml`, only if needed.
- Create: `crates/components/src/addon.rs` (`pub mod addon;`), with `Addon` and `AddonError`.
- Create: `crates/addon-abi/tests/bundle.rs`, `crates/components/tests/addon_host.rs`, and test guests in `crates/components/tests/addon-guests/` (tiny `.wat` files, compiled with the `wat` crate the components tests already use; if they don't, add `wat` as a dev-dependency).
- Modify: the workspace `Cargo.toml` members.

**Interfaces:**
- Produces:
  - `addon_abi::{ABI_VERSION: u32 = 1, Bundle, encode(&Bundle) -> Vec<u8>, decode(&[u8]) -> Result<Bundle, String>, safe_name(&str) -> bool, Report, Notice, NoticeKind}`.
  - `Bundle` is `Vec<(String, Vec<u8>)>`, in the order given.
  - `Report` is `{ notices: Vec<Notice> }`. `Notice` is `{ slide: Option<usize> /* 1-based */, kind: NoticeKind, text: String }`, and `NoticeKind` is `Placeholder | Approximation | LeftOut | Frozen`.
  - `slides_components::addon::Addon::load(wasm: &[u8]) -> Result<Addon, AddonError>`, `Addon::info(&self) -> &AddonInfo { name, version, operations }`, and `Addon::run(&self, op: &str, args_json: &str, files: Bundle) -> Result<(Bundle, Report), AddonError>`.
  - `AddonError` implements `Display` with one sentence each: `Malformed(String)`, `TooLarge(&'static str)`, `TookTooLong`, `OutOfMemory`, `Failed(String)` (the add-on's own `.error`) and `Trapped`.

- [ ] **Step 1: Bundle tests** (`crates/addon-abi/tests/bundle.rs`).
  - `round_trip` covers 3 files, one empty and one non-ASCII name.
  - `decode_refuses`: bad magic, a count larger than the bytes, a name length past the end, a non-UTF-8 name, a name that fails `safe_name`, and trailing bytes. Each is an `Err` naming the problem.
  - `safe_name_table`:
    - accepted: `slides.md`, `public/a b.png`;
    - refused: `../x`, `/x`, `a\b`, `a/../b`, `""`, `"a\0b"`, `C:x`.
- [ ] **Step 2:** Run `scripts/cargo-safe.sh test -p addon-abi`. It is expected to FAIL, because the crate is missing.
- [ ] **Step 3:** Implement `addon-abi`. It has no std-only deps beyond `serde` and `serde_json`, and `cargo build -p addon-abi --target wasm32-unknown-unknown` must pass.
- [ ] **Step 4: Format on wasm.** `cargo build -p slides-format --target wasm32-unknown-unknown` must succeed, and add a test that runs it: `crates/format/tests/wasm_build.rs::slides_format_builds_for_wasm32`. That test runs `cargo build` through `std::process::Command` with `--target-dir target/wasm-check`. It is `#[ignore]`, and test-all's components step runs it. The native behaviour is unchanged, and `scripts/cargo-safe.sh test -p slides-format` passes.
- [ ] **Step 5: Host tests** (`crates/components/tests/addon_host.rs`). The guests are `.wat`:
  - `a_good_guest_echoes_its_files`: the guest returns the input with `.report.json` `{"notices":[]}`.
  - `a_guest_with_an_import_is_refused`, giving `Malformed`.
  - `a_wrong_abi_is_refused`.
  - `a_guest_that_loops_is_stopped`, giving `TookTooLong`. To keep the test fast, use a host-side limit override in tests: `Addon::run_with_limits`, which is `pub` behind the `test-hooks` feature, at 200 ms.
  - `a_guest_that_grows_memory_is_stopped`, giving `OutOfMemory`.
  - `an_error_file_is_a_failure`, giving `Failed("...")` with the guest's sentence.
  - `an_unsafe_name_in_the_output_is_refused`.
  - `a_too_large_input_is_refused_before_running`, giving `TooLarge`.
- [ ] **Step 6:** Implement `addon.rs`. Reuse `sandbox.rs`'s process-wide `Engine` and clock (make the needed items `pub(crate)`), and add a separate limiter at 1 GiB. Call `info` once at load and cache the parsed JSON. Each `run` uses a fresh store and instance.
- [ ] **Step 7:** Run `scripts/cargo-safe.sh test -p addon-abi -p slides-components -p slides-format`. All tests pass.
- [ ] **Step 8:** Commit.

### Task 2: Export to Slidev, in the translator

**Files:**
- Create: `crates/slidev-addon/{Cargo.toml,src/lib.rs,src/export.rs,src/escape.rs,src/wasm.rs}`. It is `crate-type = ["cdylib","rlib"]`, and `src/wasm.rs` is `cfg(target_arch="wasm32")` with the ABI exports.
- Create: `crates/slidev-addon/tests/export.rs` and the expected files `crates/slidev-addon/tests/export/*.md`.
- Create: `fixtures/slidev-export/slides.md`, a made-up talk that uses every layout, a moved region, `<Box>`, `<Text>`, all six shape kinds, `<Icon>`, `<Picture>`, a table, a code fence, `click=` / `enter=` / `region.X: click=each`, transitions, `<Notes>`, `hidden: true`, and one custom component.

**Interfaces:**
- Consumes: Task 1's `Bundle`, `Report` and `Notice`; and `slides_format::parse_talk`, `Talk`, `Slide`, `Block`, `Tag`, `Span` and `Settings`.
- Produces:
  - `slidev_addon::export(args: &ExportArgs, files: &Bundle) -> Result<(Bundle, Report), String>`.
  - `ExportArgs` (serde, from `.args.json`) is `{ title: String }`.
  - The input files are `slides.md`, every `assets/**` file, and `baked/<slide>/<n>.png` (and `baked/<slide>/<n>@<step>.png` for a component that changes with clicks). `<slide>` is 1-based and `<n>` is the 0-based index of the tag in the slide's document order among the tags that are baked. A host-made `baked/index.json` lists them: `[{ "slide": 1, "n": 0, "tag": "Chart", "x":..,"y":..,"w":..,"h":..,"rotate":..,"steps":[0,1,2], "animated": false }]`.
  - The output files are `slides.md`, `public/**`, and `.stagepack.json`. The last one holds only `{"title":...}`; the app adds the rest.

The mapping, by spec. The engine's own layout boxes are in `crates/engine/src/layouts.rs` (read the region rectangles from there).

| Ours | Slidev |
|---|---|
| layout `title` | `layout: cover` |
| layout `content` | `layout: default` |
| layout `two-columns` | `layout: two-cols`, with `::right::` before the right region |
| layout `section` | `layout: section` |
| layout `blank` | `layout: none` |
| a region moved by `region.NAME:` | `<div style="position:absolute;left:Xpx;top:Ypx;width:Wpx;height:Hpx;transform:rotate(Rdeg)">` holding the region's Markdown |
| `<Box …>` | a positioned `<div>` with `background`, `border`, `border-radius` and `padding`, in px |
| `<Text color size bold italic underline>` | `<span style="…">` |
| `<Shape kind=…>` | inline `<svg>` drawn by the add-on: rect, rounded, ellipse, line, arrow (a line plus a marker), and callout (rounded rect plus a tail), sized to the box |
| `<Picture src="assets/x">` | `<img src="/x">`, copying the file to `public/x` |
| `<Icon>`, chart, custom component, model, terminal, and video's poster frame | `<img src="/baked-<slide>-<n>.png">` at the index's box; a stepped one gives one `<img v-if="$clicks === k">` per step; an animated one is frozen, with a `Frozen` notice |
| `<Video src>` | `<video src="/x" controls>`, copying the file |
| `click="N"` | `v-click="N"`; `click="each"` on a list becomes `<v-clicks>`; `enter=fade` gives a `.slidev-vclick-target` fade (the default), and `fly-up` becomes `v-motion :initial="{y:40,opacity:0}" :click-N="{y:0,opacity:1}"` |
| `transition:` none / fade / push | `transition: none` / `fade` / `slide-left` |
| `hidden: true` | `hide: true` |
| `<Notes>` | a trailing HTML comment, `<!-- … -->`, with `-->` inside escaped as `--&gt;` |
| talk colours and fonts | one `<style>` block on the first slide that sets `--slidev-theme-primary`, the slide's background and colour, and `font-family` (with no font files fetched); a font of the talk's own in `fonts/` is copied to `public/fonts/` with `@font-face` |

- [ ] **Step 1: Write the tests** in `crates/slidev-addon/tests/export.rs`.
  - `each_layout_maps`, `a_moved_region_is_a_positioned_div`, `boxes_text_and_shapes`, `pictures_are_copied_to_public`, `baked_parts_are_images_at_their_box`, `a_stepped_component_switches_on_clicks`, `clicks_and_entries`, `transitions_hidden_and_notes` and `talk_colours_become_one_style_block`. Each compares the `slides.md` of a one-slide input against an expected file in `tests/export/`.
  - `the_fixture_exports_with_no_code_reasons`: export `fixtures/slidev-export` with its baked index made by hand in the test. Every output name and the headmatter are checked against the Global Constraints list.
  - `text_that_slidev_would_read_is_escaped`: `---`, `<`, `{{ x }}`, `::right::` and `<!--` in plain text each come out as literal text.
  - `a_slide_with_an_error_is_left_out_with_a_notice`.
- [ ] **Step 2:** Run `scripts/cargo-safe.sh test -p slidev-addon --test export`. It is expected to FAIL.
- [ ] **Step 3:** Implement `export.rs` and `escape.rs`, and `wasm.rs` (the `run` dispatch on `.op`). `cargo build -p slidev-addon --release --target wasm32-unknown-unknown` must pass.
- [ ] **Step 4:** Run the tests again. They are expected to PASS.
- [ ] **Step 5:** Commit.

### Task 3: Import from Slidev, in the translator

**Files:**
- Create: `crates/slidev-addon/src/import.rs` and `src/headmatter.rs` (`yaml-rust2`).
- Create: `crates/slidev-addon/tests/import.rs` and `tests/import/*.md`.
- Create: `fixtures/slidev-import/` (a made-up Stagepack talk folder):
  - `slides.md`, which uses headmatter, per-slide frontmatter, the layouts `cover`, `default`, `two-cols` (with `::right::`), `section`, `center`, `image-right` and `none`, `v-click`, `<v-clicks>`, `<v-click>`, transitions, notes comments, a code fence, a `src:` import of `pages/extra.md`, a Vue component `<Counter />`, raw `<div class="grid">`, a `<script setup>`, a custom `theme: seriph`, and a `public/` picture;
  - `pages/extra.md`;
  - `public/photo.png`;
  - `stagepack.json`.

**Interfaces:**
- Produces:
  - `slidev_addon::import(args: &ImportArgs, files: &Bundle) -> Result<(Bundle, Report), String>`.
  - `ImportArgs` is `{ entry: String }`, from the manifest's `entry` and defaulting to `slides.md`.
  - The input is every entry of the Stagepack zip except `stagepack.json` and `vendor/**`; the app leaves out the vendor files, and the add-on reports them once as `LeftOut`.
  - The output is `slides.md` (our syntax) and `assets/**`.

The mapping:
- **Layouts:** `cover` and `intro` → `title`; `default` and `center` → `content`; `two-cols` and `two-cols-header` → `two-columns`; `section` → `section`; `none` and `full` → `blank`. Anything else → `content`, with an `Approximation` notice that names the layout.
- **Slots:** `::right::` → `<Region name="right" />`. Other slot names → the main region, with a notice.
- **Clicks:**
  - `v-click` on an element, and `<v-click>…</v-click>`, become `click="N"` on a `<Box>` around it. N counts the slide's clicks in order, the way Slidev counts them.
  - `<v-clicks>` around a list becomes `click="each"` on it.
  - `v-after` takes the previous number.
  - `at="+1"` or a number is handled; anything else gets a notice.
- **Transitions:** `fade` → `fade`; `slide-*` → `push`; others → `none`, with a notice.
- **Notes:** the trailing `<!-- -->` → `<Notes>`.
- **`hide: true`** → `hidden: true`.
- **`src:` imports:** inlined, as Slidev does. An import outside the bundle, or missing, becomes a placeholder.
- **`public/x`** → `assets/x`, and `/x` references are rewritten to `assets/x`.
- **Not converted:** Vue components, unknown HTML tags, `<script>`, `<style>` and custom themes. Each becomes a labelled placeholder: a `<Box>` titled "From Slidev, not converted: <what>" holding the original text in a fenced code block. Each is listed in the report with its slide. The theme is a notice on slide 1.
- **Headmatter** `title` → the talk's `title`; headmatter colours are ignored, with a notice. Nothing is ever evaluated.

- [ ] **Step 1: Write the tests.**
  - `each_layout_maps`, `slots_become_regions`, `clicks_in_order`, `v_clicks_on_a_list`, `notes_comments`, `transitions_and_hide`, `src_imports_are_inlined` and `public_files_move_to_assets`. These compare against expected files.
  - `vue_html_and_script_become_placeholders_and_are_reported`: one notice per thing, with the right slide numbers, and the original text present in the output.
  - `an_import_outside_the_talk_is_a_placeholder`.
  - `the_fixture_imports_and_parses_cleanly`: the output parses with `slides_format::parse_talk`, with no slide errors.
  - `export_then_import_keeps_text_layouts_clicks_and_notes`: running the export fixture through export and then import keeps every heading's text, every layout, the click count per slide, and the notes.
- [ ] **Step 2:** Run the tests. They are expected to FAIL.
- [ ] **Step 3:** Implement the import. Add `import` to `wasm.rs`.
- [ ] **Step 4:** Run the tests. They are expected to PASS. `cargo build -p slidev-addon --release --target wasm32-unknown-unknown` must still pass.
- [ ] **Step 5:** Commit.

### Task 4: The `.spaddon` package, its signature and the owner's key

**Files:**
- Create: `crates/addon-abi/src/package.rs`:
  - `read_package(bytes: &[u8], key: &[u8; 32]) -> Result<Package, String>`, checking the signature;
  - `Package { manifest: AddonManifest, wasm: Vec<u8>, manifest_bytes: Vec<u8>, sig: [u8;64] }`;
  - `AddonManifest { name, version, abi: u32, slidev: String }`.

  It uses `ed25519-dalek` (verify only, `default-features=false`) and `sha2`. The zip is read with `zip` =9.5.0 (deflate) and these limits: exactly the 3 entries, `addon.wasm` up to 32 MiB, `addon.json` up to 64 KiB, safe names only.
- Create: `crates/xtask/src/addon.rs`, with the `package addon --key FILE [--out DIR]` and `package addon-keygen FILE` subcommands in `main.rs`.
- Create: `crates/app/src/addons/key.rs`.
- Tests: `crates/addon-abi/tests/package.rs` and `crates/xtask/tests/addon.rs`.

**Interfaces:**
- Produces:
  - `addon_abi::package::{read_package, write_package(manifest: &AddonManifest, wasm: &[u8], sign: impl Fn(&[u8]) -> [u8; 64]) -> Vec<u8>, signed_message(manifest_bytes, wasm) -> [u8; 64] /* the two sha256 */}`.
  - The file name is `slidev-<version>.spaddon`.
  - `package addon` builds `slidev-addon` for wasm32 in release mode, with `--remap-path-prefix` (`release_flags`). It writes `target/package/slidev-<version>.spaddon` and prints its sha256.
  - `package addon-keygen FILE` writes 32 raw secret bytes with mode 0600, refuses to overwrite, and prints the public key as a Rust array.

- [ ] **Step 1: Package tests.**
  - `a_signed_package_reads`.
  - `a_changed_wasm_byte_is_refused`, `a_changed_manifest_is_refused` and `the_wrong_key_is_refused`.
  - `extra_entries_or_unsafe_names_are_refused`.
  - `a_huge_wasm_is_refused_before_hashing`.
  - `a_wrong_abi_or_name_is_refused`: the name must be `slidev`, and `abi` must be 1.
- [ ] **Step 2:** Run them. They are expected to FAIL. Then implement the package code and run them again. They are expected to PASS.
- [ ] **Step 3: xtask tests.**
  - `addon_keygen_writes_a_private_file_and_refuses_to_overwrite` (scratch folder).
  - `package_addon_signs_what_read_package_accepts`. This is `#[ignore]` because it builds wasm. It uses a scratch key, and the package must pass `read_package` with that key's public half. Run it.
- [ ] **Step 4: The owner's key.**
  - Run `scripts/cargo-safe.sh run -q -p xtask --bin package -- addon-keygen "$HOME/stagepack-studio-signing/addon-signing.key"`. Create the folder with mode 0700; this is the one file outside the repository this plan writes.
  - Put the printed public key into `crates/app/src/addons/key.rs` as `ADDON_PUBLIC_KEY`.
  - Never print, copy or commit the secret file.
- [ ] **Step 5:** Commit. Do not commit the key file.

### Task 5: The app: the Add-ons dialog, Export ▸ Slidev… and import

**Files:**
- Create:
  - `crates/app/src/addons/mod.rs`: `Installed`, `install_from_file`, `remove` and `load`;
  - `crates/app/src/addons/bake.rs`: the baked pictures and `baked/index.json` for an export;
  - `crates/app/src/addons/dialog.rs`: the Add-ons dialog.
- Modify:
  - `crates/app/src/export.rs`: `ExportKind::Slidev`, run through the add-on;
  - `crates/app/src/workspace_view.rs`: the export menu, an *Add-ons…* item in the File menu, and the action wiring;
  - `crates/app/src/workspace_view/starting.rs` and `document/errors.rs`: a Slidev talk with the add-on installed imports;
  - `crates/app/Cargo.toml`: deps on `addon-abi` and `slides-components` (already present);
  - `README.md` and the guide `docs/guide/components.md`, only if it mentions exports.
- Test:
  - `crates/app/tests/addons.rs` (logic);
  - `crates/app/tests/ui_addons.rs` (headless UI, as in `ui_start.rs`);
  - `crates/app/tests/slidev_round.rs`, an ignored test that builds the real wasm.

**Interfaces:**
- Consumes:
  - `Addon::load` and `Addon::run` (Task 1);
  - `read_package` and `ADDON_PUBLIC_KEY` (Task 4);
  - `slidev_addon` behaviour (Tasks 2 and 3), only through the wasm;
  - `pack_new` and `write_atomically` (engine);
  - `render_layer` and `Deck` (engine), as `crates/pptx/src/write.rs` uses them.
- Produces:
  - `addons::Installed { version: String, state: InstalledState /* Ready(Addon) | Damaged(String) */ }`;
  - `addons::load(app_folder: &Path) -> Option<Installed>`;
  - `addons::install_from_file(app_folder, file: &Path) -> Result<Installed, String>`, which replaces any earlier version;
  - `addons::remove(app_folder) -> Result<(), String>`;
  - `bake::bake(deck: &mut Deck, files) -> Bundle`, giving the `baked/**` files and `baked/index.json` at 2× scale;
  - `export::export_slidev(input: ExportInput, addon: &Addon, out: &Path, now, id) -> Result<Vec<String> /* warnings, from the report */, String>`;
  - `import_slidev(file: &Path, addon: &Addon) -> Result<(String /* slides.md */, Vec<NewFile>, Report), String>`.

Behaviour:
- **The Add-ons dialog** opens from *File ▸ Add-ons…*. It shows one row, "Slidev — export to and import from Slidev (for Stagepack)", with its state:
  - *Not installed*, with an *Install from file…* button (F2 adds *Get*);
  - *Installed, version X*, with *Remove* and *Install from file…*;
  - *Damaged: <sentence>*, with *Remove*.

  The file picker filters `*.spaddon`. An install that fails says why in one line and leaves any earlier install as it was.
- **The export menu** gains *Slidev…*, which is disabled with the tooltip "Install the Slidev add-on from File ▸ Add-ons" when it isn't installed. The save dialog suggests `<talk name> (Slidev).sptx`. The add-on's notices become the export's warnings, shown the way PowerPoint export shows its own.
- **Opening a Stagepack `.sptx`:**
  - With the add-on installed, it imports into a new, unsaved talk (like *New*, with no file path), then shows a report dialog listing the notices by slide. The original is never written to.
  - Without the add-on, the current sentence stays, and gains an *Add-ons…* button beside *Open in Stagepack*.
  - The existing tests `ui_start.rs:978-1014` keep passing, with the not-installed text extended.
- **Export writes the manifest:** the app writes `stagepack.json` from the add-on's `.stagepack.json` title plus the Global Constraints fields. The zip is written through `write_atomically` with `slides.md` first. Stagepack's reader takes any entry order, so the order is our choice.
- **Size:** a bundle over 512 MiB isn't sent. Video files are left out first, each with a notice ("<file> is too large to include; copy it into public/ yourself"), until the bundle fits. If it still doesn't fit, the export is refused with a sentence.

- [ ] **Step 1: Logic tests** (`crates/app/tests/addons.rs`). They use `XDG_DATA_HOME` set to a scratch folder, and a test key through `set_addon_key_for_test`.
  - `install_from_file_puts_three_files_in_addons_slidev`.
  - `a_bad_signature_is_refused_and_an_earlier_install_stays`.
  - `a_damaged_install_loads_as_damaged`, where a changed byte in `addon.wasm` makes the add-on Damaged.
  - `remove_takes_only_the_addon_folder`.
  - `bake_writes_an_index_and_a_png_per_baked_tag`, using `fixtures/slidev-export`.
  - `an_export_over_the_limit_leaves_video_out_with_a_notice` (a synthetic sparse 600 MB file).
  - `import_leaves_the_original_untouched`: hash it before and after.
- [ ] **Step 2: UI tests** (`crates/app/tests/ui_addons.rs`).
  - `the_addons_dialog_shows_not_installed_then_installed`.
  - `slidev_export_is_disabled_until_installed`.
  - `a_stagepack_talk_without_the_addon_offers_add_ons`.
  - `a_stagepack_talk_with_the_addon_opens_unsaved_with_a_report`.
- [ ] **Step 3: Round trip** (`crates/app/tests/slidev_round.rs`). This test is `#[ignore]` and test-all's components step runs it. It builds the real wasm through `package addon` with a scratch key, installs the package, and exports `fixtures/slidev-export` to a `.sptx`. Then it checks three things:
  - the zip has `stagepack.json` with exactly the Global Constraints keys and `slidev: "53.0.0"`;
  - no file is on the code list;
  - importing that `.sptx` again gives a talk that parses with no slide errors, with a notice for each baked part.
- [ ] **Step 4:** Run `scripts/cargo-safe.sh test -p stagepack-studio --test addons --test ui_addons`. The tests are expected to FAIL. Then implement, and run them again. They are expected to PASS. Run the ignored round trip as well. Then run `scripts/cargo-safe.sh clippy --workspace --all-targets -- -D warnings`, which must be clean.
- [ ] **Step 5: Wire into test-all.** `scripts/test-all.sh` and `scripts/test-all.ps1`: the components step adds the ignored `wasm_build`, `package_addon_signs_what_read_package_accepts` and `slidev_round`.
- [ ] **Step 6:** Commit.

### Task 6: The record

- Create `docs/checks/plan-f1.md`, in plan-e2.md's shape. It covers:
  - what was built;
  - the test counts from the commands' own output;
  - the add-on file's size and sha256;
  - where the owner's key is, and that it must be backed up and never shared;
  - hand checks for the owner: install the add-on on Windows and Linux, export the showcase talk, open it in Stagepack (no code question, it presents), and import a real Stagepack talk of the owner's own and read the report;
  - "## The final review", left for the controller.
- [ ] Commit.
