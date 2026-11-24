# Plan F2: getting the Slidev add-on

The run was on the `plan-f` branch, on a Linux laptop, on 2026-11-24. The branch hasn't been pushed or merged. **Nothing here has run on Windows, and nothing has run against the real internet:** every test uses a small local server, and no repository or release exists yet. The owner publishes the first one (`docs/guide/publishing-the-addon.md`) and does the hand checks below.

## What was built

The app can get the add-on itself, from the releases of `Eiriksb/stagepack-studio-addons`, only when the owner clicks. Installing still goes through the F1 path: the signature is checked, then the same atomic install.

- **The fetch module** (task 1, `5ededc2`, `22a083a`). `fetch::check` reads the index of the app's own line and `fetch::download` reads the package, over https only (ureq with rustls and the bundled web roots).
  - Caps: the index at most 64 KiB, the package at most 48 MiB, both counted while reading. Connecting has 10 s, the index 20 s in total, the package 180 s.
  - Redirects are followed by hand, at most 5, to `https://` addresses that aren't local or private. Every name is checked again once it is resolved: only globally routable addresses are connected to (final fix wave).
  - Each request sends only `User-Agent` and `Accept`: no cookies, no query string.
  - The index must name `slidev`, three numbers, ABI 1 and a `url` under `PACKAGE_PREFIX`; `size` and `sha256` must match the download; and after the signature check, the signed manifest must agree with the index.
  - Every refusal is one sentence that echoes nothing the server wrote.
- **Get, Check for updates and Update** (task 2, `d19582b`, `9624ee0`). Buttons in *File ▸ Add-ons…*, with progress in MB and a *Cancel* that stops within a second.
  - A download runs on its own thread. Nothing is requested at start-up, on a timer or on opening a talk; a test pins that.
  - An update is offered only for a strictly newer version, and an install that would go back is refused even if the check lost a race.
  - Update says the add-on is in use while an export or import runs, and keeps the download for the next try.
  - A failed or cancelled download writes nothing; it is held in memory and dropped.
  - A damaged install, as an app update to another Slidev leaves one, offers *Get again*, which replaces it (final fix wave).
- **The release files** (task 3, `1eff7dc`; reworked in the final fix wave). `package addon-release [--package FILE]` writes `target/package/index-abi<abi>-slidev<version>.json` beside the signed package: its `size`, `sha256` and `url` (`PACKAGE_PREFIX` + `slidev-vX.Y.Z/slidev-X.Y.Z.spaddon`) come from the package itself, and it reads the package with the app's built-in key first, so a package the app would refuse gets no index. It is written with the same `addon_abi::package::Index` type the app reads. It then prints the commands to publish. The steps are in `docs/guide/publishing-the-addon.md`.
- **An index for each line of apps** (final fix wave). Each package has a release of its own, `slidev-vX.Y.Z`. One release, tagged `index`, holds an index file for each line of add-on interface and exact Slidev version. The app asks only for its own line's, at an address that never moves: `INDEX_URL` = `PACKAGE_PREFIX` + `index/index-abi1-slidev53.0.0.json`, built from `addon_abi::ABI_VERSION` and the app's `SLIDEV_VERSION` by `addon_abi::package::index_url`, which the packaging tool's file name shares. So an app that has shipped keeps finding the last add-on made for it after the owner publishes one for a later Slidev or interface. Nothing reads GitHub's *latest* release any more, so `--latest` is gone. The index is a release file like the package, `releases/download/<tag>/<file>`, so it takes the same redirect to GitHub's storage, and passes the same checks.

## Tests on Linux

All through `scripts/cargo-safe.sh`. Counts are from each report's own command output.

