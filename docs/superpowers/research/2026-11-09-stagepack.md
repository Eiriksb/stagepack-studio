# Stagepack as an editor sidecar: research notes

Read-only research. Sources:

- Stagepack at origin/main = `5e00427` (worktree `/home/bjoeir/Documents/Projects/stagepack/.claude/worktrees/export-pptx`). Paths below are relative to it unless absolute.
- Slidev 53.0.0 as installed in `/home/bjoeir/.local/share/stagepack/runtimes/53/node_modules` (called `RT/` below). The bundle file names are hashed: `@slidev/cli/dist/resolver-BIhhHpe7.mjs`, `serve-DzvJFv6v.mjs`, `cli.mjs`.

---

## 1. The CLI

Dispatch is in `cmd/stagepack/main.go` `run(args []string, stdout, stderr io.Writer) int` (L42). Usage string L27: `stagepack {pack|open|export-pptx|install|uninstall|version} [args]`. Every subcommand uses its own `flag.NewFlagSet(..., flag.ContinueOnError)`. For `pack`, `open` and `export-pptx`, positional args may come before or after flags (a parse loop, e.g. L102-117).

Special cases in `run`:
- No args: on Windows it becomes `install` (L44-46); elsewhere it prints the usage and exits 2 (L47-50).
- On Windows a single arg ending in `.sptx` becomes `open FILE` (a double-click, L52-54).

| command | flags (where parsed) | exit codes | stdout |
|---|---|---|---|
| `pack DIR -o OUT` | `-o OUT` (required), `--entry slides.md`, `--title T`, `--exclude PREFIX` (repeatable), `--only-used-public`, `--meta k=v` (repeatable). `runPack` main.go L88-139 | 0 ok, 1 error, 2 usage; `-h` gives 0 | one human line, `packed "Title" (Slidev 53.0.0) into OUT` (L137; the title is Go `%q`, which the addon parses with `JSON.parse`). Notes and warnings go to stderr. |
| `open FILE` | `--headless`, `--trust-code`, `--home DIR` (default: the folder of the symlink-resolved executable), `--default-browser`. `runOpen` L145-200 | 0 on a clean stop, and also when it hands over to a running instance. 1 on an error, `ErrNotTrusted` included. 2 usage | with `--headless`, JSON lines from `ui.NewHeadless` (`internal/ui/headless.go` L27-91): `{"event":"opening"|"status"|"notify"|"error"|"confirm","status":"saved|pending|failed|throwaway","title","message","detail","answer"}`. Confirm always answers `"no"` (L84-87). **No event carries the port or URL.** |
| `export-pptx FILE` | `--animations`, `--headless`, `--trust-code`, `--yes` (replace an existing .pptx), `--home DIR`. `runExportPptx` L206-275 | 0 done, 1 failed, 2 usage, 3 cancelled or not trusted, 4 busy (`ErrBusy`), 5 no browser (`ErrNoBrowser`, L263-265; the doc comment at L204-205 leaves 5 out) | with `--headless`, JSON lines from `export.HeadlessDialogs` (`internal/export/export.go` L583-635): `{"event":"confirm"|"save"|"error","title","message","path","answer"}`, then `{"event":"progress","step","fraction"}` (`ui.NewHeadlessProgress`, `internal/ui/progress.go` L46) and `{"event":"finished","title","message","buttons"}` (L76). SIGINT/SIGTERM cancel (main.go L254). |
| `install` / `uninstall` | `--yes`, `--home DIR` (default `$XDG_DATA_HOME/stagepack` or `%LOCALAPPDATA%\Stagepack`). `installFlags` `cmd/stagepack/install.go` L120 | 0 (also when the person says no), 1 error (uninstall: also not installed), 2 usage or not Windows/Linux (main.go L66-68) | human text (terminal on Linux, message boxes on Windows) |
| `version`, `--version` | none | 0 | the version string (`internal/version`) |

Logging: `startViewerLog(home)` (`cmd/stagepack/viewerlog.go` L20) sends Go's `log` to `<home>/logs/viewer.log` (rotated at 1 MiB) and to stderr.

**Machine-readable output today:** only the `--headless` JSON lines of `open` and `export-pptx`, plus the `packed ...` line of `pack`. Nothing reports the port, URL, work folder or trust status as data. The e2e tests get the URL by setting `BROWSER` to a script that records it (`test/e2e_test.go` L66-74 and L91-96). The other way is to read `<home>/instances/<key>.json`, an internal record.

---

## 2. The open flow, end to end

Entry point: `app.Open(ctx context.Context, o app.Options) error` (`internal/app/app.go` L119).

```go
type Options struct { Home, DeckPath string; UI func(ui.Actions) ui.UI; Browser func(url string) error;
    GOOS string; Env func(string) string; TrustCode, DefaultBrowser bool }   // L36-48
```

Steps (numbering as in the code):

