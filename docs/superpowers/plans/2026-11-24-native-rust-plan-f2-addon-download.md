# Plan F2: Get and update the Slidev add-on — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Add-ons dialog gets a *Get* button and a *Check for updates* button. Each downloads the signed `.spaddon` over HTTPS from a public releases repository, checks it with the same signature gate as *Install from file*, and installs it. This happens only when the user clicks, never in the background.

**Architecture:**
- **The fetch module.** A new module, `crates/app/src/addons/fetch.rs`, fetches a small `index.json` from a fixed HTTPS address. That index names the version and the package file.
- **Checking the download.** The module downloads the file with hard size and time caps and checks its sha256 against the index. Then it hands the bytes to the existing `install_from_bytes` path, which runs `read_package` and the ed25519 check.
- **The index proves nothing.** It is untrusted. Only the signature decides what runs.
- **Publishing.** `package addon-release` in xtask writes the index next to the package, for the owner to upload.

**Tech Stack:**
- Rust 1.98;
- `ureq` (3.x, exactly pinned at the current release), with rustls and webpki-roots and no native-tls. Its TLS stack is rustls 0.23, which is already in the lockfile;
- gpui-kit 0.9.0;
- the crates from F1: addon-abi and `addons::*`.

**Spec:** the design doc `docs/superpowers/specs/2026-11-09-native-rust-slides-design.md` ("The Slidev add-on", "Getting it"), plan F1 (`docs/superpowers/plans/2026-11-24-native-rust-plan-f1-slidev-addon.md`), and the owner's answer of 2026-11-24 that the add-on is published in a separate **public** repository.

## Global Constraints

- **Network only on a click.** The app opens no connection unless the user clicks *Get*, *Check for updates* or *Update*. No request is made at start-up, on a timer, or on opening a Slidev talk. A test asserts that the app's start-up path never calls the fetch module.
- **The addresses** are constants in `fetch.rs`:
  - `INDEX_URL = "https://github.com/Eiriksb/stagepack-studio-addons/releases/latest/download/index.json"`;
  - `PACKAGE_PREFIX = "https://github.com/Eiriksb/stagepack-studio-addons/releases/download/"`. The index's `url` must start with exactly this prefix.
  - Redirects are followed only to `https://` addresses, and at most 5 of them, because GitHub sends release files to its own storage host.
  - Plain `http` is refused everywhere, except `http://127.0.0.1:<port>/` in tests behind the `test-hooks` feature, via `set_source_for_test`.
- **Nothing personal is sent.** The only headers are `User-Agent: stagepack-studio/<CARGO_PKG_VERSION>` and `Accept`. There are no cookies, no query strings and no telemetry.
- **The caps:**
  - the index can be at most 64 KiB;
  - the package can be at most 48 MiB. Both caps apply while reading, whatever `Content-Length` says;
  - connecting has 10 s;
  - the index has 20 s in total;
  - the package has 180 s in total.

  Each refusal is one sentence.
- **The index format** is `{"name":"slidev","version":"X.Y.Z","slidev":"53.0.0","abi":1,"size":N,"sha256":"<64 hex>","url":"<PACKAGE_PREFIX>slidev-vX.Y.Z/slidev-X.Y.Z.spaddon"}`. Every field is required, and unknown fields are refused.
  - `size` must match the downloaded length, and `sha256` must match the downloaded bytes.
  - After the signature check, `read_package`'s manifest must agree with the index's `name`, `version`, `abi` and `slidev`.
