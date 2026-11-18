# Plan D5: decisions made while building

The calls made during plan D5 without asking the owner, and the smaller findings and notes left for later. Each decision says what it costs if it's wrong. They are in the order they were made. The final review triaged every smaller finding, and those it wanted fixed were fixed in its fix wave. `docs/checks/plan-d5.md` has what ran, and says line by line how far the design's first success test now holds.

## Questions for the owner

Each of these changed what a person sees, and each was decided without asking. Say so if one is wrong.

- **Starting the app shows a start screen**, not a folder dialog, and cancelling a dialog no longer quits the app. `stagepack-studio <path>` still opens that talk.
- **Five templates:** *Light*, *Dark*, *Warm* and *Ocean*, each one empty title slide in its colours, and *Tour*, eight slides that show what a slide can hold. A colour template differs by three colours only, as the first release has one theme per talk, set when it's made. Four of the five tiles are a plain panel of colour.
- **A new presentation is called *Untitled presentation*** until it's saved, and a second one *Untitled presentation 2*.
- **Save on an untitled presentation is Save As**, starting in the Documents folder, with the packed file as the first choice.
- **An untitled presentation that hasn't been edited closes without asking.** Once edited, closing asks, and *Save* there opens Save As; the window closes only when the save is done. A window with a save running waits for it before it closes, for every talk.
- **Presentations never saved are listed on the start screen** under *Not saved last time*, newest first, each with *Open* and *Discard*. They're kept for 30 days and then removed without a word, as other working copies are.
- **A folder saved from the app starts with *Always build this folder* on only if the talk holds no component's code at that save.** The design says talks made in the app start with it on. But a component pasted in from someone else's talk before the first save would then be built later with nobody having said yes, so a talk that holds any is treated as an opened folder, and the bar asks. (This came from the automated check of a commit, and the reviews agreed.)
- **A component pasted into a folder that is already always built is built without a further yes.** That is the design's rule for such folders. It wasn't changed, and should be settled before the compiler ships in an installer.
- **Ctrl+N opens a start screen in a new window.** Ctrl+O is *Open talk file…* and Ctrl+Shift+O *Open talk folder…*. From a start screen a talk opens in that window; from a window showing a talk, in a new one.
- **Two copies of the app running at once don't offer each other's open presentations.** Each working copy is held by a lock the system lets go of when the app ends or crashes. On a disk that can't lock files, the copies are still kept, without the lock.
- **The editor's marks are drawn in a colour that stands out from the slide.** On a white slide the selection's outline and the snapping lines are a little darker than they were, as the old grey didn't stand out enough by the usual measure. The words *Click to add text* are darker too.
- **A talk that can't be opened says why in one plain sentence**, on the start screen and when opened by its path. The detail (which file inside it was missing) is no longer shown.
- **The Home tab still doesn't fit at 1,280 pixels**, the width the window opens at, so a person who has just picked a template sees the paragraph tools behind the arrow. Left from D4.

## For the start of plan E

- **A double-click opens a talk by its path, in a new copy of the app each time.** The app isn't single-instance, so the lock is what keeps two copies apart. Check the lock on a real Windows PC before relying on it, or have the installer's launcher hand paths to the copy already running.
- **Working copies over 30 days old are removed at launch**, never-saved presentations among them, with nothing said. With an installer, long gaps between uses are likelier. Decide whether an old never-saved presentation should be shown before it goes.
- **Upgrading over real data should be tried once.** Copies kept by earlier versions have no lock file and read as free.
- **A template with a font or another kind of file** must use a path a working copy can hold, or a crash loses it. A test now fails if one doesn't.
- **Always-build is remembered by a folder's path.** A Documents folder that moves (a synced folder, a reinstall) keeps the old paths.
- **Fonts on the computer are loaded on the window's thread** each time a template is picked or a talk opened. Measure it before shipping.
- **The executable is 115 MB.**
- **Not seen on Windows:** anything in this plan, the tour's golden pictures (compared byte for byte) among it.