0. **Setup** (L119-176). `home` and `deckPath` are made absolute. The runtime is `rtDir = <home>/runtimes/<slidevrt.Major()>` (L149). `Major()` comes from the `@slidev/cli` version in the embedded lock (`runtime/runtime.go` L58, `//go:embed package-lock.json` L16), so the runtime is fixed at build time and there is no choice beyond it. The journal is `<home>/journal`. The browser is chosen at L139-146: if `browser.FindAppBrowser(goos, env, exec.LookPath)` (`internal/browser/appwindow.go` L176) finds Chrome, Edge, Chromium or Brave (snaps skipped), `DefaultBrowser` is off and `$BROWSER` is unset, it uses `browser.NewAppWindow(exe, <home>/browser/<instance.Key(deck)>, goos)` (L44: app mode with its own profile, args at `AppArgs` L49). Otherwise it uses `browser.Open(url)` (`internal/browser/browser.go` L15: `$BROWSER`, else `xdg-open` or `rundll32`). Closing all app windows calls `s.stop()` (L156-166).
1. **Clean stale work folders** (L179): `workspace.CleanStale(runtimeDir string, alive func(dir string) bool) error` (`internal/workspace/extract.go` L207). The keep-predicate is `session.inUse()` (app.go L380-407). It keeps a folder if its mtime is younger than `staleAge = 10 * time.Minute` (L79), if it is the `WorkDir` of an `instance.List(home)` record that is `instance.Alive`, or if it is the `WorkDir` of `instance.RunningExports(home)` (`internal/instance/export.go` L43).
2. **One instance per deck** (L183): `instance.Claim(home, absPath string, alive func(Record) bool) (existing *Record, h *Handle, err error)` (`internal/instance/instance.go` L105).
   - The record is `<home>/instances/<sha256(PathKey(abs))[:16]>.json`, holding `Record{PID, Port, BaseURL, DeckPath, WorkDir, Token, StartedAt}` (L29-40).
   - Check-and-write is serialised by `<key>.lock`: an O_EXCL token file with `lockWait` 35 s, broken when older than `staleLock` 30 s (L42-52, `acquire` L258).
   - `Alive(r Record) bool` (L193): the PID is running AND `GET <base>/` answers within 1 s (a timeout counts as alive). A record with no address counts while it is younger than `startupGrace` 2 min.
   - If an instance exists, the browser is opened on `existing.BaseURL + "/presenter/"` and Open returns (L189-199). Later calls: `(*Handle).Update(port int, baseURL, workDir string) error` (L138), `Renew() error` (L157), `Release()` (L174).
3. **Unpack** (L202): `workspace.Extract(deckPath, runtimeDir string) (workspace.Work, error)` (extract.go L72).
   - It reads the manifest (`deck.ReadManifest`, `internal/deck/manifest.go` L53) and runs `deck.Check(m, slidevrt.SlidevVersion())` (L82): it refuses format > 1 or a different Slidev major (`*deck.TooNewError`).
   - The entry defaults to `slides.md` and must pass `deck.CheckSlidesEntry` (`internal/deck/entries.go` L145): a top-level `.md` that does not start with `-`.
   - Every name is validated before anything is written (`plan` L144, using `deck.CheckEntryNames`; symlinks are refused).
   - It writes into `os.MkdirTemp(<rt>/work, "")` (L106-110). The folder gets a random numeric name such as `work/2134987343`, not the deck ID. `vendor/node_modules/*` lands in `node_modules/*` (`deck.ExtractedPath`, entries.go L23). The manifest is not written to disk.
   - `Work` (L22-35) is `{Dir, Manifest, Entries []string (zip order, manifest excluded), Code []string, Hash string}`.
   - `Hash` is sha256 over the sorted lines `"<name>" <sha256>` for every entry except the manifest and the slides entry (folders included, with an empty sum), plus `code "<reason>"` for each file-based reason (L114-138). So editing slides.md keeps the Hash. Changing any other file (an image in public/, an imported .md) changes it.
3a. **Trust check** (L217-245): `app.CodeCheck{Home, Node, RuntimeDir, TrustCode, Ask func(string) bool, ReadHeadmatter}` (L279-291) and `(CodeCheck).Allow(ctx, w *workspace.Work, deckPath string) bool` (L298-327).
   - What counts as code, from the files: `deck.CodeReasons(names []string, read func(string) []byte) []string` (`internal/deck/code.go` L53). It counts `vite.config*`, `uno.config*`, `unocss.config*`, `postcss.config*`, `.postcssrc*`; anything under `setup/`; a top-level `index.html`; any `.vue .js .mjs .cjs .ts .mts .cts .jsx .tsx .node .wasm` (public/ included); any package.json that lists `slidev.addons` (other than the bundled one), has `postcss` or can't be parsed; a bundled package with code files; and, whatever they hold, bundled packages that shadow the export addon (`shadowsAddon` L180: the name contains "stagepack") or the runtime (`shadowsRuntime` L194: any package in the embedded lock, bundled theme names, `talkLoads` L215 = vite-plugin-pwa, playwright-chromium, sass-embedded, sass, sugarss, less, stylus).
   - From the headmatter: `slidev.ReadHeadmatter(ctx, node, runtimeDir, slidesPath string) (slidev.Headmatter, error)` (`internal/slidev/headmatter.go` L47) runs the runtime's `@slidev/parser/dist/core.mjs` `parse` + `resolveConfig(hm, undefined, file)` in runtimeDir, with NODE_OPTIONS removed (script L29-38). It returns `{Theme, Addons, PWA json.RawMessage}`. `deck.HeadmatterReasons(theme, addons, pwa json.RawMessage) []string` (code.go L269) counts any theme but the bundled ones (`bundledThemes` L326: `none` plus `@slidev/theme-*` in the lock, i.e. `default`), any addon not in `bundledAddons` (L167), and any `pwa` value but false. An unreadable headmatter counts as code (app.go L303-306). `(*Work).AddCode(reasons...)` (extract.go L43) folds these into Hash: `sha256(oldHash + "\n" + "code %q\n"...)`.
   - The per-version trust record is `trust.Store{Path: <home>/trusted.json}` (`internal/trust/trust.go`). The file is `{"decks": {"<manifest id>": {"hash", "title", "trusted"}}}`. `Trusted(id, hash string) bool` (L48) needs an exact hash. `Remember(id, hash, title string) error` (L61) replaces the earlier version (temp file and rename; last writer wins).
   - The question: if `!w.HasCode() || c.TrustCode`, it is allowed. Else if `Trusted`, allowed. Else `Ask(CodeQuestion(file, title, reasons))` (L332-347: up to 8 reasons, then Yes/No text), and a yes calls `Remember`. `--trust-code` allows without remembering. In the app, Ask is `ui.Confirm` (a tray dialog; headless always says no).
   - Not allowed: `workspace.Remove(work)`, `h.Release()`, `ErrNotTrusted` (L228-233). Allowed with code: `h.Renew()`; `ErrNotOwner` means another launch took it over meanwhile, so it stops quietly (L234-245).
