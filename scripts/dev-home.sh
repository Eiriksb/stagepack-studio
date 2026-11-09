#!/usr/bin/env bash
# Builds target/dev-home: Stagepack's Linux home plus the Studio addon, and packs the fixture talk.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/dev.env"
HOME_DIR="$ROOT/target/dev-home"
NM="$HOME_DIR/runtimes/53/node_modules"
NODE="$HOME_DIR/node/bin/node"

mkdir -p "$ROOT/target/fixtures" "$HOME_DIR"
rsync -a --delete "$STAGEPACK_SRC/dist/linux/" "$HOME_DIR/"

# Pack Studio and copy it in (no npm install: it would prune slidev-addon-stagepack).
PACK_DIR="$ROOT/target/studio-pack"
rm -rf "$PACK_DIR"; mkdir -p "$PACK_DIR"
(cd "$STUDIO_SRC" && pnpm pack --pack-destination "$PACK_DIR" >/dev/null)
tar -xzf "$PACK_DIR"/*.tgz -C "$PACK_DIR"
rm -rf "$NM/slidev-addon-studio"
cp -r "$PACK_DIR/package" "$NM/slidev-addon-studio"

# Check Studio's dependency ranges against the runtime, with the dev home's Node and the runtime's semver.
"$NODE" -e '
const path = require("path");
const nm = process.argv[1];
const semver = require(path.join(nm, "semver"));
const deps = require(path.join(nm, "slidev-addon-studio/package.json")).dependencies;
let bad = 0;
for (const name of ["@vueuse/core", "yaml"]) {
  const v = require(path.join(nm, name, "package.json")).version;
  if (!semver.satisfies(v, deps[name])) { console.error(`${name} ${v} does not satisfy ${deps[name]}`); bad = 1; }
}
process.exit(bad);
' "$NM" || { echo "dev home: runtime dependency range check failed" >&2; exit 1; }

"$HOME_DIR/stagepack" pack "$ROOT/fixtures/hello" -o "$ROOT/target/fixtures/hello.sptx"
echo "dev home ready: $HOME_DIR"
