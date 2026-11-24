# Publishing the Slidev add-on

These are the owner's steps. Nobody else runs them, and nothing in the repository creates a repository or a release for you. The app's *Get* and *Check for updates* read an index file from the public repository `Eiriksb/stagepack-studio-addons`.

## How it is laid out

- **Each version of the add-on has a release of its own,** tagged `slidev-vX.Y.Z`, holding only its `.spaddon` package.
- **One release, tagged `index`, holds the index files,** one for each line of apps: `index-abi<abi>-slidev<version>.json`, for the apps that take add-on interface `<abi>` and write for exactly Slidev `<version>`. Today there is one line, `index-abi1-slidev53.0.0.json`.
- **An app asks only for its own line's file,** at an address that never moves: `https://github.com/Eiriksb/stagepack-studio-addons/releases/download/index/index-abi1-slidev53.0.0.json`. Publishing for another Slidev version (even a minor one) or a new add-on interface adds a new file beside it, so apps already shipped keep finding the last add-on made for them.
- **Which release GitHub marks *latest* doesn't matter.** Nothing reads it, so the package's release isn't made with `--latest` (it was only needed while the app read the latest release's file).

## Once

1. **Create the public repository** with a first commit, a README, for the release tags to point at. GitHub can't make a release in an empty repository.

   ```
   gh repo create Eiriksb/stagepack-studio-addons --public --add-readme
   ```

   On the web, tick *Add a README file* instead. It needs that one commit and no code.
2. **Create the release that holds the indexes,** with no files yet. `package addon-release` prints this command too, each time; it is needed before the first release only.

   ```
   gh release create index --repo Eiriksb/stagepack-studio-addons --title "Add-on index" --notes "The index files the app reads, one for each line of apps. Never delete them."
   ```

## For each version

1. **Set the version.** It is `version` in `crates/slidev-addon/Cargo.toml`, three numbers with no leading zeros (say `0.1.0`). Commit it.
2. **Build and sign the package** with your signing key, which stays outside the repository:

   ```
   scripts/cargo-safe.sh run -p xtask --bin package -- addon --key ~/stagepack-studio-signing/addon-signing.key
   ```

   This writes `target/package/slidev-X.Y.Z.spaddon`. It warns if the key isn't the one built into the app, since the app would refuse that package.
3. **Write the index** for it:

   ```
   scripts/cargo-safe.sh run -p xtask --bin package -- addon-release
   ```

   This reads the package beside it (with the app's built-in key, so a package the app would refuse gets no index), writes `target/package/index-abi1-slidev53.0.0.json` (named for the package's own line) with its size, sha256 and download address, and prints the commands of the next two steps. `--package FILE` picks another package.
4. **Publish the package** in a release of its own, tagged `slidev-vX.Y.Z`:

   ```
   gh release create slidev-vX.Y.Z target/package/slidev-X.Y.Z.spaddon --repo Eiriksb/stagepack-studio-addons --title "Slidev add-on X.Y.Z" --notes "Slidev add-on X.Y.Z"
   ```

5. **Then put its index in place** of the line's earlier one, in the release tagged `index`. Do this only once the package is up, or *Get* would be sent to a file that isn't there yet.

   ```
   gh release upload index target/package/index-abi1-slidev53.0.0.json --clobber --repo Eiriksb/stagepack-studio-addons
   ```

   For the moment the file is being replaced, *Get* may answer 404; trying again a little later works.
6. **Check it in the app.** On a scratch user account (or after removing the add-on), open *File ▸ Add-ons…* and use *Get*; then *Check for updates* should say it is the newest.

## Rules

- **Never delete the release `index` or a file in it.** Each file is what every app of its line reads, for as long as those apps are used. A file deleted leaves that line unable to get or update the add-on; it can still install from a file.
- **Never delete a package's release** that an index names, for the same reason.
- **A new line gets a new file.** When the app moves to another Slidev version (a minor one too) or a new add-on interface, its package's index is written under the new name, and uploaded beside the older ones. The older lines' files stay as they are, naming the last package made for them.
- **Upload only the package and the index.** The signing key must never be uploaded, attached, pasted into an issue or committed. Anyone who holds it can sign an add-on every installed app trusts.
- **The index proves nothing.** The app checks the download against it, then against the package's signature, then against the signed `addon.json` inside. Still, a wrong index only makes *Get* fail; fix it by publishing a new version (a package release's file is not to be swapped under people who have it).
- **Versions only go up.** The app offers *Update* only for a strictly newer version, and never goes back.