4. **Journal recovery** (L246-265): `key = save.JournalKey(deckID, absPath string) string` (`internal/save/journal.go` L40) = id (hashed if not plain) + "-" + sha256(path)[:16]. The log is `<home>/logs/<key>.log`. `save.Recover(j Journal, key string, m deck.Manifest, workSlides string) (applied bool, kept string, err error)` (L134) writes the journaled slides into the work entry when `BaseUpdated == m.Updated`; otherwise `save.SetAside(j, key) (kept string, err error)` (L155) moves it to `<key>.kept-<time>.json`.
5. `ui.SetStatus(... Pending ...)`, then `s.ui.Run(func(){ go s.start() })` (L267-269). `start` (L418) calls `startup` (L442).
6. **Start Slidev** (L443-453):
   ```go
   cfg := slidev.Config{Node: <home>/node/bin/node (node\node.exe on Windows), CLI: <rt>/node_modules/@slidev/cli/bin/slidev.mjs,
       WorkDir: work.Dir, Entry: entry, LogPath: <home>/logs/<key>.log}
   port, _ := slidev.FreePort()          // internal/slidev/run.go L50: listen 127.0.0.1:0, close, return the port
   srv, err := slidev.Start(ctx, cfg, port) // L101: func Start(ctx context.Context, c Config, port int) (*Server, error)
   ```
   - `Config` (run.go L23-30): `{Node, CLI, WorkDir, Entry, LogPath string; ReadyTimeout time.Duration}`. The default timeout is 120 s (L33). There is no field for extra node args or env.
   - Exact args: `Args(c Config, port int) []string` (L45-47) returns `{CLI, "./"+Entry, "--port", N, "--open", "false", "--log", "warn"}`. There is never `--remote`, so Slidev binds `localhost` with `strictPort: true` (`RT/@slidev/cli/dist/cli.mjs` L124-130). `cmd.Dir = WorkDir`, and stdout and stderr go to LogPath.
   - Env: `childEnv(os.Environ())` (L160-178) removes any `BROWSER`, `STAGEPACK_VIEWER` and `NODE_OPTIONS`, then adds `BROWSER=none`, `STAGEPACK_VIEWER=1` (`brand.ViewerEnv`, which makes the export addon inert) and `NODE_OPTIONS=<old> --dns-result-order=ipv4first`.
   - Process control: on Linux, `Setpgid` + `Pdeathsig: SIGKILL` (`proc_linux.go` L15-17); Stop sends SIGTERM to the group, then SIGKILL after `stopGrace` 5 s (L22-36). On Windows, a kill-on-close job object (`proc_windows.go` L28-50); Stop closes the job at once, with no graceful stop.
   - Port race: FreePort closes the socket before Slidev binds. With `strictPort`, a lost race makes Slidev exit, and Start fails with "slidev exited before it was ready". There is no retry.
   - Readiness: `waitReady` (L189-223) polls every 200 ms `GET http://127.0.0.1:N/presenter/`, then `GET http://localhost:N/` (2 s client timeout, no proxy). The first 200 sets `BaseURL`. It fails on process exit, ctx cancel or the deadline. `Server` methods: `Port() int`, `BaseURL() string`, `Done() <-chan error`, `Stop() error` (L72-97). `LogTail(path string) string` (L226) gives the last 20 lines.
7. `h.Update(srv.Port(), srv.BaseURL(), work.Dir)` (L467). `watchServer` (L514) stops everything with an error dialog if Slidev exits by itself.
8. **Browser:** `openURL("/presenter/")` (L470, L532-543) calls `o.Browser(base + path)`.
9. **Notes write-back** (L474-486): `save.NewWatcher(save.WatcherConfig{SlidesPath: <work>/<entry>, DeckPath, EntryName: entry, DeckID, Key, Journal, Base: m.Updated, OnStatus})` (`internal/save/watcher.go` L63-78, L122), then `go w.Run(ctx)` (L167).
   - It polls the one slides file every 500 ms (`DefaultPoll`, L54). It writes a journal entry 1 s after the last change (`DefaultJournalDelay`) and rewrites the deck 30 s after it (`DefaultQuietDelay`) or on `Flush() error` (L206).
   - `save` (L331): journal first, then `rewrite(deckPath, entry, data, now)`, then `base = now` and the journal entry is cleared. Empty reads count as Slidev mid-write and are ignored (L301-304).
   - Journal: `Entry{DeckPath, DeckID, BaseUpdated, Saved time.Time; Slides []byte}` (journal.go L22-28). `Journal{Dir}` with `Put/Get/Clear` (L78/L97/L116); writes are atomic (`writeFileAtomic` L172).
   - `save.RewriteEntry(deckPath, name string, data []byte, now time.Time) error` (`internal/save/writeback.go` L38). It replaces exactly one existing entry (an error if it is missing or duplicated, or if it is the manifest), raw-copies every other entry in order (`zw.Copy`), deflates the new entry and the manifest, and keeps unknown manifest fields (`withUpdated` L164). It writes `<dir>/.<base>.<random>.tmp` (`tmpPattern` L26), keeps the permissions, Syncs, renames over the deck (`replaceFile`, retried 5x250 ms on Windows, journal.go L213), then `syncDir`.
   - Status values: `Saved|Pending|Failed|Throwaway` (watcher.go L18-25). Throwaway means the deck is in a temp or attachment folder (`instance.IsThrowawayCopy(p, goos string, env func(string) string) bool`, `throwaway.go` L12).
   - Save a copy: `saveCopy` (app.go L612-679) claims the new path, runs `copyFile`, falls back to `workspace.Rebuild(w Work, dst string, now time.Time) error` (`internal/workspace/rebuild.go` L24; it packs exactly `w.Entries` from the work folder), then `(*Watcher).SetTarget(deckPath, key string) error` (watcher.go L256).
