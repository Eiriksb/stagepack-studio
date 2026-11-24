# Plan F2: decisions made while building

The owner chose a separate public repository for the add-on (2026-11-24). The other calls below are the controller's. `docs/checks/plan-f2.md` has what ran, what's left and the hand checks.

## For the owner

- **Publish the add-on** by following `docs/guide/publishing-the-addon.md`:
  - create `Eiriksb/stagepack-studio-addons` once, with a README;
  - make the one-time `index` release;
  - make the package's release;
  - upload the index.

  `package addon-release` prints the exact commands. Never upload the signing key.
- **Hand checks 1 and 2** are the first test against the real GitHub. They cover the `Accept: application/octet-stream` header and the redirect to GitHub's storage.
- **Windows hasn't been tried.** In particular, check that *Install from file* waits while an export runs.

## Rulings

- **Fetching.**
  - Library: ureq =3.5.2 over rustls 0.23 (ring) and webpki-roots, so there is one TLS stack.
  - Protocol: https only (ureq's `https_only` too), at most 5 redirects, each checked.
  - Addresses:
    - a package's address must sit under `PACKAGE_PREFIX`, compared on the parsed URL;
    - after the name is looked up, only globally routable addresses are kept;
    - through a proxy, that protection rests with the proxy.
  - Limits: the index is at most 64 KiB, the package is read up to the index's `size` (48 MiB at most), with 10 s to connect, 20 s for the index and 180 s for the package.
  - Errors are the app's own sentences, never the server's words.
- **Only a click reaches the network.** A test counts every request.
- **The index.** One file for each line of apps, `index-abi<abi>-slidev<exact version>.json`, in the release tagged `index`, at an address that never moves. Package files go in versioned releases, and `--latest` isn't used. The index is untrusted: the signed manifest has to agree with it.
- **Updates never go back.** The version check runs again under the install's exclusive lock. *Install from file* still takes any version.
- **Exports and imports hold a shared lock while they run.** *Get* and *Update* then say they're busy instead of waiting.
- **Downloads run on a thread of their own,** which the dialog polls every 100 ms, because gpui's test scheduler can't take wakes from other threads.

## Smaller findings and notes

- A download that stalls keeps its thread until the time limit passes (up to 180 s), holding at most 48 MiB in memory and nothing on disk.
- `package addon-release` prints the one-time `index` command every time, marked "skip it if there already".
- xtask's `SLIDEV_TARGET` and the app's `SLIDEV_VERSION` are separate constants.
- *Install from file* is hidden for a damaged install; *Get again* replaces it.