- **Versions** are compared as three numbers. *Update* is offered only for a strictly newer version. A downgrade is never offered, whatever the index says.
- **Installing** goes through the F1 path only: verify, then the same atomic install with its lock. A failed download changes nothing on disk except a temporary file in `addons/`, which is removed afterwards.
- **The UI thread is never blocked.** Downloads run in the background with progress, and *Cancel* stops them within 1 s.
- **Rules:**
  - No real decks, clients or organisations in commits or docs.
  - Stage by name, never `git add -A`, and never stage `docs/superpowers/plans/2026-11-09-roadmap.md`, `.superpowers/`, `.claude/` or `target/`.
  - Use `scripts/cargo-safe.sh` for cargo, never `cargo fmt`, never `pkill`, and never run the GUI on the real display.
  - Never touch `~/.local/share/stagepack-studio`, `~/.cache/stagepack-studio` or `~/Documents`.
  - No test reaches the internet.
  - The commit trailer is `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A hostile or broken index or server.** This covers:
   - an index larger than 64 KiB;
   - a package that never stops streaming;
   - a `url` outside `PACKAGE_PREFIX`;
   - a redirect to `http://` or `file:`;
   - a sha256 or size that doesn't match;
   - an index that names a version the signed manifest contradicts.

   Each is refused with a sentence, and nothing is installed. Task 1's tests cover each case with a local server.