## Rulings

- **Setup.** No linked worktree, as for the earlier plans. Cost if wrong: the owner's uncommitted roadmap edit shares the checkout, and it stays unstaged.
- **Setup.** The plan wasn't held for the owner's review before it ran. The owner said to continue and has run every earlier plan this way. Cost if wrong: rework of whatever the owner would have changed. The plan's own rulings are at its top.
- **In the plan.** Not in this plan: changing a talk's colours after it's made, templates from a file, saving a talk as a template, presenting from the current slide. Cost if wrong: each is a later, separate piece.
- **Tasks 1 and 2.** Built by one implementer and reviewed together. The files a new talk may hold are those under its pictures, components and build folders, as the existing writers allow. Cost if wrong: none found; nothing a new talk needs is refused.
- **Task 3.** The rule for a mark's colour lives in the engine, with the words in an empty area. Cost if wrong: none.
- **Task 4.** A first packed save that is written but can't be read back counts as saved and says so, as D3 ruled for a pack saved in place. Cost if wrong: the talk carries on from memory until the next save.
- **Task 4.** As in the questions above: always-build only for a talk holding no component's code.
- **Task 5.** The lock is the system's own advisory lock on a file in the copy's folder, not a file holding a process's number. Cost if wrong: on a disk that can't lock, two copies of the app can offer each other's presentations, as before this plan.
- **Task 5.** A list of unsaved presentations reads at most 1 MiB of each one's text. A longer one is listed as having *that many slides or more*, and comes back whole when opened.
- **Task 6.** `chrono`, already built as part of the window toolkit, is used for the list's dates. Cost if wrong: none; no new package.
- **Task 6.** A copy that can't be read stays in the list with *Discard*; one that's gone or open elsewhere leaves it.
- **Task 7.** Its commits were reviewed by the final reviewer, not on their own. Cost if wrong: one fewer read of two small fixes.
- **Final.** One fix wave took the review's two Important findings, the two the controller had parked, and three small ones. Cost if wrong: none, each is small.

## Smaller findings and notes

- **Found by the reviews and fixed before the end:** two copies of the app could delete or take over each other's open, unsaved presentation. A presentation brought back had, for a moment, no copy on disk at all. Two start screens could bring back the same one. A window closed during its first save could cut the write short.
- **Found by the final review and fixed:** the Tour's picture, deleted, then a first save, then Undo, came back as a line naming a file the talk no longer had. A picture added and taken back within two seconds left an empty folder that later start screens counted as *1 couldn't be read.* A talk opened by its path that couldn't be opened showed file names. Marks on a white slide were near-black.
- **Found by the real-window run and fixed:** the start screen's tiles sat at two heights. On the Tour's chart slide the offer to add text lay under the chart.
- **Not fixed, in what a person sees:** a colour that comes from the talk's theme is called *None* in the form. An icon's form shows its name as written (*location-heart*) and a picture's its file name. Save As says *Packed talk (.sptx)*. The question before discarding doesn't name the presentation. A presentation brought back takes the lowest number free, so it may come back under another number than it had. There's no *Close* or *Quit* in the File menu. With the caret in an empty, centred area, *Click to add text* stays until a letter is typed. A click a few pixels outside an area types nothing and says nothing.
- **Not fixed, small:** tile pictures are drawn at the first window's scale. A start screen left open past midnight says *Today* until its window is next brought forward. Choosing Save As from the menu while an untitled talk's Save As is open drops a close that was waiting for the save; the window stays open and nothing is lost. A new folder that fails to be written removes the folder it made, and would take with it anything another program put there in that moment.
- **Not fixed, in the tests:** the test that a pack and a folder of one talk read alike uses plain names only. The engine's area names are held to the format crate's list by a test, not by construction.
- **The disk:** the build cache grew to 193 GB during the plan and free space fell to 46 GB. Stale test programs and old build leftovers were deleted from `target/debug/deps`. The final reviewer's own copy of the branch was removed when it finished.