10. **Shutdown:** `session.stop()` (app.go L547-594), run once:
    1. Cancel the ctx and wait for start.
    2. Close the app window.
    3. `srv.Stop()`.
    4. Stop the watcher, then `FlushSettled() error` (watcher.go L218: reads until two reads 100 ms apart agree, at most 2 s, then saves). If that fails, `flushFailed` keeps the work folder when the journal doesn't have the notes (L598-609).
    5. `workspace.Remove(w Work) error` (extract.go L198).
    6. `h.Release()`, then `ui.Quit()`.
    - A crash or kill leaves the work folder and record; the next `open` removes them via CleanStale (step 1) once they are 10 min old and the PID is dead.

---

## 3. What a headless sidecar for an editor needs

### What is missing

1. **Port, URL, work folder and trust status as data.** `open --headless` prints none of them, and it still opens a browser window (an app window or `$BROWSER`/xdg-open). There is no "don't open a browser" option.
2. **Answering the trust question from outside.** The headless Confirm always says no (`headless.go` L84). The only alternative is `--trust-code`, which skips the check entirely. There is no `trust` command to record a yes.
3. **Loading an extra addon.** `slidev.Config` has no NodeArgs or Env, and `Args` is fixed (section 5).
4. **Saving the whole folder.** All saving assumes a single entry file (see below).
5. **A portable stop signal.** On Windows, a console-less child can't be sent Ctrl-C or SIGTERM by a Rust parent. Today `--headless` stops only on SIGINT/SIGTERM (`headless.go` L50-63). The sidecar should stop when stdin reaches EOF.
6. **Process lifetime is part of the safety model.** `Claim` records `os.Getpid()`, `Alive` needs that PID plus an HTTP answer, and `CleanStale` deletes any work folder older than 10 min without a live owner. So **unpacking, serving and saving must happen in one long-lived process**. If a short-lived `unpack` process exits, the next `stagepack open` of any talk deletes its work folder 10 min later. A dead PID also lets the installed viewer open the same .sptx in parallel and race the write-backs.
7. **Autosave semantics.** The watcher rewrites the .sptx 30 s after every change to slides.md. Whether the editor wants that, or an explicit save with the journal kept only for crash recovery, is a product decision; today there is no switch.

### Proposed commands (smallest set)

**A. `stagepack serve --json [--home DIR] [--trust-code] [--addon DIR]... FILE`** (long-lived; the core of the sidecar). It is essentially `app.Open` with a JSON stdin/stdout UI, no browser, and a folder save.

- stdout JSON lines:
  - `{"event":"unpacked","id","title","entry","workDir","code":[...],"trusted":bool}`
  - if code and not trusted: `{"event":"confirm","question":...}`
  - `{"event":"ready","port","baseUrl","workDir"}`
  - `{"event":"saved","updated","hash"}`
  - `{"event":"conflict"}`
  - `{"event":"error","message","log"}`
  - `{"event":"stopped"}`
- stdin JSON lines: `{"answer":"yes"|"no"}`, `{"cmd":"save"}`, `{"cmd":"save-as","path":...}`, `{"cmd":"stop"}`. **EOF on stdin = stop.**
- Exit codes like export-pptx: 0, 1, 2, 3 not trusted, 4 busy (open in the viewer: report `existing.BaseURL` rather than open a browser).
- Reuses:
  - `workspace.CleanStale`, `session.inUse` logic (move it out of app), `instance.Claim/Update/Renew/Release`, `workspace.Extract`
  - `app.CodeCheck.Allow`, with Ask = emit `confirm` and read the stdin answer; `Remember` stays as is
  - `save.JournalKey`, `save.Recover`, `slidev.FreePort`, `slidev.Start` (extended), `slidev.LogTail`, `workspace.Remove`
  - for save-as: the `saveCopy` claim logic (Claim the destination first)
- Code changes:
  - Add a `Ready(port int, baseURL, workDir string)` hook to `ui.UI`, or an optional interface.
  - `Options.Browser` = no-op.
  - Add `slidev.Config.NodeArgs []string` and `Config.Env []string`, so `Args` can put `--import <preload>` before the CLI path.
  - Retry `FreePort`+`Start` once or twice on the strictPort race.
  - Make watcher mode selectable (off, journal-only, autosave).

**B. Whole-folder save, as `serve`'s `save` command** (preferably not a separate process; see above). A separate `stagepack save --work DIR FILE` would race the serving process's watcher writes on the same file. New function, roughly `workspace.SaveFolder(w *Work, deckPath string, now time.Time) error`:

1. **Walk the work folder.** Same rules as `pack.collectProject` (`internal/pack/pack.go` L253, unexported: move it to a shared package): skip `node_modules/` and `.git` anywhere, and a top-level `dist/` and `.slidev/`. `.slidev/` matters because Slidev writes `snapshots.json` and persisted drawings there (`RT/@slidev/cli/dist/serve-DzvJFv6v.mjs` L2960-2990), and `node_modules/.slidev` is Slidev's generated code. Exception: vendored packages that were in the original `Entries` map back from `node_modules/x` to `vendor/node_modules/x` (the inverse of `deck.ExtractedPath`). New packages are never added; vendoring is pack's job. Never write editor files (launcher, preload, generated config) into the work folder, so none can be swept up.
2. **Check names.** Run `deck.CheckEntryNames(manifest + names)` and report bad names, so the editor can rename them. For example, a pasted "Screenshot 10:00.png" fails on the `:`.
3. **Check for conflicts.** Re-read the deck's manifest. If `Updated != w.Manifest.Updated`, someone else wrote it (another copy, or the viewer's watcher): return a conflict and touch nothing.
4. **Reuse unchanged entries.** Keep a per-entry sha256 in `Work`. Extract already computes it in `write` (extract.go L171-194) but keeps it only for Hash. `zw.Copy(f)` unchanged entries raw from the old zip (fast, byte-identical, important for big videos). Write changed and new entries with `deck.EntryMethod` (entries.go L168). Deleted files simply drop out (Rebuild would fail on them).
5. **Write the manifest and the file.** Write the manifest with `withUpdated` (writeback.go L164, unexported: export it). Use the temp file and rename from `RewriteEntry` (`tmpPattern`, chmod, Sync, `replaceFile`, `syncDir`, all unexported in `internal/save`). Each save still rewrites the whole zip: about a second per few hundred MB on SSD, far slower on USB sticks.
6. **Refresh state after the save.**
   - Update `w.Entries`, the per-entry sums, `w.Manifest.Updated`, and recompute `w.Code` and `w.Hash` (`deck.CodeReasons`, then `ReadHeadmatter`, `HeadmatterReasons`, `AddCode`).
   - **Carry trust forward:** if the version before was trusted, or allowed in this session, `Remember` the new hash. Otherwise the next open (viewer or editor) asks again, because any new public/ file changes Hash.
   - Clear the journal.
7. **Crash recovery.** The journal holds one file (`Entry.Slides`), so it can't hold new images. Options: journal a manifest of changed files, or mark the work folder dirty and teach `CleanStale` (and `inUse`) to keep dirty folders.

**C. `stagepack inspect --json FILE`** (optional, one-shot). Prints manifest, `code` and `trusted` without starting anything, for a file list or a trust badge before opening. It needs Extract split into a "plan + hash" pass that streams entries through sha256 without writing them, plus `ReadHeadmatter` on a temp copy of the entry, plus `CodeCheck` split into `Assess` (reasons, trusted) and `Allow`. **`stagepack trust FILE`** is only needed if trust is decided outside `serve`. It must recompute the hash itself, never take one from the caller.

### Places that assume one entry file

- `save.RewriteEntry`: one existing entry; it can't add or remove entries.
- `save.Watcher` / `WatcherConfig`: one `SlidesPath`/`EntryName`.
- `save.Entry.Slides`: one file's bytes; `save.Recover` writes one file.
- `workspace.Rebuild`: exactly `w.Entries`, so no new files, and an error on a missing one.
- `Work.Hash` leaves out only the entry, so the version (and trust) changes with any other file.
- **An existing gap even for the viewer:** Slidev saves note and content edits to the `.md` file a slide came from (`serve-DzvJFv6v.mjs` L1554-1597: `parser.save(data.markdownFiles[slide.source.filepath])`). Edits to slides imported with `src:` go into the imported file in the work folder. The watcher never sees that file, so those edits are lost when the talk is stopped.

---

## 4. How addons resolve for a talk today

- `slidev-addon-stagepack` is **copied** into `<rt>/node_modules/slidev-addon-stagepack` by `build/addon.sh` (L20-24, called from `build/runtime.sh` L28 and from the build-linux/build-windows targets). It is not in `runtime/package.json`.
- Talks resolve it by **plain parent lookup**. The work folder is `<rt>/work/<random>/`, inside the runtime folder. Slidev's `findPkgRoot(dep, parent)` (`resolver-BIhhHpe7.mjs` L168) uses vitefu's `findDepPkgJsonPath`, which walks up `<dir>/node_modules/<dep>/package.json` from the work folder (`RT/vitefu/src/index.js` L240-252). It finds `<rt>/node_modules/slidev-addon-stagepack`.
  - No NODE_PATH, symlink, `--root`, `--theme` or generated config is used (README.md L433-441).
  - `runtimes/53/pnpm-workspace.yaml` (`packages: []`) makes `<rt>` Slidev's `userWorkspaceRoot` (`searchForWorkspaceRoot` L263). That puts `<rt>` into Vite's `server.fs.allow` (`serve-DzvJFv6v.mjs` L591-598, which also adds every addon and theme root), so runtime files such as fonts are served with 200.
- **Surprise, global mode:** Stagepack runs `node <rt>/node_modules/@slidev/cli/bin/slidev.mjs` with cwd in `<rt>/work/<id>`. `findInvocationNodeModulesPath(argv[1])` gives `<rt>/node_modules` (L39-44), which is outside userRoot, so `isInstalledGlobally.value = true` (L275). Effects:
  - Vite's dependency cache is `<rt>/node_modules/@slidev/cli/node_modules/.vite`, **shared by all open talks and inside the runtime folder** (`cacheDir`, serve L617; seen on disk at `RT/@slidev/cli/node_modules/.vite/deps`).
  - Unresolved packages are also looked up in `findGlobalPkgRoot` (L174-188): the runtime's node_modules, **every sibling folder `runtimes/*/node_modules`** (`computeInvocationSearchPaths` L74-95), and the yarn and npm global package folders.
  - A second runtime placed as a sibling (`runtimes/53-studio`) would be visible to the stock one as a fallback.