2. **No network, or a captive portal.** With DNS failing or the connection refused, the user sees "Couldn't reach the add-on's download page. Check the internet connection, or install it from a file." A captive portal that answers with HTML fails the index parse with the same kind of sentence. Task 1 pins both.
3. **Cancel during a download.** The temporary file is gone and the old install is untouched. Task 2's UI test pins this.
4. **An update while the add-on is in use.** If an export or import is running, *Update* waits for the lock, or says the add-on is in use (F1's `BUSY`). It never swaps the files under a running export. Task 2 pins this.
5. **The release files the owner uploads.** `package addon-release` writes an `index.json` whose `sha256` and `size` match the `.spaddon` beside it, and whose `url` matches the tag the published steps use. Task 3 pins this.

---

### Task 1: The fetch module

**Files:**
- Create: `crates/app/src/addons/fetch.rs` (`pub mod fetch;`).
- Modify: `crates/app/src/addons/mod.rs`. Factor out `install_from_bytes(app_folder, bytes: &[u8]) -> Result<Installed, String>`, which `install_from_file` then calls.
- Modify: `crates/app/Cargo.toml`. Add `ureq`, exactly pinned, with default features off and `rustls`, `platform-verifier` off and `webpki-roots`. Pick whatever feature names give rustls with the webpki roots and no native-tls. Also add `serde_json` (present already) and `sha2` (present already).
- Test: `crates/app/tests/addon_fetch.rs`. Its local server is a small `std::net::TcpListener` HTTP/1.1 server written in the test file, with no new dependency.

**Interfaces:**
- Produces:
  - `fetch::Index { name, version, slidev, abi, size, sha256, url }`.
  - `fetch::Source { index_url: String, package_prefix: String }`, with `Source::official()`.
  - `fetch::check(source: &Source, cancel: &AtomicBool) -> Result<Index, String>`, which fetches and validates the index.
  - `fetch::download(source: &Source, index: &Index, cancel: &AtomicBool, progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String>`.
  - `fetch::newer(installed: &str, offered: &str) -> bool`.
  - `#[cfg(feature = "test-hooks")] fetch::set_source_for_test(Source)`. Loopback `http` is allowed only through this hook.

- [ ] **Step 1: Write the failing tests.** Each runs against the local server:
  - `a_good_index_and_package_install` (signed with the shared test key);
  - `an_index_over_64_kib_is_refused`;
  - `an_endless_package_is_cut_at_48_mib`;
  - `a_url_outside_the_prefix_is_refused_before_connecting`;
  - `a_redirect_to_http_is_refused`;
  - `a_wrong_sha256_or_size_is_refused`;
  - `an_index_that_disagrees_with_the_signed_manifest_is_refused`;
  - `html_instead_of_an_index_says_so` (the captive portal);
  - `a_refused_connection_says_check_the_internet`;
  - `cancel_stops_within_a_second`;
  - `newer_compares_numbers` (0.1.10 > 0.1.9; equal isn't newer; malformed isn't newer);
  - `http_is_refused_without_the_test_hook`.
- [ ] **Step 2:** Run `scripts/cargo-safe.sh test -p stagepack-studio --features test-hooks --test addon_fetch`. Expected: it fails.
- [ ] **Step 3:** Implement. Use ureq's agent config for the timeouts and redirects, plus a manual redirect check if ureq can't restrict the scheme. Read with `take(cap + 1)`.
- [ ] **Step 4:** Run the tests again. Expected: they pass. Also run `clippy -p stagepack-studio --all-targets -- -D warnings`, which must be clean.
- [ ] **Step 5:** Add `ureq` and its new dependencies to the NOTICES walk if the walk doesn't pick them up by itself. Then run `scripts/cargo-safe.sh test -p xtask --test xtask`. Expected: `every_dependency_has_its_notice` passes.
- [ ] **Step 6:** Commit.

### Task 2: Get, Check for updates and Update in the Add-ons dialog

**Files:**
- Modify: `crates/app/src/addons/dialog.rs`, and `workspace_view/starting.rs` if the Slidev-talk offer needs a direct *Get*.
- Test: `crates/app/tests/ui_addons.rs`, with the fetch source pointed at a local server.

Behaviour:
- **Not installed:** the dialog shows *Get* and *Install from file…*.
  - *Get* checks the index, downloads and installs. While it runs, the dialog shows "Downloading… N of M MB" and *Cancel*. When it's done: "Installed, version X".
- **Installed:** the dialog shows *Check for updates*, *Remove* and *Install from file…*.
  - When a newer version exists, the row says "Version Y is available" and shows *Update*.
  - Otherwise it says "This is the newest version".
- **Errors** appear as a single line in the row.
- **Opening a Slidev talk without the add-on:** the existing *Add-ons…* button opens the dialog. Nothing downloads until *Get* is clicked.
- **While an export or import holds the add-on,** *Update* says F1's `BUSY` sentence.

- [ ] **Step 1: Write the failing UI tests.**
  - `get_installs_from_the_local_server`
  - `check_for_updates_offers_a_newer_version_only`
  - `cancel_leaves_nothing_behind`
  - `an_update_while_the_addon_is_busy_says_so`
  - `opening_the_dialog_makes_no_request`: the server counts requests and expects 0.
  - `start_up_makes_no_request`
- [ ] **Step 2:** Implement. Run the tests, then the whole app suite, then `clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 3:** Commit.

### Task 3: The release files, the steps and the record

**Files:**
- Modify: `crates/xtask/src/addon.rs`, adding `package addon-release [--package FILE]`. It writes `target/package/index.json` for the `.spaddon` beside it, and prints the owner's publishing steps.
- Test: `crates/xtask/tests/addon.rs`, with the test `the_release_index_matches_its_package`.
- Create: `docs/guide/publishing-the-addon.md`. These are the owner's steps:
  1. Create the public repository `Eiriksb/stagepack-studio-addons` once.
  2. Run `package addon --key …`.
  3. Run `package addon-release`.
  4. Run `gh release create slidev-vX.Y.Z target/package/slidev-X.Y.Z.spaddon target/package/index.json --repo Eiriksb/stagepack-studio-addons --title "Slidev add-on X.Y.Z" --latest`.
  5. Check it in the app with *Check for updates*.

  The guide also says that the newest release must always hold `index.json`, since that is what *Get* reads. Nobody but the owner runs these steps: the plan never creates a repository or a release.
- Modify: `README.md`, with one line pointing to the guide.
- Create: `docs/checks/plan-f2.md`, in plan-f1's shape. It covers:
  - what was built;
  - the test counts from the commands' own output;
  - the hand checks:
    - publish 0.1.0 as above;
    - on a clean user account, *Get* installs it;
    - *Check for updates* says it is the newest;
    - with the network off, the sentence is right;
    - on Windows, the same steps.
  - "## The final review", left for the controller.

- [ ] **Step 1:** Write the test, then make it pass.
- [ ] **Step 2:** Write the guide, the README line and the record.
- [ ] **Step 3:** Commit.
