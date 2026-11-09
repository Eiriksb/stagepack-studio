# Walking Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** on Linux, open a `.sptx` in a native gpui-kit window, select and move things on the slide, see and change the selection in a native inspector, and save the result back into the `.sptx`.

**Architecture:**
- **Stagepack gets a long-lived `serve --json` sidecar** that does three things:
  - unpacks the talk and asks the trust question over stdin and stdout;
  - starts Slidev with extra addons loaded through a Node `--import` preload;
  - saves the whole work folder back into the `.sptx` on command.
- **The Studio fork gets a JSON bridge** over `window.ipc` and hides its web panels when the bridge is present.
- **The Rust shell** puts it together: it runs the sidecar, shows Slidev in `gpui-webview`, and turns bridge messages into native UI.

**Tech Stack:**
- Rust 1.98, edition 2024: gpui-kit `=0.9.0`, gpui-webview `=0.9.0`, lb-wry `=0.53.3`, serde.
- Go: Stagepack's cached toolchain.
- TypeScript and Vue: Studio, pnpm, vitest.
- Node 24: Stagepack's bundled build.
- Slidev 53.0.0.

**Spec:** `docs/superpowers/specs/2026-11-09-native-slidev-editor-design.md`. The roadmap is `docs/superpowers/plans/2026-11-09-roadmap.md`.

**Research notes:** the internals this plan builds on, with file and line references, are in `docs/superpowers/research/`:
- `2026-11-09-stagepack.md`: the open flow, saving, trust, how addons resolve, and the runtime build;
- `2026-11-09-studio.md`: selection, edit functions, the mount seams and the inline editor;
- `2026-11-09-gpui.md`: the bootstrap, the web view, the components, headless tests and the system packages.

The task that touches each area points at its note.

## Repos and paths

| Name in this plan | Path | Branch |
| --- | --- | --- |
| **APP** | `/home/bjoeir/Documents/Projects/stagepack-studio` | `main` (worktree per superpowers:using-git-worktrees at execution) |
| **STUDIO** | `/home/bjoeir/Documents/Projects/slidev-addon-studio`: clone of `https://github.com/Eiriksb/slidev-addon-studio` | new branch `editor-bridge` from `main` (`a308b35`) |
| **SP** | `/home/bjoeir/Documents/Projects/stagepack/.claude/worktrees/editor-sidecar`: `git worktree add -b claude/editor-sidecar .claude/worktrees/editor-sidecar origin/main` in `/home/bjoeir/Documents/Projects/stagepack` | `claude/editor-sidecar` from `origin/main` (`5e00427`) |

**SP setup, once:**
- `ln -s /home/bjoeir/Documents/Projects/stagepack/.cache .cache`, so the Makefile's Go is found.
- `cp -a ../export-pptx/dist ./dist`. That is the built Linux home (Node, runtime, ffmpeg) the e2e tests use.

Nothing is pushed in this plan. All three repos stay local.

## Prerequisites (the owner does these; the executor stops and asks)

1. **System packages for GPUI and WebKitGTK,** installed by the owner in a terminal:
   ```bash
   sudo apt install -y gcc g++ clang libfontconfig-dev libwayland-dev libwebkit2gtk-4.1-dev libxkbcommon-x11-dev libx11-xcb-dev libssl-dev libzstd-dev libasound2-dev vulkan-validationlayers libvulkan1
   ```
2. **Downloads:** consent to fetch crates from crates.io (`cargo fetch`) and npm packages from the registry (`pnpm install` in STUDIO, `npm install` into the dev home).

## Global Constraints

- **Platform for this plan:** Linux x86_64 on X11 or XWayland. Start the app with `gpui_kit::platform::linux(WindowingModes::X11)`.
- **Crate pins, exactly:**
  - `gpui-kit = { version = "=0.9.0", features = ["gpui-fast"] }`
  - `gpui-webview = { version = "=0.9.0", features = ["gpui-fast"] }`
  - `wry = { version = "=0.53.3", package = "lb-wry" }`

  Commit `Cargo.lock`.
- **Studio is loaded only through `serve --addon`.** Nothing the editor adds is written into the work folder or the `.sptx`. That covers addons, preloads, generated configs and package.json edits.
- **The web view loads `<baseUrl>/<slide>`.** Never add `?embedded`, which disables Studio.
- **The bridge protocol is version 1.** Every message carries `"v":1`, and the shapes are exactly as in Task 2. A mismatch is an error the user sees, never a silent ignore.
- **Licences:** APP is MIT. STUDIO keeps its MIT notice. `package.json` `repository` and `homepage` in STUDIO point to `https://github.com/Eiriksb/slidev-addon-studio`.
- **Public repos:** don't name real decks, clients or organisations in code, commits or fixtures. Fixtures are made up.
- **Git:** never `git add -f`. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Long commands:** anything over about 2 minutes (a cargo build, Slidev e2e) runs in the background.

## Review Focus