- **Trust allowlist:** `bundledAddons = map[string]bool{brand.AddonPkg: true, brand.AddonName: true}` (`internal/deck/code.go` L167), i.e. `slidev-addon-stagepack` and `stagepack` (`internal/brand/brand.go` L17-18).
  - It is used by `isBundledAddon` (L171) in `readPackageJSON` (for `package.json` `slidev.addons`), in `HeadmatterReasons` (for headmatter addons) and in `SlidesReasons` (pack's rough check).
  - A bundled package whose folded name contains "stagepack" always counts as code (`shadowsAddon` L180), so a talk can't swap in its own.
  - `pack` never vendors `@slidev/cli`, `slidev-addon-stagepack`, `stagepack`, `playwright-chromium` or `playwright` (`skipPackages`, pack.go L53), nor anything in the embedded lock (`vendor`, `internal/pack/vendor.go` L62-64).

---

## 5. Slidev 53: how the addon list is resolved

`resolveOptions(entryOptions, mode)` (`RT/@slidev/cli/dist/serve-DzvJFv6v.mjs` L1018-1081):

```js
const entry = await resolveEntry(entryOptions.entry);          // slash(resolve(entryRaw))
const rootsInfo = await getRoots(entry);                        // cached after the first call
const initialLoaded = await parser.load({ userRoot, roots: [userRoot], allowedRoots: ... }, entry, void 0, mode);
let themeRaw = entryOptions.theme || initialLoaded.headmatter.theme;   // --theme CLI flag wins
...
const initialConfig = parser.resolveConfig(initialLoaded.headmatter, themeMeta, entryOptions.entry);
const addonRoots = await resolveAddons(initialConfig.addons);
const roots = uniq([...themeRoots, ...addonRoots, rootsInfo.userRoot]);
```

The resolving function (serve L728-743):

```js
async function resolveAddons(addonsInConfig) {
	const { userRoot, userPkgJson } = await getRoots();
	const resolved = [];
	const resolveAddonNameAndRoot = createResolver("addon", {});
	async function resolveAddon(name, parent) {
		const [, pkgRoot] = await resolveAddonNameAndRoot(name, parent);
		if (!pkgRoot) return;
		resolved.push(pkgRoot);
		const { slidev = {}, engines = {} } = JSON.parse(await fs$1.readFile(resolve(pkgRoot, "package.json"), "utf-8"));
		if (engines.slidev && !satisfies(version, engines.slidev, { includePrerelease: true })) throw new Error(...);
		if (Array.isArray(slidev.addons)) await Promise.all(slidev.addons.map((addon) => resolveAddon(addon, pkgRoot)));
	}
	if (Array.isArray(addonsInConfig)) await Promise.all(addonsInConfig.map((addon) => resolveAddon(addon, userRoot)));
	if (Array.isArray(userPkgJson.slidev?.addons)) await Promise.all(userPkgJson.slidev.addons.map((addon) => resolveAddon(addon, userRoot)));
	return resolved;
}
```

Sources, in effect:

1. `config.addons` from `@slidev/parser` `resolveConfig` (`RT/@slidev/parser/dist/core.mjs` L64-113): `{...getDefaultConfig() /* addons: [] */, ...themeMeta.defaults, ...headmatter.config, ...headmatter}`. So it is the headmatter `addons:` if set, else `config: {addons}` in the headmatter, else the **theme's package.json `slidev.defaults.addons`**, else `[]`. These replace each other; they are not merged. The headmatter is the first slide's frontmatter of the entry file (`RT/@slidev/parser/dist/fs.mjs` L128).
2. The **closest package.json to userRoot**, `slidev.addons`, is added to that list (`getRoots`: `findClosestPkgJsonPath(userRoot)`, then `getUserPkgJson`, `resolver-BIhhHpe7.mjs` L247-251 and L277-278). It is read once and cached in `rootsInfo`.
3. Recursively, each resolved addon's own package.json `slidev.addons`, resolved from that addon's folder. A theme's own `slidev.addons` is **not** followed; only its `slidev.defaults.addons` is, via (1).
4. **Nothing else:** no env var (the only env read is `EDITOR`), no `--addons` CLI flag (only `--theme`/`-t`, `commonOptions` cli.mjs L467-477), and no `slidev.config.*` file. `vite.config.*` and `setup/vite-plugins.ts` are loaded from `roots`, so they apply only to code that is already an addon or theme root (`resolveViteConfigs` serve L3443-3462).

Name resolution, `createResolver` (`resolver-BIhhHpe7.mjs` L211-246):

- `"none"` is skipped.
- `"/abs"` is used as is.
- `"@/x"` means `userRoot/x`.
- `"./x"` or `"a/b"` resolves against `dirname(importer)`. Importer is userRoot (a folder) for addons, so **`./x` resolves next to the talk folder, not inside it**: `<rt>/work/x`.
- Otherwise it must be a safe npm name: it tries `@slidev/addon-<n>`, `slidev-addon-<n>`, `<n>` via `findPkgRoot`, then prompts to install. Without a TTY it exits 1.
- On Windows, an absolute path needs forward slashes (`C:/...`): it then goes down the `includes("/")` branch, which resolves it as an absolute path.

### Ways to load an extra addon without touching the talk's files

1. **Node `--import` preload that seeds Slidev's roots cache (recommended).**
   - Run `node --import <preload.mjs> <rt>/.../slidev.mjs ./slides.md --port N ...` with `STUDIO_ADDON=<abs path>` in env. The preload loads `@slidev/cli/dist/resolver-*.mjs` (the package exports `"./*"`, so deep imports are allowed) and finds the exported function whose `.name === 'getRoots'` (exported as `n`). It calls it with `slash(resolve(process.argv[2]))`, the same value `resolveEntry` computes, and replaces `rootsInfo.userPkgJson` with a copy whose `slidev.addons` has the absolute Studio path appended.
   - `getRoots` returns the cached object on every later call (L272), and `userPkgJson` is used only by `resolveAddons` (serve L729/L741). Slidev therefore adds Studio to `addonRoots` and `roots`: its components, layouts, `setup/*`, vite plugins and `fs.allow` all work. Server restarts on theme or config changes keep it (cache), and `process.argv[1]` stays the real slidev.mjs, so global-mode detection is unchanged.
   - No talk file, no Slidev patch, no reimplemented CLI. With an absolute path, a talk that bundles a fake `slidev-addon-studio` can't shadow it.
   - Go side: add `NodeArgs` and `Env` to `slidev.Config` (pass `--import` as an argument, not in NODE_OPTIONS, which `childEnv` rewrites).
   - Cost if wrong: it relies on an internal, hashed chunk and on `getRoots` caching. A Slidev bump can break it. Make the preload throw when it can't find `getRoots`, and have the sidecar check an endpoint Studio serves (e.g. `/__studio/ping`) after readiness, so a failure is loud, not a silently missing editor. Switching to option 2 or 3 stays inside the runtime/launcher layer, with no change to the file format or to saved talks: roughly a day plus a test. The runtime is pinned, so this can only break at a deliberate runtime bump, which an integration test (Studio root present) catches.
2. **Patch `@slidev/cli` in the editor's runtime.** For example, `resolveAddons` also reads `process.env.SLIDEV_EXTRA_ADDONS`, applied after `npm ci` in the runtime build. Robust in behaviour and about 2 lines, but the patch must be maintained per Slidev release, the runtime's node_modules is no longer stock npm output, and npm has no native patch support (Stagepack uses npm ci, not pnpm `patchedDependencies`).
3. **Programmatic server through the exported API** (`@slidev/cli` index exports `resolveOptions`, `createServer`, `createDataUtils`, `parser`, `ViteSlidevPlugin`). Steps: `resolveOptions`, push the root into `addonRoots` and into `roots` before userRoot, recompute `utils` with `createDataUtils`, then `createServer(options, viteConfig, {loadData})`. It uses public API only, but it must reimplement the CLI's dev loop: port, `strictPort`, restart on theme, config or feature change (cli.mjs L100-170), and `setupPreparser()` (which needs the unexported `loadSetups`). It also misses the second `parser.load` with addon roots that `resolveOptions` performs (preparser extensions and allowedRoots from the addon). That means more code and more drift.
4. **Change the work copy:** add the addon to the work folder's `package.json` or headmatter and strip it on save. Rejected. The watcher and Slidev write slides.md back (a headmatter injection would land in the .sptx), `Rebuild` and Save-a-copy pack the work folder's package.json as is, and a crash or a save bug leaves a trace in the talk.
5. **Wrapper entry with `src:`** (a hidden `.studio.md` in the work folder whose first slide is `src: ./slides.md` plus `addons:`). Rejected. The deck's headmatter is the wrapper's first-slide frontmatter (fs.mjs L128), so the talk's own headmatter (theme, fonts, title, config) would have to be copied in and kept in sync on every edit. Headmatter edits go to slides.md while the config comes from the wrapper. The `entry` (and so `.slidev/` and the addon's `options.entry`) becomes the wrapper. A wrapper outside the talk folder moves userRoot, which breaks the talk's components/, setup/, public/ and styles.
6. **Generated package.json outside the talk.** Doesn't work: Slidev uses the *closest* package.json to userRoot, and every .sptx has its own, since pack requires one (pack.go L93-96).
7. **Theme wrapper or `--theme`.** Rejected. `--theme` replaces the talk's theme, Slidev themes can't extend other themes, and theme `defaults.addons` lose to any `addons:` in the headmatter.
8. **Vite plugin only.** An addon is more than a Vite plugin (Slidev discovers components, layouts, global layers and `setup/*` from roots), and the only ways to inject a plugin without roots are a vite.config in the talk or option 3.