| Command | Result |
|---|---|
| `test -p stagepack-studio --test addon_fetch` (task 1, after its fix round) | 13 passed |
| `test -p stagepack-studio` (after task 2's fix round) | 1529 passed, 0 failed |
| `test -p xtask --test addon` (task 3) | 4 passed, 0 failed, 1 ignored (the add-on build, run by name) |
| `test -p stagepack-studio --test addon_fetch` (task 3, after the constants moved) | 13 passed |
| `test -p addon-abi` (task 3) | 24 passed (10 bundle, 14 package) |
| `clippy -p xtask -p addon-abi -p stagepack-studio --all-targets -- -D warnings` (task 3) | clean |
| `test -p xtask --test xtask` (task 1) | passed in task 1 (its report counts 25 tests across the xtask suites; this binary has 9 today), among them `every_dependency_has_its_notice`, the check that NOTICES covers the new crates |

Tasks 1 and 2 each went through one fix round and a re-review that approved it. The re-reviews' minor points, and the final review's, were all made in the final fix wave; its runs are below.

### The final fix wave

| Command | Result |
|---|---|
| `test -p addon-abi` | 26 passed (10 bundle, 16 package) |
| `test -p xtask --test addon --test xtask` | addon: 4 passed, 1 ignored (the add-on build, run by name); xtask: 9 passed, among them `every_dependency_has_its_notice` |
| `test -p stagepack-studio --test addon_fetch --test addons --test ui_addons` | 13, 31 and 19 passed |
| `test -p stagepack-studio --lib addons::fetch` | 7 passed (the resolver's allow-list, with a stub resolver) |
| `test -p stagepack-studio` (first run) | stopped at `ai_tool`: `a_picture_of_a_model_ends_cleanly_and_soon` took 68.5 s against its 20 s, under the load of the run; alone it passed in 1.0 s |
| `test -p stagepack-studio --no-fail-fast` | 1542 passed, 0 failed, 42 ignored |
| `clippy --workspace --all-targets -- -D warnings` | clean |

## What was run on the real file

`package addon-release` was run on `target/package/slidev-0.1.0.spaddon`, signed with the owner's key earlier. No key was read; the index is made from the package alone. In the final fix wave it wrote `target/package/index-abi1-slidev53.0.0.json`:

```json
{
  "name": "slidev",
  "version": "0.1.0",
  "slidev": "53.0.0",
  "abi": 1,
  "size": 394313,
  "sha256": "b6365436804b7ed481ceedc101c6210fc29989a04ceac8427ad48705a1ff376e",
  "url": "https://github.com/Eiriksb/stagepack-studio-addons/releases/download/slidev-v0.1.0/slidev-0.1.0.spaddon"
}
```

and printed:

```
To publish (the full steps are in docs/guide/publishing-the-addon.md):
Once only, before the first release (skip it if the release index is there already):
  gh release create index --repo Eiriksb/stagepack-studio-addons --title "Add-on index" --notes "The index files the app reads, one for each line of apps. Never delete them."
The package, in a release of its own:
  gh release create slidev-v0.1.0 target/package/slidev-0.1.0.spaddon --repo Eiriksb/stagepack-studio-addons --title "Slidev add-on 0.1.0" --notes "Slidev add-on 0.1.0"
Then its index, in place of the earlier one, once the package is up:
  gh release upload index target/package/index-abi1-slidev53.0.0.json --clobber --repo Eiriksb/stagepack-studio-addons
Never upload the signing key.
```

Nothing was published, and no repository or release was created.

## Hand checks for the owner

Do each on **Linux and Windows**.

1. **Publish 0.1.0** by `docs/guide/publishing-the-addon.md`: create the public repository once, with a README (`gh repo create … --public --add-readme`), and the release tagged `index`; build the package with your key; run `package addon-release`; run the two commands it prints for the release, the package's and then the index's. Check that the release `slidev-v0.1.0` holds the `.spaddon` alone, and the release `index` holds `index-abi1-slidev53.0.0.json`.
2. **Get.** On a clean user account (no add-on installed), *File ▸ Add-ons… ▸ Get*. Progress shows, then "Installed, version 0.1.0", and an export works.
3. **Check for updates.** It says this is the newest version.
4. **With the network off,** *Get* or *Check for updates* shows "Couldn't reach the add-on's download page. Check the internet connection, or install it from a file." and changes nothing.
5. **Cancel** part-way through *Get* (throttle the connection if the file is too quick). The row goes back as it was and nothing is left in `addons/`.
6. **An update.** Publish a newer version (say 0.1.1) the same way; *Check for updates* offers it, *Update* installs it, and an export that was running at the time makes it say the add-on is in use.
7. **Nothing at start-up.** Watch the network (a firewall log will do) while starting the app and opening a talk: no connection to GitHub until you click.
8. On Windows, the same steps, and that *Install from file* waits during an export (F1's lock, now shared with Update).
9. **A damaged install.** Change a byte of `addons/slidev/addon.wasm`: the dialog says it is damaged and offers *Get again*, which installs the published version in its place.

Two limits to know before trying these on a corporate network, both by plan:
- **Only the environment's proxy is used.** The app reads `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY`. A Windows machine whose proxy is set only in the system settings (WinINET or a PAC file) gets "Couldn't reach the add-on's download page…".
- **Only the bundled web roots are trusted.** A network that intercepts TLS with its own certificate authority gets "The add-on's download page couldn't be reached securely…", since the system's certificate store isn't read.

Both sentences point to *Install from file*, which works on either.

## The final review

A whole-branch review of `f8b19c8..1eff7dc`, on the most capable model, found the chain sound: nothing unverified is installed or run, nothing reaches the network without a click, the notices are right (webpki-roots is CDLA-Permissive-2.0, with its text), and the guide agrees with the app. It still asked for fixes: 2 important findings and 6 minor ones.
- **The empty repository.** The guide said to create the releases repository with no code, and GitHub can't make a release in an empty repository. It now creates it with `--add-readme`.
- **One "latest" index.** A single index would have cut off every shipped app once an add-on for a later Slidev was published. Each line of apps now has an index file of its own, at an address that never moves, in the release tagged `index`.

The fix wave was `aefd7e9`, `f10a778`, `1819f11` and `966254e`. Besides those two findings, it fixed:
- the minor findings;
- the items parked from the task reviews, among them the resolver `GlobalOnly`, which keeps only globally routable addresses after the name is looked up (the security review's point);
- the kept-download, version and `is_busy` points.

After the wave, the app suite gave 1542 passed and 0 failed, and clippy was clean.

The re-review, on the same model, confirmed that ureq resolves only a proxy's own address, that the `index` release's redirect passes the checks, and that the guide works as written. It found one more important gap: the index file was named by Slidev's major version while the app accepts only the exact version. The controller fixed it in `60b673f`, which also added the special-purpose ranges the re-review listed (2001::/23, 5f00::/16, 192.88.99/24) to the refused list. The file is now `index-abi1-slidev53.0.0.json`. `package addon-release` wrote it again for the signed 0.1.0 package.

Not checked here: anything against the real GitHub (hand checks 1 and 2), and Windows.