1. **A path with spaces, `æøå` and `%`** in the `.sptx` name or folder must open, serve and save normally. The exporter once broke on `%`.
2. **A talk already open in plain Stagepack:** `serve` must say `busy` and exit 4, and the shell shows the busy message without starting a second copy.
3. **Slidev crashes while editing:** the shell shows the error with the log tail. It neither hangs nor claims the work was saved.
4. **The page reloads** (Slidev's full reload after a component or layout change): the bridge sends `ready` again, the shell clears the selection, and nothing is sent to a page that isn't ready.
5. **The `.sptx` changes on disk while it is open,** for example a copy synced over it: Save reports `conflict` and leaves the file untouched.

These are pinned by: Task 7 (`TestServeOddPath`, `TestServeBusy`, `TestServeSlidevDies`, `TestServeConflict`), Task 3 (`reload sends ready again`), and Task 12 (`busy_shows_message`, `slidev_death_fails_with_log`, `reload_clears_selection`, `conflict_keeps_dirty`).

---

### Task 1: Studio runs on Slidev 53 (STUDIO)

**Files:**
- Modify: `package.json` (devDependencies, engines, repository, homepage), `pnpm-lock.yaml`.

**Interfaces:**
- Produces: STUDIO on branch `editor-bridge`, testing green against `@slidev/cli` 53.0.0.

- [ ] **Step 1: Clone and branch.** `git clone https://github.com/Eiriksb/slidev-addon-studio /home/bjoeir/Documents/Projects/slidev-addon-studio && cd … && git switch -c editor-bridge`.
- [ ] **Step 2: Bump.**
  - Set `@slidev/cli`, `@slidev/client` and `@slidev/types` devDependencies to `^53.0.0`.
  - Set `engines.node` to `>=22.12.0`.
  - Set `repository.url` and `homepage` to the fork.
  - Run `pnpm install`.
- [ ] **Step 3: Run the suite.** Run `pnpm test && pnpm typecheck`. Expected: all tests pass (about 170 cases; `qa.test.ts` is skipped), and typecheck exits 0.
- [ ] **Step 4: Check by hand.**
  - Run `pnpm play`, press `E`, and click a code block.
  - Expected: the Element panel shows the fence's Markdown lines. Slidev 53 changed `CodeBlockWrapper.vue`, and this checks that the fence annotation still lands.
  - If it doesn't, fix `node/markdown-source.ts` with a test in `test/md.test.ts` before continuing.
- [ ] **Step 5: Commit.** `git commit -am "Build and test against Slidev 53"`.

### Task 2: Bridge protocol v1, types and fixtures (STUDIO)

**Files:**
- Create: `client/bridge/protocol.ts`, `test/fixtures/bridge-v1/*.json`, `test/bridge-protocol.test.ts`.

**Interfaces:**
- Consumes: `StudioTarget`, `DragPos`, `TargetKind` (`client/types.ts`).
- Produces, used by Task 3 and the Rust side (Task 8):
  ```ts
  export const BRIDGE_VERSION = 1
  export type PageMessage =
    | { v: 1, type: 'ready', studio: string, slide: number, total: number }
    | { v: 1, type: 'selection', selection: SelectionInfo | null }
    | { v: 1, type: 'history', canUndo: boolean, canRedo: boolean, lastAction: string | null }
    | { v: 1, type: 'edited', slide: number, label: string }
    | { v: 1, type: 'slide', slide: number, total: number }
    | { v: 1, type: 'error', message: string }
    | { v: 1, type: 'result', id: number, ok: boolean, error?: string }
  export interface SelectionInfo {
    slide: number, range: [number, number] | null, kind: TargetKind, tag: string | null,
    label: string, positioned: boolean, nested: boolean, pos: DragPos | null, canReorder: boolean
  }
  export type CommandBody =
    | { cmd: 'set-pos', pos: DragPos } | { cmd: 'free-position' } | { cmd: 'return-to-flow' }
    | { cmd: 'delete' } | { cmd: 'duplicate' } | { cmd: 'undo' } | { cmd: 'redo' }
    | { cmd: 'deselect' } | { cmd: 'go', slide: number }
  export type Command = { v: 1, id: number } & CommandBody
  export function selectionInfo(target: StudioTarget, content: string, canReorder: boolean): SelectionInfo
  export function parseCommand(json: string): Command          // throws BridgeError
  export class BridgeError extends Error {}
  ```
  The exact error texts, used in `result.error`:
  - `"Unsupported bridge version: <v>"`
  - `"Unknown command: <cmd>"`
  - `"Select an element first."`
  - `"This element is not freely positioned."`
  - `"Bad command: <reason>"`

- [ ] **Step 1: Write the fixtures.** One file per message and command, named after its type: `ready.json`, `selection.json` (a positioned component), `selection-none.json`, `history.json`, `edited.json`, `slide.json`, `error.json`, `result-ok.json`, `result-error.json`, and `cmd-set-pos.json`, `cmd-free-position.json`, `cmd-return-to-flow.json`, `cmd-delete.json`, `cmd-duplicate.json`, `cmd-undo.json`, `cmd-redo.json`, `cmd-deselect.json`, `cmd-go.json`. For example, `selection.json` is:
  ```json
  {"v":1,"type":"selection","selection":{"slide":2,"range":[3,5],"kind":"component","tag":"Shape","label":"<Shape>","positioned":true,"nested":false,"pos":{"x":120,"y":80,"w":200,"h":null,"rotate":0},"canReorder":true}}
  ```
  and `cmd-set-pos.json` is `{"v":1,"id":7,"cmd":"set-pos","pos":{"x":140,"y":80,"w":200,"h":null,"rotate":0}}`.
- [ ] **Step 2: Write the failing tests** in `test/bridge-protocol.test.ts`:
  - `selectionInfo matches the selection fixture`: build a target for `<Shape v-drag pos="120,80,200,_" />` on slide 2 at lines `[3,5]`. Expect `{ v: 1, type: 'selection', selection: selectionInfo(...) }` to deep-equal `selection.json`.
  - `parseCommand accepts every cmd fixture`: each `cmd-*.json` round-trips to a deep-equal object.
  - `parseCommand rejects v 2` with `"Unsupported bridge version: 2"`.
  - `parseCommand rejects an unknown cmd` with `"Unknown command: explode"`.
  - `parseCommand rejects set-pos without pos` with a message that starts with `"Bad command:"`.
- [ ] **Step 3: Run them to see them fail.** `pnpm vitest run test/bridge-protocol.test.ts`. Expected: FAIL, because `client/bridge/protocol.ts` does not exist.
- [ ] **Step 4: Implement `client/bridge/protocol.ts`.** `selectionInfo` reads `pos` with `readDrag(content, range)?.pos ?? null`. It only parses and serialises, with no DOM or Vue.
- [ ] **Step 5: Run the file again.** Expected: PASS. Then `pnpm test && pnpm typecheck`, green.
- [ ] **Step 6: Commit.** `git add client/bridge test/bridge-protocol.test.ts test/fixtures/bridge-v1 && git commit -m "Bridge protocol v1: message types and fixtures"`.

### Task 3: The bridge in the page (STUDIO)

**Files:**
- Create: `client/bridge/index.ts`, `test/bridge-dispatch.test.ts`.
- Modify:
  - `client/actions.ts`: add `setPosition`, moved from `PanelInspect.vue` `setPos`.
  - `client/ui/panels/PanelInspect.vue`: use `setPosition`.
  - `client/composables/useSlideSource.ts`: add `onSlideWritten`.
  - `client/ui/StudioRoot.vue`: lines about 80, 145-160, 174 and 178.
  - `setup/shortcuts.ts`: `studio_toggle` becomes a no-op in bridge mode.

**Interfaces:**
- Consumes: Task 2's types; `StudioContext` (`client/context.ts`); `useSelection()` (`select`, `reselect`); `useStudioHistory()`; actions `freePosition`, `returnToFlow`, `deleteBlock`, `duplicateBlock`.
- Produces:
  ```ts
  // client/bridge/index.ts
  export function bridgePresent(): boolean          // typeof window.ipc?.postMessage === 'function'
  export function useStudioBridge(studio: StudioContext): void
  // installs window.__studioBridge = { dispatch(json: string): void }
  export function createDispatcher(studio: StudioContext, post: (m: PageMessage) => void): (json: string) => Promise<void>
  // client/actions.ts
  export async function setPosition(studio: StudioContext, target: StudioTarget, pos: DragPos): Promise<boolean>
  // client/composables/useSlideSource.ts
  export function onSlideWritten(listener: (slide: number, label: string) => void): () => void
  ```
  `onSlideWritten` fires after every successful content or frontmatter write, but not after a note write.

**Behaviour:**
- **`useStudioBridge`** is called in StudioRoot's setup right after `studioContext.value = context`, and only when `bridgePresent()` is true. It does the following:
  - sets `studioOpen.value = true`;
  - posts `ready` with `studio` set to the package version, `slide` set to `studio.no()`, and `total` taken from Slidev's nav;
  - watches the selection (posts `selection`), the history (posts `history`), the slide number (posts `slide`) and `lastError` (posts `error` when it is non-null);
  - subscribes `onSlideWritten`, posting `edited`.
- **Commands run one at a time,** in arrival order, from a promise queue. Each command posts exactly one `result` with its `id`.
- **`set-pos`:** with no selection it fails with `"Select an element first."`, and on an element that isn't positioned with `"This element is not freely positioned."`.
- **`go`:** calls `studio.go(slide)`.
- **`deselect`:** calls `clearSelection()`.
- **In bridge mode,** `StudioToolbar` and `StudioDock` are not rendered, the deck-scale effect uses a dock width of 0, and the `slidev-studio-docked` class stays.
- **Without a bridge, nothing changes.**

- [ ] **Step 1: Write the failing tests** in `test/bridge-dispatch.test.ts`. Use a fake `StudioContext`, whose `commit` records calls and resolves after a tick, and a `post` that collects messages.
  - `set-pos on a positioned selection commits writeDrag and posts ok`: expect commit label `'Set position'` and `result {id:7, ok:true}`.
  - `set-pos with no selection posts the error`: `{ok:false, error:'Select an element first.'}`, and no commit.
  - `set-pos on an element in the flow posts the error`: `'This element is not freely positioned.'`.
  - `commands run one at a time`: dispatch two `set-pos` with no await between them. The second commit starts only after the first resolves, and the results arrive in id order.
  - `unknown command posts a result error`: `'Unknown command: explode'`. Its id is read from the JSON if present, else `-1`.
  - `reload sends ready again`: calling `useStudioBridge` twice, as on two page loads, posts `ready` twice and leaves one `window.__studioBridge`.
- [ ] **Step 2: Run them to see them fail.** `pnpm vitest run test/bridge-dispatch.test.ts`. Expected: FAIL (the module is missing).
- [ ] **Step 3: Implement** `setPosition`, then `onSlideWritten`, then `client/bridge/index.ts`, then the StudioRoot and shortcuts changes. PanelInspect calls `setPosition(studio, selection, { ...pos, [field]: value })`.
- [ ] **Step 4: Run the file again.** Expected: PASS. Then `pnpm test && pnpm typecheck`, green.
- [ ] **Step 5: Check by hand without a bridge.** Run `pnpm play` and press `E`. The toolbar, dock and Element panel work as before, including the position fields.
- [ ] **Step 6: Commit.** `git commit -am "Bridge: publish selection and history over window.ipc, run commands one at a time, hide the web panels when the bridge is present"`.

### Task 4: Bridge smoke test in a real browser (STUDIO)

**Files:**
- Create: `test/smoke/bridge.smoke.mjs`.
- Modify: `package.json` (script `smoke:bridge`, devDependency `playwright-core: 1.64.1`).

**Interfaces:**
- Consumes: Task 3's page behaviour.
- Produces: `pnpm smoke:bridge`, which later tasks run after any bridge change.

**What it does:**
1. Copies `playground/` to `test/smoke/.tmp/<random>/`, which is gitignored and removed afterwards.
   - The copy's headmatter `addons: [.]` is rewritten to the absolute repo root, because `.` resolves next to the talk folder, not inside it.
   - The copy stays inside the repo so parent lookup still finds the repo's `node_modules`.
2. Starts `slidev <tmp>/slides.md --port <free> --open false`.
3. Launches Chrome through `playwright-core`, using the executable `STUDIO_SMOKE_BROWSER` or the first of `/usr/bin/google-chrome`, `/usr/bin/chromium`, `/usr/bin/microsoft-edge`.
4. Uses `addInitScript` to define `window.ipc = { postMessage: m => window.__sent.push(JSON.parse(m)) }`.
5. Opens `<base>/<n>`, where `<n>` is the playground slide with the `Shape`.

- [ ] **Step 1: Write the script.** It prints one line per check and `bridge smoke: N checks passed`. It exits 1 at the first failure, printing the messages received so far. The checks:
  - `ready` arrives within 30 s;
  - the toolbar `.studio-toolbar` is absent;
  - clicking the Shape posts `selection` with `tag: 'Shape'`;
  - `dispatch(cmd-set-pos)` posts `result ok` and then `edited`;
  - the temp `slides.md` now contains the new `pos`;
  - `dispatch(cmd-undo)` posts `history` with `canRedo: true`.
- [ ] **Step 2: Run it.** `pnpm smoke:bridge`. Expected: `bridge smoke: 6 checks passed`. If a check fails, fix the page code (Task 3's files) with a unit test where one fits, then run it again.
- [ ] **Step 3: Commit.** `git add test/smoke package.json pnpm-lock.yaml && git commit -m "Smoke test the bridge in a real browser"`.

### Task 5: Extra addons through a Node preload (SP)

**Files:**
- Create: `internal/slidev/addons.go`, `internal/slidev/addons-preload.mjs` (embedded), `internal/slidev/addons_test.go`, `testdata/addons/probe/{package.json,setup/vite-plugins.ts}`.
- Modify: `internal/slidev/run.go` (`Config`, `Args`, `childEnv`).

**Interfaces:**
- Produces:
  ```go
  // run.go
  type Config struct { Node, CLI, WorkDir, Entry, LogPath string; ReadyTimeout time.Duration; NodeArgs, Env []string }
  func Args(c Config, port int) []string   // NodeArgs, then CLI, "./"+Entry, "--port", N, "--open", "false", "--log", "warn"
  // addons.go
  const ExtraAddonsEnv = "STAGEPACK_EXTRA_ADDONS"
  // AddonLaunch writes the embedded preload to <home>/lib/addons-preload-<sha256[:12]>.mjs (once) and returns
  // NodeArgs {"--import", <file URL of it>} and Env {ExtraAddonsEnv + "=" + JSON array of absolute, forward-slash paths}.
  func AddonLaunch(home string, addons []string) (nodeArgs, env []string, err error)
  ```

**The preload:**
- It imports each `resolver-*.mjs` chunk in `<dirname(process.argv[1])>/../dist/`, the runtime the CLI path points at. It finds the exported function whose `.name` is `getRoots`; the export itself is minified.
- It calls that function with `slash(resolve(process.argv[2]))`, the entry, which is argv[2] once `--import` has been consumed.
- It appends the paths from `STAGEPACK_EXTRA_ADDONS` to the cached `userPkgJson.slidev.addons`. It replaces that object's copy and never writes to disk.
- If `getRoots` isn't found, it throws `stagepack: this Slidev runtime has no getRoots; extra addons can't be loaded`.

**Trust:** addons passed this way are the editor's, so they never count as the talk's code.

- [ ] **Step 1: Write the failing unit tests:**
  - `TestArgsNodeArgsFirst`: with `NodeArgs{"--import","file:///x.mjs"}`, Args starts `["--import","file:///x.mjs", CLI, "./slides.md", "--port", "4000", ...]`.
  - `TestChildEnvAddsEnv`: `Env{"A=1"}` appears in the child's environment, after the existing rewrites.
  - `TestAddonLaunchPaths`: `AddonLaunch(tmp, []string{"/a b/x"})` gives env `STAGEPACK_EXTRA_ADDONS=["/a b/x"]` and a file URL that exists. Calling it twice writes the file once.
- [ ] **Step 2: Write the failing runtime test** `TestAddonPreloadLoadsProbe`. It skips when `dist/linux` is missing, using the pattern of `test/e2e_test.go` `linuxBuild`.
  - It starts Slidev on `testdata/deck` (copied to a temp folder) with `AddonLaunch(home, {abs testdata/addons/probe})`.
  - Expected: `GET <base>/__probe-addon` returns 200 with body `ok`, and the temp deck's files are byte-identical before and after.
  - The probe addon's `setup/vite-plugins.ts` adds a middleware answering `/__probe-addon` with `ok`.
- [ ] **Step 3: Run them to see them fail.** `.cache/go/bin/go test ./internal/slidev/ -run 'Args|ChildEnv|AddonLaunch|AddonPreload' -v`. Expected: FAIL (they don't compile).
- [ ] **Step 4: Implement** `Config` fields, `Args`, `childEnv`, `addons.go` and the preload.
- [ ] **Step 5: Run them again.** Expected: PASS, including the runtime test. Then `make test` and `GOOS=windows .cache/go/bin/go vet ./...`, both clean.
- [ ] **Step 6: Commit.** `git commit -m "slidev: load extra addons through a Node --import preload, without touching the talk"`.

### Task 6: Saving the whole work folder (SP)

**Files:**
- Create: `internal/workspace/savefolder.go`, `internal/workspace/savefolder_test.go`, `internal/atomicfile/atomicfile.go` (+ test).
- Modify:
  - `internal/save/writeback.go`: use `atomicfile`, and export `WithUpdated`.
  - `internal/workspace/extract.go`: keep per-entry sums.
  - `internal/pack/pack.go`: move `collectProject`'s walk rules to `internal/deck/walk.go` as `ProjectFiles`.

**Interfaces:**
- Produces:
  ```go
  // internal/atomicfile
  func Replace(path string, write func(w io.Writer) error) error   // temp ".<base>.<random>.tmp" beside it, keep mode, Sync, rename (retry 5x250ms on Windows), sync dir; temp removed on any error
  // internal/deck
  func ProjectFiles(root string) ([]string, error)                 // slash paths; skips node_modules/ and .git anywhere, top-level dist/ and .slidev/
  // internal/workspace
  type Work struct { /* existing fields */ ; Sums map[string]string }   // entry name -> sha256, filled by Extract
  type ConflictError struct{ Want, Got time.Time }                       // deck's manifest Updated changed since open
  type BadNamesError struct{ Names []string }                            // names deck.CheckEntryNames refuses
  func SaveFolder(w *Work, deckPath string, now time.Time) error
  ```

**SaveFolder:**
- **The walk:** a work folder file `node_modules/x/...` is saved as `vendor/node_modules/x/...` only when that entry was in `w.Entries`. New packages are never added.
- **Unchanged entries,** by sum, are `zw.Copy`'d raw from the old zip. Changed and new ones use `deck.EntryMethod`. Deleted files drop out.
- **The manifest** is written with `WithUpdated(now)`.
- **On success,** `w.Entries`, `w.Sums` and `w.Manifest.Updated` are updated. Recomputing code and hash is the caller's job (Task 7).

- [ ] **Step 1: Write the failing tests** in `savefolder_test.go`. Each extracts a fixture `.sptx` packed from `testdata/deck` into a temp folder.
  - `TestSaveFolderUnchanged`: no edits. Every non-manifest entry is byte-identical, and the manifest's Updated is `now`.
  - `TestSaveFolderChangedAndNew`: slides.md is edited and `public/new image æ.png` added. Both appear with the new bytes, and the others are unchanged.
  - `TestSaveFolderDeleted`: a public file is removed, so its entry is gone.
  - `TestSaveFolderSkips`: `.slidev/snapshots.json`, `dist/x`, `node_modules/.slidev/y` and `.git/HEAD` are not in the zip.
  - `TestSaveFolderVendoredMapsBack`: a deck with `vendor/node_modules/pkg` keeps it under `vendor/`, and an unknown `node_modules/other` is not added.
  - `TestSaveFolderConflict`: another `RewriteEntry` changes the deck between Extract and Save. The result is `*ConflictError`, and the deck's bytes are unchanged.
  - `TestSaveFolderBadName`: with `public/a:b.png` present, the result is `*BadNamesError{Names: ["public/a:b.png"]}`, the deck is unchanged, and no temp file is left.
  - `atomicfile`: `TestReplaceRemovesTempOnError`.
- [ ] **Step 2: Run them to see them fail.** `.cache/go/bin/go test ./internal/workspace/ ./internal/atomicfile/ -v`. Expected: FAIL (they don't compile).
- [ ] **Step 3: Implement** `atomicfile`, `ProjectFiles` (with pack using it), `Sums` in Extract, `WithUpdated` exported, then `SaveFolder`.
- [ ] **Step 4: Run them again.** Expected: PASS. Then `make test` (pack's and save's existing tests must stay green) and `GOOS=windows .cache/go/bin/go vet ./...`.
- [ ] **Step 5: Commit.** `git commit -m "workspace: save a whole work folder back into its .sptx"`.

### Task 7: `stagepack serve --json` (SP)

**Files:**
- Create: `internal/serve/serve.go`, `internal/serve/serve_test.go`, `cmd/stagepack/serve.go`, `test/serve_e2e_test.go`.
- Modify:
  - `cmd/stagepack/main.go`: add `case "serve"`, and the usage string.
  - `internal/app/app.go`: move `inUse` into `internal/instance` as `func InUse(home string, now time.Time) func(dir string) bool`, so serve shares it.
  - `internal/app` CodeCheck: split into `Assess(ctx, w, deckPath) (reasons []string, trusted bool)` and `Allow`.

**Interfaces:**
- Consumes: Task 5's `AddonLaunch` and `Config.NodeArgs`/`Env`; Task 6's `SaveFolder`, `ConflictError` and `BadNamesError`; plus `instance.Claim`/`Update`/`Release`, `workspace.Extract`/`CleanStale`/`Remove`, `trust.Store`, `slidev.FreePort`/`Start`/`LogTail`.
- Produces:
  - **The CLI:** `stagepack serve --json [--home DIR] [--trust-code] [--addon DIR]... FILE`.
  - **Events, one JSON object per line on stdout,** in this order:
    ```
    {"event":"unpacked","id":"…","title":"…","entry":"slides.md","workDir":"/abs","code":["…"],"trusted":false}
    {"event":"confirm","question":"…"}                                  (only if code and not trusted)
    {"event":"ready","port":41234,"baseUrl":"http://localhost:41234","workDir":"/abs"}
    {"event":"saved","updated":"2026-11-09T12:00:00Z"}                  (after each save)
    {"event":"conflict","message":"This file was changed by another program since it was opened, so it was not overwritten."}
    {"event":"save-failed","message":"…","names":["public/a:b.png"]}
    {"event":"busy","baseUrl":"http://localhost:…"}                     (already open in another Stagepack; then exit 4)
    {"event":"error","message":"…","log":"<last 20 lines>"}
    {"event":"stopped"}
    ```
  - **Commands, one JSON object per line on stdin:** `{"answer":"yes"}`, `{"answer":"no"}`, `{"cmd":"save"}`, `{"cmd":"stop"}`. EOF on stdin means stop.
  - **Exit codes:** 0 stopped, 1 error (Slidev died or start failed), 2 usage, 3 not trusted, 4 busy.
  - **The Go side:**
    ```go
    // internal/serve
    type Deps struct { Start func(ctx context.Context, c slidev.Config, port int) (Server, error); Now func() time.Time }
    type Server interface { Port() int; BaseURL() string; Done() <-chan error; Stop() error }
    func Run(ctx context.Context, o Options, in io.Reader, out io.Writer, d Deps) int
    type Options struct { Home, DeckPath string; TrustCode bool; Addons []string }
    ```

**Behaviour:**
- **Startup:** CleanStale, Claim, Extract, Assess, ask, then Start with `AddonLaunch(home, o.Addons)`. It starts Slidev at most 3 times on a strictPort race.
- **Save:** SaveFolder. Then it recomputes code and hash with `deck.CodeReasons`, `ReadHeadmatter`, `HeadmatterReasons` and `AddCode`. If the talk was trusted or allowed this session, it calls `Remember(id, newHash, title)`.
- **No watcher, no autosave and no journal in this plan.** Crash recovery is plan 2.
- **Stop:** `srv.Stop()`, `workspace.Remove`, `Release`, then the `stopped` event.
- **Not trusted:** the work folder is removed before exit 3.

- [ ] **Step 1: Write the failing tests** in `internal/serve/serve_test.go`, with a fake `Deps.Start`:
  - `TestServeNoCode`: event order `unpacked{trusted:true, code:[]}`, then `ready{port}`. EOF gives `stopped`, exit 0, and the work folder is gone.
  - `TestServeCodeAskedYes`: `unpacked{trusted:false}`, then `confirm`. After `{"answer":"yes"}`, `ready`, and `trusted.json` holds the hash.
  - `TestServeCodeAskedNo`: `{"answer":"no"}` gives exit 3, no `ready`, and the work folder removed.
  - `TestServeSave`: `{"cmd":"save"}` after editing `slides.md` in the work folder gives `saved`, and unzipping the deck shows the edit.
  - `TestServeSaveKeepsTrust`: a trusted code deck gains a new `public/` file. After save, `Trusted(id, newHash)` is true.
  - `TestServeConflict`: the deck is rewritten underneath, so save gives `conflict`, and the file is unchanged.
  - `TestServeBusy`: a live instance record for the deck gives `busy{baseUrl}` and exit 4.
  - `TestServeSlidevDies`: the fake server's `Done` yields an error, giving `error{log}` then `stopped`, and exit 1.
  - `TestServeOddPath`: a deck named `my talk æøå 100%.sptx` in a folder `a b%c` runs the same flow as `TestServeNoCode`.
  - `TestServeRetriesPortRace`: the fake Start fails twice with `slidev exited before it was ready`, then succeeds. The result is `ready`, and Start was called 3 times.
- [ ] **Step 2: Write the e2e test** `test/serve_e2e_test.go` `TestServeE2E`. It skips without `dist/linux`, like `linuxBuild`.
  - It runs the built `stagepack serve --json --home dist/linux --addon <abs testdata/addons/probe>` on a packed `testdata/deck`.
  - It waits for `ready`. Then `GET baseUrl/` returns 200 and `GET baseUrl/__probe-addon` returns `ok`.
  - It edits slides.md in `workDir`, sends `{"cmd":"save"}` and gets `saved`. It closes stdin and gets `stopped` and exit 0.
  - The `.sptx` holds the edit and no `probe` or `addons-preload` file.
- [ ] **Step 3: Run them to see them fail.** `.cache/go/bin/go test ./internal/serve/ -v`. Expected: FAIL (they don't compile).
- [ ] **Step 4: Implement** `instance.InUse`, the CodeCheck split, `internal/serve`, and `cmd/stagepack/serve.go`, whose flag parsing follows `runExportPptx`.
- [ ] **Step 5: Run them again,** plus `make build` (so `dist/linux/stagepack` is the new binary), then `.cache/go/bin/go test ./test/ -run ServeE2E -v` in the background. Expected: PASS. Then `make test` and `GOOS=windows .cache/go/bin/go vet ./...`, clean.
- [ ] **Step 6: Commit.** `git commit -m "Add stagepack serve --json: a long-lived sidecar for editors"`.

### Task 8: APP workspace and the protocol crate (APP)

**Files:**
- Create:
  - `Cargo.toml` (workspace: `crates/protocol`, `crates/sidecar`, `crates/app`);
  - `crates/protocol/{Cargo.toml,src/lib.rs}`, `crates/protocol/tests/fixtures.rs`;
  - `LICENSE` (MIT, "Copyright (c) 2026 Eirik B."), `README.md` (a short status note);
  - `dev.env` (`STUDIO_SRC=/home/bjoeir/Documents/Projects/slidev-addon-studio`, `STAGEPACK_SRC=/home/bjoeir/Documents/Projects/stagepack/.claude/worktrees/editor-sidecar`).

**Interfaces:**
- Consumes: STUDIO's `test/fixtures/bridge-v1/*.json` (Task 2).
- Produces, crate `studio-protocol` (serde and serde_json only, no GUI):
  ```rust
  pub const BRIDGE_VERSION: u32 = 1;
  #[derive(Debug, Clone, PartialEq, Deserialize, Serialize)] #[serde(tag = "type", rename_all = "kebab-case")]
  pub enum PageMessage { Ready { studio: String, slide: u32, total: u32 }, Selection { selection: Option<SelectionInfo> },
      History { #[serde(rename="canUndo")] can_undo: bool, #[serde(rename="canRedo")] can_redo: bool, #[serde(rename="lastAction")] last_action: Option<String> },
      Edited { slide: u32, label: String }, Slide { slide: u32, total: u32 }, Error { message: String },
      Result { id: i64, ok: bool, #[serde(default)] error: Option<String> } }
  pub struct SelectionInfo { pub slide: u32, pub range: Option<[u32; 2]>, pub kind: String, pub tag: Option<String>, pub label: String,
      pub positioned: bool, pub nested: bool, pub pos: Option<DragPos>, #[serde(rename="canReorder")] pub can_reorder: bool }
  pub struct DragPos { pub x: f64, pub y: f64, pub w: f64, pub h: Option<f64>, pub rotate: f64 }
  #[serde(tag = "cmd", rename_all = "kebab-case")]
  pub enum CommandBody { SetPos { pos: DragPos }, FreePosition, ReturnToFlow, Delete, Duplicate, Undo, Redo, Deselect, Go { slide: u32 } }
  pub struct Command { pub id: i64, pub body: CommandBody }   // serialises as {"v":1,"id":…,"cmd":…}
  pub fn parse_page_message(json: &str) -> Result<PageMessage, ProtocolError>   // checks "v" == 1 first
  pub fn command_script(cmd: &Command) -> String   // "window.__studioBridge.dispatch(" + JSON string literal of the command JSON + ")"
  #[derive(Debug, thiserror::Error)] pub enum ProtocolError { #[error("Unsupported bridge version: {0}")] Version(u64), #[error("Bad message: {0}")] Bad(String) }
  ```

- [ ] **Step 1: Write the failing tests** in `crates/protocol/tests/fixtures.rs`. They read the fixture folder from `STUDIO_SRC` (env, else `dev.env`).
  - `every_page_fixture_parses`: each non-`cmd-*` fixture parses. Serialising and parsing it again gives an equal `PageMessage`.
    - Compare typed values, not `serde_json::Value`: `120` and `120.0` are different `Value`s.
    - The serialised form carries `"v":1`. `Result.error` is skipped when `None`.
  - `every_command_fixture_round_trips`: each `cmd-*` fixture deserialises into `Command`. Serialising and deserialising it again gives an equal `Command`, and the JSON has `"v":1` and the fixture's keys.
  - `version_two_is_refused`: gives `ProtocolError::Version(2)`.
  - `command_script_escapes`: a `Go { slide: 3 }` with id 1 gives exactly `window.__studioBridge.dispatch("{\"v\":1,\"id\":1,\"cmd\":\"go\",\"slide\":3}")`.
- [ ] **Step 2: Run them to see them fail.** `cargo test -p studio-protocol`. Expected: FAIL (the crate is empty).
- [ ] **Step 3: Implement the crate.**
- [ ] **Step 4: Run them again.** Expected: PASS.
- [ ] **Step 5: Commit.** `git add Cargo.toml Cargo.lock crates/protocol LICENSE README.md dev.env && git commit -m "Protocol crate for the Studio bridge, checked against Studio's fixtures"`.

### Task 9: The sidecar client crate (APP)

**Files:**
- Create: `crates/sidecar/{Cargo.toml,src/lib.rs}`, `crates/sidecar/tests/fake_serve.rs`, `crates/sidecar/tests/fake-serve.sh`.

**Interfaces:**
- Consumes: Task 7's event and command shapes and exit codes.
- Produces, crate `studio-sidecar` (std, serde_json, async-channel; no GUI):
  ```rust
  #[derive(Debug, Clone, PartialEq, Deserialize)] #[serde(tag = "event", rename_all = "kebab-case")]
  pub enum SidecarEvent { Unpacked { id: String, title: String, entry: String, #[serde(rename="workDir")] work_dir: PathBuf, code: Vec<String>, trusted: bool },
      Confirm { question: String }, Ready { port: u16, #[serde(rename="baseUrl")] base_url: String, #[serde(rename="workDir")] work_dir: PathBuf },
      Saved { updated: String }, Conflict { message: String }, SaveFailed { message: String, #[serde(default)] names: Vec<String> },
      Busy { #[serde(rename="baseUrl")] base_url: String }, Error { message: String, #[serde(default)] log: String }, Stopped,
      Exited { code: Option<i32> } }   // synthesised by the client when the process ends
  pub struct SpawnOptions { pub stagepack: PathBuf, pub home: PathBuf, pub deck: PathBuf, pub addons: Vec<PathBuf> }
  pub struct Sidecar { /* child, stdin */ }
  impl Sidecar {
      pub fn spawn(o: SpawnOptions) -> std::io::Result<(Sidecar, async_channel::Receiver<SidecarEvent>)>   // reader thread parses stdout lines
      pub fn answer(&mut self, yes: bool) -> std::io::Result<()>
      pub fn save(&mut self) -> std::io::Result<()>
      pub fn stop(&mut self) -> std::io::Result<()>   // writes {"cmd":"stop"} then closes stdin
  }
  impl Drop for Sidecar { /* closes stdin; waits up to 10 s, then kills */ }
  ```
  An unparseable stdout line becomes `Error { message: "Unreadable message from Stagepack: <line>", log: "" }`.

- [ ] **Step 1: Write `fake-serve.sh`.** It prints events from a script file named by `FAKE_SERVE_SCRIPT`, and appends each stdin line to `FAKE_SERVE_STDIN`. On a stdin EOF it prints `{"event":"stopped"}` and exits with `FAKE_SERVE_EXIT` (default 0).
- [ ] **Step 2: Write the failing tests** in `tests/fake_serve.rs`:
  - `events_arrive_in_order`: unpacked, then ready, then on `stop()`, stopped and `Exited { code: Some(0) }`.
  - `answer_and_save_write_json_lines`: stdin holds exactly `{"answer":"yes"}` then `{"cmd":"save"}`.
  - `drop_closes_stdin`: dropping the `Sidecar` makes the fake print stopped and exit.
  - `exit_code_three_is_reported`: `FAKE_SERVE_EXIT=3` gives `Exited { code: Some(3) }`.
  - `garbage_line_is_an_error_event`.
- [ ] **Step 3: Run them to see them fail.** `cargo test -p studio-sidecar`. Expected: FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run them again.** Expected: PASS.
- [ ] **Step 6: Commit.** `git commit -m "Sidecar client: run stagepack serve and read its events"`.

### Task 10: The dev home and a fixture talk (APP)

**Files:**
- Create: `scripts/dev-home.sh`, `scripts/check-dev-home.sh`, `fixtures/hello/{slides.md,package.json,components/Box.vue}`.

**Interfaces:**
- Consumes:
  - SP's built `dist/linux` (Task 7 step 5);
  - STUDIO at `editor-bridge` (Tasks 1-4);
  - `dev.env`.
- Produces:
  - `target/dev-home/`: a Stagepack home with `stagepack`, `node/`, `runtimes/53` and `slidev-addon-studio` installed in `runtimes/53/node_modules`;
  - `target/fixtures/hello.sptx`.

  The app finds them through `STUDIO_DEV_HOME` (Task 11).

**What the scripts do:**
- **`dev-home.sh`:**
  - `rsync -a --delete $STAGEPACK_SRC/dist/linux/ target/dev-home/`;
  - `pnpm pack` STUDIO into `target/`;
  - unpack the tarball's `package/` into `target/dev-home/runtimes/53/node_modules/slidev-addon-studio/`. This copies, the way Stagepack's `build/addon.sh` places its own addon, with no `npm install`, which would prune the copied-in `slidev-addon-stagepack` as extraneous;
  - check, with the dev home's Node, that the runtime's `@vueuse/core` satisfies `^14.0.0` and `yaml` satisfies `^2.9.0` (today 14.4.0 and 2.9.1). If either doesn't, it exits 1 with the name;
  - `target/dev-home/stagepack pack fixtures/hello -o target/fixtures/hello.sptx`;
  - finally it prints `dev home ready: <abs path>`.
- **The fixture talk:**
  - slide 1 is a title;
  - slide 2 has `<Box v-drag pos="100,100,200,120" />`, plus a paragraph in the flow;
  - slide 3 is a list.

  `package.json` holds `{ "name": "hello-fixture", "private": true }`. `Box.vue` is a plain coloured div with a `label` prop.

- [ ] **Step 1: Write both scripts and the fixture.** `check-dev-home.sh` runs `serve --json --home target/dev-home --addon <home>/runtimes/53/node_modules/slidev-addon-studio target/fixtures/hello.sptx`, waits up to 90 s for `ready`, closes stdin, and expects `stopped`. It prints `dev home check: ok`.
- [ ] **Step 2: Run them,** in the background. `scripts/dev-home.sh && scripts/check-dev-home.sh`. Expected: `dev home ready: …` and `dev home check: ok`.
- [ ] **Step 3: Commit.** `git add scripts fixtures && git commit -m "Dev home: Stagepack's Linux build plus the Studio fork, and a fixture talk"`. `target/` is gitignored.

### Task 11: The window and the canvas (APP)

**Files:**
- Create:
  - `crates/app/Cargo.toml` (bin `stagepack-studio`; deps per Global Constraints, `studio-protocol`, `studio-sidecar`, `async-channel`, `anyhow`; dev-dep `gpui-kit` with `test-support`);
  - `crates/app/src/main.rs`, `crates/app/src/canvas.rs`, `crates/app/src/workspace_view.rs`, `crates/app/tests/ui_layout.rs`.

**Interfaces:**
- Consumes: `studio_protocol::{parse_page_message, command_script, Command, CommandBody, PageMessage}`.
- Produces:
  ```rust
  // canvas.rs
  pub struct Canvas { webview: Entity<gpui_webview::WebView>, next_id: i64, ready: bool, pending: Vec<CommandBody> }
  pub enum CanvasEvent { Page(PageMessage), ProtocolError(String) }
  impl EventEmitter<CanvasEvent> for Canvas {}
  impl Canvas {
      pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self   // builds wry with with_ipc_handler -> async_channel, spawns the receive loop
      pub fn load(&mut self, base_url: &str, slide: u32, cx: &mut Context<Self>)   // ready=false, loads "<base_url>/<slide>"
      pub fn send(&mut self, body: CommandBody, cx: &mut Context<Self>) -> i64   // queues until ready; then evaluate_script(command_script)
  }
  // workspace_view.rs
  pub struct WorkspaceView { /* title bar, canvas area, inspector slot */ }
  pub const CANVAS_ID: &str = "canvas"; pub const INSPECTOR_ID: &str = "inspector"; pub const TITLE_ID: &str = "title";
  ```

**Behaviour:**
- **One `Entity<WebView>` per window, built once and never dropped while the window lives.** Published 0.9.0 aborts on X11 when a web view is dropped, so it is hidden instead.
- **The `ready` handshake:** a `Ready` message sets `ready = true` and flushes `pending`. A `load` sets it back to `false`.
- **The layout:**
  - `TitleBar` at the top, showing the file name, a `•` when the talk is dirty, and the Undo, Redo and Save buttons;
  - below it, `h_resizable`: the canvas `flex_1`, then the inspector at 280 px.
- **`main.rs`:** on Linux, `platform::linux(WindowingModes::X11)`, with assets.
- **Debug builds only:** if `STUDIO_DEV_URL` is set, the app skips the sidecar and loads that URL in the canvas. This is for the hand smoke test below, and release builds ignore it.

- [ ] **Step 1: Write the failing headless test** `tests/ui_layout.rs` `layout_has_canvas_inspector_and_title`.
  - It renders a `WorkspaceView` built with a test constructor `WorkspaceView::for_test(cx)`, which uses a plain div in place of the web view.
  - It asserts that `find(CANVAS_ID)`, `find(INSPECTOR_ID)` and `find(TITLE_ID)` exist.
  - It asserts that the inspector is to the right of the canvas, with a width of 280 ± 1 px at a 1280×800 window.
- [ ] **Step 2: Run it to see it fail.** `cargo test -p stagepack-studio --test ui_layout`. Expected: FAIL.
- [ ] **Step 3: Implement** `main.rs`, `Canvas` and `WorkspaceView`.
- [ ] **Step 4: Run it again.** Expected: PASS.
- [ ] **Step 5: Smoke test it by hand.**
  - Start serve on the dev home by hand.
  - Run `STUDIO_DEV_URL=<baseUrl> cargo run -p stagepack-studio`. This is a debug-only override that loads the URL directly.
  - Expected: the slide shows, there are no Studio toolbar or dock, and clicking the Box draws Studio's selection outline.
- [ ] **Step 6: Commit.** `git commit -m "App window: title bar, Slidev canvas in gpui-webview, inspector slot"`.

### Task 12: Opening, trust, saving and closing (APP)

**Files:**
- Create: `crates/app/src/document.rs` (pure state machine), `crates/app/src/document_view.rs`, `crates/app/tests/document_state.rs`, `crates/app/tests/ui_dialogs.rs`.
- Modify: `crates/app/src/main.rs` (CLI path, File > Open, `STUDIO_DEV_HOME`), `crates/app/src/workspace_view.rs`.

**Interfaces:**
- Consumes: Task 9's `Sidecar`/`SidecarEvent`, and Task 11's `Canvas`/`CanvasEvent`.
- Produces:
  ```rust
  // document.rs (no gpui)
  pub enum DocState { Opening, AwaitingTrust { question: String }, Ready { base_url: String }, Failed { message: String, log: String }, Closed }
  pub struct Document { pub path: PathBuf, pub title: String, pub state: DocState, pub dirty: bool, pub selection: Option<SelectionInfo>, pub can_undo: bool, pub can_redo: bool }
  pub enum Effect { AskTrust(String), LoadCanvas { base_url: String }, ShowMessage(String), CloseWindow }
  impl Document {
      pub fn new(path: PathBuf) -> Self
      pub fn on_sidecar(&mut self, e: SidecarEvent) -> Vec<Effect>
      pub fn on_page(&mut self, m: &PageMessage) -> Vec<Effect>
      pub fn on_protocol_error(&mut self, e: &str) -> Vec<Effect>   // Failed { message: MISMATCH_MESSAGE + e, log: "" }
      pub fn wants_close_prompt(&self) -> bool   // dirty && state is Ready
  }
  pub const MISMATCH_MESSAGE: &str = "This copy of the editor and its runtime don't match: ";
  pub const BUSY_MESSAGE: &str = "This presentation is already open in Stagepack. Close it there, then open it here.";
  pub const NOT_TRUSTED_MESSAGE: &str = "The presentation was not opened.";
  ```

**The mapping:**
- **Startup:** `Unpacked` sets the title. `Confirm` leads to `AwaitingTrust` and `AskTrust`. `Ready` leads to `Ready` and `LoadCanvas`.
- **After saving:** `Saved` sets `dirty = false`. `Conflict` and `SaveFailed` give a `ShowMessage(message)` and keep `dirty`.
- **Exits:**
  - `Busy` gives `Failed{BUSY_MESSAGE}`.
  - `Error` gives `Failed{message, log}`.
  - `Exited{3}` gives `Failed{NOT_TRUSTED_MESSAGE}`.
  - `Stopped` and `Exited{0}` give `Closed`.
  - `Stopped` and any `Exited` never replace a `Failed` state. `Error` is followed by `Stopped`, and the error must stay on screen.
- **From the page:**
  - `Edited` sets `dirty = true`.
  - `Selection` replaces `selection`.
  - `History` sets `can_undo` and `can_redo`.
  - `Ready` clears `selection`.

**The UI:**
- **The trust dialog** shows the question with **Yes** and **No**, mapping to `answer(true)` and `answer(false)`.
- **Closing while dirty** gets a dialog: "Save changes to <title>?", with **Save**, **Don't Save** and **Cancel**. **Save** saves, waits for `Saved`, then closes. On `Conflict` or `SaveFailed` it shows the message and keeps the window open.
- **Ctrl+S** saves.
- **The sidecar** is spawned with `stagepack = $STUDIO_DEV_HOME/stagepack`, `home = $STUDIO_DEV_HOME`, and `addons = [$STUDIO_DEV_HOME/runtimes/53/node_modules/slidev-addon-studio]`.

- [ ] **Step 1: Write the failing state tests** in `tests/document_state.rs`:
  - `trusted_flow_loads_canvas`;
  - `confirm_asks_trust`;
  - `busy_shows_message`;
  - `not_trusted_exit_fails_with_message`;
  - `slidev_death_fails_with_log`: `Error{message:"slidev exited", log:"boom"}`, then `Stopped`, then `Exited{1}`. The state stays `Failed{log:"boom"}`;
  - `protocol_error_fails_with_mismatch`: `on_protocol_error("Unsupported bridge version: 2")` gives `Failed` with the message `MISMATCH_MESSAGE` + that text;
  - `edited_marks_dirty_saved_clears`;
  - `conflict_keeps_dirty`: the `ShowMessage` holds the sidecar's message text;
  - `reload_clears_selection`: `Ready` after a `Selection` gives `selection == None`.
- [ ] **Step 2: Write the failing UI tests** in `tests/ui_dialogs.rs`:
  - `trust_dialog_shows_question_and_buttons`: ids `trust-yes` and `trust-no`.
  - `close_when_dirty_prompts`: ids `save-changes-save`, `save-changes-discard` and `save-changes-cancel`. **Cancel** keeps the window.
- [ ] **Step 3: Run them to see them fail.** `cargo test -p stagepack-studio --test document_state --test ui_dialogs`. Expected: FAIL.
- [ ] **Step 4: Implement** `document.rs`, then `document_view.rs` (wiring sidecar events and canvas events into `Document` and applying its effects), then main's open paths.
- [ ] **Step 5: Run them again.** Expected: PASS. Then `cargo test --workspace`.
- [ ] **Step 6: Commit.** `git commit -m "Open, trust, save and close a talk through the sidecar"`.

### Task 13: The inspector (APP)

**Files:**
- Create: `crates/app/src/inspector.rs`, `crates/app/tests/ui_inspector.rs`.
- Modify: `crates/app/src/workspace_view.rs`, which puts the inspector in its slot and forwards `InspectorEvent::Send` to `Canvas::send`, with Undo and Redo wired the same way.

**Interfaces:**
- Consumes: `Document.selection`, `can_undo` and `can_redo` (Task 12), and `Canvas::send` (Task 11).
- Produces:
  ```rust
  pub struct Inspector { selection: Option<SelectionInfo>, x: Entity<InputState>, y: …, w: …, h: …, rotate: … }
  pub enum InspectorEvent { Send(CommandBody) }
  impl Inspector { pub fn set_selection(&mut self, s: Option<SelectionInfo>, window: &mut Window, cx: &mut Context<Self>) }
  ```

**What it shows:**
- **No selection:** the text "Click something on the slide to select it." (id `inspector-empty`).
- **A selection:** its `label` (id `inspector-label`).
- **When positioned:** X, Y, W, H and Rotate number inputs (ids `pos-x`, `pos-y`, `pos-w`, `pos-h`, `pos-rotate`). H is empty when `h` is `None`. Enter sends `SetPos` with the edited field changed and the rest as they are. A **Return to flow** button (`return-to-flow`).
- **When not positioned:** a **Free position** button (`free-position`), disabled when `nested`.
- **Always:** **Duplicate** (`duplicate`) and **Delete** (`delete`) buttons.

- [ ] **Step 1: Write the failing UI tests** in `tests/ui_inspector.rs`. Collect emitted `InspectorEvent`s through `cx.subscribe`.
  - `empty_state_text`.
  - `positioned_selection_shows_fields`: `pos-x` has the value `120`, and `pos-h` is empty.
  - `enter_in_x_sends_set_pos`: type `140` in `pos-x` and press Enter. Expect `SetPos{pos: {x:140, y:80, w:200, h:None, rotate:0}}`.
  - `flow_selection_offers_free_position`.
  - `nested_disables_free_position`.
  - `delete_sends_delete`.
  - `undo_redo_follow_history`: in the `WorkspaceView` test constructor, the title-bar buttons `undo` and `redo` are disabled until `can_undo` or `can_redo` is true. Clicking `undo` sends `CommandBody::Undo`.
- [ ] **Step 2: Run them to see them fail.** `cargo test -p stagepack-studio --test ui_inspector`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them again.** Expected: PASS. Then `cargo test --workspace`.
- [ ] **Step 5: Commit.** `git commit -m "Inspector: the selection's position, flow, duplicate and delete, driven through the bridge"`.

### Task 14: End-to-end check on Linux (APP)

**Files:**
- Create: `scripts/check-saved.sh`, `docs/checks/plan-1-linux.md`.

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write `check-saved.sh FILE.sptx`.** It unzips the talk to a temp folder and checks four things:
  1. `slides.md` contains `pos="140,100,200,120"`, the value the check below types;
  2. no entry name contains `studio`, `addons-preload` or `.slidev/`;
  3. `stagepack pack` of the unzipped folder succeeds, so the talk is still a plain project;
  4. plain Stagepack opens it without the editor. SP's stock `dist/linux/stagepack serve --json --home $STAGEPACK_SRC/dist/linux FILE`, with no `--addon`, reaches `ready`, and `GET <baseUrl>/` returns 200. Closing stdin then gives `stopped`.

  It prints `saved talk check: ok`.
- [ ] **Step 2: Run the manual check,** and record each result in `docs/checks/plan-1-linux.md` with the date.
  1. Copy `target/fixtures/hello.sptx` to `/tmp/…/my talk æøå 100%.sptx`.
  2. Open it: `STUDIO_DEV_HOME=target/dev-home cargo run --release -p stagepack-studio -- "<path>"`.
  3. Go to slide 2 and click the Box. The inspector shows `<Box>` and X 100.
  4. Type 140 in X and press Enter. The Box moves, and the title shows `•`.
  5. Press Undo, then Redo. The Box moves back, then forward again.
  6. Click the paragraph. **Free position** shows; click it and the paragraph becomes positioned.
  7. Undo it.
  8. Press Ctrl+S. The `•` goes away.
  9. Close the window, with no prompt.
  10. Run `scripts/check-saved.sh "<path>"`: `saved talk check: ok`.
  11. Reopen the talk, make an edit and close. The save prompt appears. **Cancel** keeps the window. **Don't Save** closes, and the file's mtime is unchanged.
  12. Open the same file in plain Stagepack first (`dist/linux/stagepack open`), then in the app. The busy message shows.
- [ ] **Step 3: Commit.** `git add scripts/check-saved.sh docs/checks/plan-1-linux.md && git commit -m "Plan 1: Linux end-to-end check"`.