**Pick: option 1** (preload plus absolute path, Studio installed into the editor runtime's node_modules so its own dependencies resolve). It is the only option that needs no file in the talk, no Slidev patch and no copy of the CLI. Its failure mode is loud and contained, and the fallbacks (2, then 3) swap in without touching anything users have saved.

---

## 6. How the runtime folder is built

**Inputs** (`runtime/`):
- `package.json`: exact pins `@slidev/cli 53.0.0`, `@slidev/theme-default 0.25.0`, `@fontsource-variable/inter` and `/jetbrains-mono 5.3.0`, `playwright-core 1.64.1`, `slidev-pptx-export 0.5.0`.
- `package-lock.json`: lockfileVersion 3, 732 package entries (README: 731 packages).
- `pnpm-workspace.yaml` (`packages: []`, the Vite workspace-root marker).
- An optional `runtime/vendor/*.tgz`, named in the lock as `file:vendor/...` (none at present; the mechanism is tested in `runtime/runtime_test.go` L199).

**Go side:**
- `runtime/runtime.go` embeds the lock (`Lock()`, `SlidevVersion()`, `Major()`).
- `Lock()` feeds pack's vendoring skip (vendor.go L63-64), `runtimeShadows` (code.go L218-232), `bundledThemes` (L326), `deck.Check` and `runtimes/<Major>`.

**`build/runtime.sh {linux|win32}`:**
1. Targets `dist/<linux|windows>/runtimes/53`; `major=53` is hard-coded, and tests check it agrees with `Major()` (`TestRuntimeScriptMajor`, `TestPayloadScriptMajor`, `TestAddonAndReleaseScriptsAgree`).
2. `rm -rf node_modules vendor .runtime-stamp` (it keeps `work/`).
3. Copies the 3 inputs, plus `vendor/` if present.
4. `npm ci --no-audit --no-fund` (win32: `--os=win32 --cpu=x64 --omit=dev` for rolldown, oxc and lightningcss natives).
5. `rm -rf vendor`.
6. `node build/licenses.mjs <dir>`: writes THIRD_PARTY_LICENSES.txt and **fails on any licence not in `ALLOWED`** (permissive only).
7. `build/addon.sh <os>`.
8. `build/runtime-stamp.sh > .runtime-stamp`.

**`build/runtime-stamp.sh`:** the stamp is sha256 over `sha256sum` of package.json, package-lock.json, pnpm-workspace.yaml and every vendor file, sorted. `check <os>` exits 0 when `dist/<os>/runtimes/53/.runtime-stamp` matches.

**Makefile:**
- `runtime` (L69-70, `OS=linux|win32`).
- Phony `linux-runtime` / `win-runtime` (L124-132): `runtime-stamp.sh check || make runtime`.
- `build-linux` (L136-140): node, runtime, `addon.sh`, `programs` (ffmpeg), `build/programs.sh`, `build`.
- `build-windows` (L143-146): adds `setup` and the payload.
- `release` (L157-163) builds both, and `build/release.sh` refuses a runtime whose stamp isn't that of `runtime/`.

### Producing a second runtime (Studio fork, shapes addon, playwright-chromium)

1. **Inputs.** Add a second input folder, e.g. `runtime-studio/`, with package.json = the stock deps plus `slidev-addon-studio` (the fork as `file:vendor/slidev-addon-studio-<v>.tgz`; the vendor/tarball path already exists), the shapes addon, and anything else needed. Generate its lock *from* the stock lock (copy it, then `npm install --package-lock-only`, online) so shared packages keep identical versions. Add a test asserting that every stock lock entry has the same version in the studio lock, mirroring `TestLockPinsSlidev`.
2. **Scripts.** Parameterise `runtime.sh`, `runtime-stamp.sh` and `addon.sh` by input folder and output folder instead of the hard-coded `runtime/` and `runtimes/53`. Add Makefile targets (e.g. `RUNTIME=studio`). `licenses.mjs` must accept the fork's licence.
3. **Placement.** Put the editor runtime in the editor's own Stagepack home (`--home`), as `<editorHome>/runtimes/53`. **Not** as a sibling `runtimes/53-studio` of the viewer's runtime: Slidev's global-mode lookup searches sibling `runtimes/*/node_modules`, and the app always uses `runtimes/<Major>`, so the same major collides. A separate home also means separate `trusted.json`, `journal/` and `instances/`. The viewer and the editor won't see each other's claims (both can open one .sptx and race write-backs) or each other's trust records. Decide this explicitly, or add a `--runtime DIR` flag so the editor shares the viewer's home for instances, trust and journal but runs its own runtime.
4. **Keep the stock lock for pack and trust.** The editor's Stagepack binary should still embed the **stock** lock. If it embedded the studio lock, `pack` would skip vendoring the shapes addon (it is "in the runtime"), and the saved .sptx would not open in a stock viewer: Slidev can't prompt without a TTY, so it exits 1 with "addon not found". A talk that names the shapes addon also counts as code in the viewer (`HeadmatterReasons`). If shapes should be trust-free and open anywhere, add it to the stock runtime and to `bundledAddons` in Stagepack itself, rather than only to the editor runtime.
5. **playwright-chromium.** Its postinstall downloads a Chromium (more than 100 MB) during `npm ci`, and it is in `talkLoads`/`skipPackages` for a reason. Prefer the `playwright-core` already in the runtime with the system Chrome or Edge from `browser.FindAppBrowser`, as `export-pptx` does (`internal/export/run.go` `Command`, L69-81). If it must ship, set `PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD` and supply the browser deliberately.
6. **Size.** About 587 MB for the stock node_modules. A superset runtime is not deduplicated against the viewer's, so ship only the superset in the editor bundle.

---

## Surprising or blocking

- **Blocking:** `open --headless` can't be a sidecar. It reports no port or URL, opens a browser anyway, always answers no to the trust question, and stops only on signals (hard to send on Windows). A new long-lived command is needed (section 3A).
- **Blocking:** saving is single-entry throughout (`RewriteEntry`, the watcher, the journal, `Rebuild`). A whole-folder save (new images, deleted files, the vendored-package mapping, conflict check, trust carry-forward) is new code.
- **Lifetime:** unpack, serve and save must be one process. `CleanStale` (10 min, PID + HTTP liveness) deletes work folders of exited processes, and a dead PID frees the deck for a parallel viewer.
- **Existing gap:** edits to `src:`-imported slides are written by Slidev into the imported .md in the work folder, and are lost on stop.
- **Global mode:** Slidev runs in "installed globally" mode under Stagepack. The Vite dependency cache is shared in `<rt>/node_modules/@slidev/cli/node_modules/.vite`, and unresolved packages fall back to sibling `runtimes/*` and the global npm and yarn folders.
- **Relative addons:** `addons: ['./x']` resolves next to the work folder (`<rt>/work/x`), not inside the talk.
- **Trust after edits:** trust is per hash, and any non-entry file change re-asks. The editor must re-`Remember` after its own saves, and its trust list is separate from the viewer's if it uses its own home.
