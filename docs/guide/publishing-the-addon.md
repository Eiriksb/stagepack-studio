# Publishing the Slidev add-on

These are the owner's steps. Nobody else runs them, and nothing in the repository creates a repository or a release for you. The app's *Get* and *Check for updates* read the newest release of the public repository `Eiriksb/stagepack-studio-addons`.

## Once

Create the public repository `Eiriksb/stagepack-studio-addons` on GitHub. It holds only releases; it needs no code.

## For each version

1. **Set the version.** It is `version` in `crates/slidev-addon/Cargo.toml`, three numbers (say `0.1.0`). Commit it.
2. **Build and sign the package** with your signing key, which stays outside the repository:

   ```
   scripts/cargo-safe.sh run -p xtask --bin package -- addon --key ~/stagepack-studio-signing/addon-signing.key
   ```

   This writes `target/package/slidev-X.Y.Z.spaddon`. It warns if the key isn't the one built into the app, since the app would refuse that package.
3. **Write the index** for it:

   ```
   scripts/cargo-safe.sh run -p xtask --bin package -- addon-release
   ```

   This reads the package beside it (with the app's built-in key, so a package the app would refuse gets no index), writes `target/package/index.json` with its size, sha256 and download address, and prints the command of the next step. `--package FILE` picks another package.
4. **Publish the release,** with the tag `slidev-vX.Y.Z` and both files:

   ```
   gh release create slidev-vX.Y.Z target/package/slidev-X.Y.Z.spaddon target/package/index.json --repo Eiriksb/stagepack-studio-addons --title "Slidev add-on X.Y.Z" --latest
   ```

5. **Check it in the app.** On a scratch user account (or after removing the add-on), open *File ▸ Add-ons…* and use *Get*; then *Check for updates* should say it is the newest.

## Rules

- **The newest release must always hold `index.json`.** *Get* reads `https://github.com/Eiriksb/stagepack-studio-addons/releases/latest/download/index.json`, which is the newest release's file. A release without it, marked `--latest`, leaves everyone unable to get or update the add-on. Never mark a release latest that lacks it.
- **Upload only the two files.** The signing key must never be uploaded, attached, pasted into an issue or committed. Anyone who holds it can sign an add-on every installed app trusts.
- **The index proves nothing.** The app checks the download against it, then against the package's signature, then against the signed `addon.json` inside. Still, a wrong index only makes *Get* fail; fix it by publishing a new version (the same tag's files are not to be swapped under people who have them).
- **Versions only go up.** The app offers *Update* only for a strictly newer version, and never goes back.
