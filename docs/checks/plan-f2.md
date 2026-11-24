# Plan F2: getting the Slidev add-on

The run was on the `plan-f` branch, on a Linux laptop, on 2026-11-24. The branch hasn't been pushed or merged. **Nothing here has run on Windows, and nothing has run against the real internet:** every test uses a small local server, and no repository or release exists yet. The owner publishes the first one (`docs/guide/publishing-the-addon.md`) and does the hand checks below.

## What was built

The app can get the add-on itself, from the releases of `Eiriksb/stagepack-studio-addons`, only when the owner clicks. Installing still goes through the F1 path: the signature is checked, then the same atomic install.

- **The fetch module** (task 1, `5ededc2`, `22a083a`). `fetch::check` reads `index.json` and `fetch::download` reads the package, over https only (ureq with rustls and the bundled web roots).
  - Caps: the index at most 64 KiB, the package at most 48 MiB, both counted while reading. Connecting has 10 s, the index 20 s in total, the package 180 s.
  - Redirects are followed by hand, at most 5, to `https://` addresses that aren't local or private.
  - Each request sends only `User-Agent` and `Accept`: no cookies, no query string.
  - The index must name `slidev`, three numbers, ABI 1 and a `url` under `PACKAGE_PREFIX`; `size` and `sha256` must match the download; and after the signature check, the signed manifest must agree with the index.
  - Every refusal is one sentence that echoes nothing the server wrote.
- **Get, Check for updates and Update** (task 2, `d19582b`, `9624ee0`). Buttons in *File ▸ Add-ons…*, with progress in MB and a *Cancel* that stops within a second.
  - A download runs on its own thread. Nothing is requested at start-up, on a timer or on opening a talk; a test pins that.
  - An update is offered only for a strictly newer version, and an install that would go back is refused even if the check lost a race.
  - Update says the add-on is in use while an export or import runs, and keeps the download for the next try.
  - A failed or cancelled download leaves nothing but a temporary file, which is removed.
- **The release files** (task 3). `package addon-release [--package FILE]` writes `target/package/index.json` beside the signed package: its `size`, `sha256` and `url` (`PACKAGE_PREFIX` + `slidev-vX.Y.Z/slidev-X.Y.Z.spaddon`) come from the package itself, and it reads the package with the app's built-in key first, so a package the app would refuse gets no index. It is written with the same `addon_abi::package::Index` type the app reads. The two addresses now live in `addon-abi` (`INDEX_URL`, `PACKAGE_PREFIX`) and `fetch.rs` re-exports them. It then prints the `gh release create` command. The steps are in `docs/guide/publishing-the-addon.md`.

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

Tasks 1 and 2 each went through one fix round and a re-review that approved it. The re-reviews left minor points for a final fix wave, not yet made:
- The redirect check refuses only literal local and private addresses; ranges such as 100.64/10, NAT64 and 6to4 pass, and a name that resolves to a private address isn't caught. Only the fixed GitHub addresses are fetched and the result must pass the signature, so the exposure is small.
- A busy Get keeps a download it never reuses, and a later check can leave a stale one in memory (up to 48 MiB) until the dialog closes.
- A version that isn't three numbers blocks updates (the packaging tool now refuses to make one).
- `is_busy` can create a `.lock` behind a linked `addons` folder.

## What was run on the real file

`package addon-release` was run on `target/package/slidev-0.1.0.spaddon`, signed with the owner's key earlier. No key was read; the index is made from the package alone. It wrote:

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

Nothing was published, and no repository or release was created.

## Hand checks for the owner

Do each on **Linux and Windows**.

1. **Publish 0.1.0** by `docs/guide/publishing-the-addon.md`: create the public repository once, build the package with your key, run `package addon-release`, run the `gh release create` command it prints, and check the release page holds the `.spaddon` and `index.json`, and is marked latest.
2. **Get.** On a clean user account (no add-on installed), *File ▸ Add-ons… ▸ Get*. Progress shows, then "Installed, version 0.1.0", and an export works.
3. **Check for updates.** It says this is the newest version.
4. **With the network off,** *Get* or *Check for updates* shows "Couldn't reach the add-on's download page. Check the internet connection, or install it from a file." and changes nothing.
5. **Cancel** part-way through *Get* (throttle the connection if the file is too quick). The row goes back as it was and nothing is left in `addons/`.
6. **An update.** Publish a newer version (say 0.1.1) the same way; *Check for updates* offers it, *Update* installs it, and an export that was running at the time makes it say the add-on is in use.
7. **Nothing at start-up.** Watch the network (a firewall log will do) while starting the app and opening a talk: no connection to GitHub until you click.
8. On Windows, the same steps, and that *Install from file* waits during an export (F1's lock, now shared with Update).

## The final review

*To be written by the controller.*
