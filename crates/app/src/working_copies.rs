//! A window's working copies of its talk's unsaved changes, with the files added to it that they
//! use: keeping one a moment after the edits stop, clearing it once the changes are saved or let go, and the copy from last time
//! that's offered back. The changes to the copies are made on background tasks, one after
//! another in the order asked, and a window that closes waits for them with [`settled`].
//!
//! [`settled`]: WorkingCopies::settled

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::*;
use slides_components::BuildEntry;
use slides_engine::AddedFile;

use crate::added_files::is_added_path;
use crate::document::{Document, LargeKeep, TalkPlace};
use crate::recovery::{Found, KeepNothing, Keeping, Recovery, hash_text};

/// How long after the last edit the working copy of unsaved changes is brought up to date.
const WORKING_COPY_DELAY: Duration = Duration::from_secs(1);
/// The longest edits that never stop for [`WORKING_COPY_DELAY`] go without being kept, from
/// the first of them.
const WORKING_COPY_MOST_WAIT: Duration = Duration::from_secs(3);
/// How long after a working copy couldn't be written it's tried again.
const KEEP_RETRY_DELAY: Duration = Duration::from_secs(10);

/// What the window says when a working copy couldn't be written.
pub const KEEP_FAILED: &str =
    "Unsaved changes couldn't be kept safe in case the app stops. The app will try again shortly.";

/// A working copy couldn't be written. It's tried again in a while.
pub struct KeepFailed;

/// The working copy last written for the talk, so the same one isn't written again.
#[derive(PartialEq)]
struct Kept {
    place: TalkPlace,
    /// What the talk is called, which an untitled one's copy says.
    name: String,
    text: String,
    on_disk: String,
    /// The files added to the talk that the text uses, by their path in it.
    files: Vec<String>,
    /// The builds entered in the talk's index with them.
    builds: Vec<BuildEntry>,
}

/// The copy from last time that was brought back, until this window's own copy or a save
/// holds what it did. The jobs take it when they run, not when they're asked for.
type Adopted = Arc<Mutex<Option<Found>>>;

/// Puts `found` back as the copy brought back, to be let go of later.
fn give_back(adopted: &Adopted, found: Found) {
    let mut held = adopted.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if held.is_none() {
        *held = Some(found);
    }
}

fn take_adopted(adopted: &Adopted) -> Option<Found> {
    adopted
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
}

/// A change to the working copies, made on a background task, one after another.
enum Job {
    /// Keeps this window's copy, then lets go of the adopted copy, which it now holds. Made
    /// only if no clear has been asked for since it was, as one has then let it all go.
    Keep {
        recovery: Recovery,
        place: TalkPlace,
        /// What the talk is called.
        name: String,
        on_disk: String,
        text: String,
        /// The files added to the talk that the text uses and no save has written, held or, for
        /// a large file, by where it lies.
        files: Vec<(String, AddedFile)>,
        /// The builds entered in its index with them.
        builds: Vec<BuildEntry>,
        adopted: Adopted,
        /// What of the talk's lies on disk, which the adopted copy isn't let go of while any lies
        /// in it.
        keep: Option<LargeKeep>,
    },
    /// Removes this window's copy, and the adopted one too if `adopted` is given, but for the
    /// large files `keep` says the talk still holds, which are the only copies there are. The
    /// copies of the places in `also`, where the talk was kept before, are cleared the same way.
    Clear {
        recovery: Recovery,
        place: TalkPlace,
        adopted: Option<Adopted>,
        keep: Option<LargeKeep>,
        also: Vec<TalkPlace>,
    },
    /// Keeps a file added to the talk, as it's added. Made only if no clear has been asked for
    /// since it was: a save has written it then, or the text doesn't use it yet, and the next
    /// keep of a text that does carries it.
    KeepFile {
        recovery: Recovery,
        place: TalkPlace,
        path: String,
        file: AddedFile,
    },
    /// Removes a copy from last time.
    Discard(Recovery, Found),
}

impl Job {
    /// Makes the change. `at` is how many clears had been asked for when it was asked for,
    /// and `clears` how many have been by now. A keep that fails leaves the adopted copy
    /// where it is, for the next keep or clear.
    fn run(self, clears: &AtomicU64, at: u64) -> Result<(), ()> {
        match self {
            Job::Keep {
                recovery,
                place,
                name,
                on_disk,
                text,
                files,
                builds,
                adopted,
                keep,
            } => {
                if clears.load(Ordering::SeqCst) == at {
                    // A file brought back that still lies in last time's copy is taken into this
                    // window's own first, so that last time's can go once this is kept.
                    if let Some(keep) = &keep {
                        keep.take_in_left_in_last(&recovery, &place);
                    }
                    let kept = match &place {
                        // No file holds anything of it, so there's no text it started from.
                        TalkPlace::Untitled(_) => recovery.keep_from(&place, Some(&name), &hash_text(""), &text, &files, &builds),
                        TalkPlace::Folder(_) | TalkPlace::Packed(_) => {
                            recovery.keep_from(&place, None, &hash_text(&on_disk), &text, &files, &builds)
                        }
                    };
                    kept.map_err(|_| ())?;
                }
                // Kept, or overtaken by a clear: either way it's no longer needed, unless a file the
                // talk holds still lies in it, when it's kept for the next keep or clear.
                let keep: &dyn Keeping = keep.as_ref().map_or(&KeepNothing, |keep| keep);
                if let Some(found) = take_adopted(&adopted)
                    && !recovery.discard_keeping(&found, keep)
                {
                    give_back(&adopted, found);
                }
            }
            Job::Clear {
                recovery,
                place,
                adopted,
                keep,
                also,
            } => {
                // What the talk holds is looked at now, not when this was asked for: a file may
                // have been added since.
                // What the talk holds is asked as each file would go, not when this was asked for.
                let keep: &dyn Keeping = keep.as_ref().map_or(&KeepNothing, |keep| keep);
                for place in also.iter().chain([&place]) {
                    recovery.clear_keeping(place, keep);
                }
                if let Some(adopted) = adopted.as_ref()
                    && let Some(found) = take_adopted(adopted)
                    && !recovery.discard_keeping(&found, keep)
                {
                    give_back(adopted, found);
                }
                // A new presentation's id is its own: any other copy of it that no window holds
                // is an older one, as a crash leaves while one is brought back, which would be
                // offered again; but not one a file the talk holds still lies in.
                if let TalkPlace::Untitled(id) = place {
                    recovery.discard_untitled_keeping(id, keep);
                }
            }
            Job::KeepFile {
                recovery,
                place,
                path,
                file,
            } => {
                if clears.load(Ordering::SeqCst) == at {
                    recovery.keep_added(&place, &path, &file).map_err(|_| ())?;
                }
            }
            Job::Discard(recovery, found) => recovery.discard(&found),
        }
        Ok(())
    }
}

/// The working copies of one window's talk.
pub struct WorkingCopies {
    /// The talk, if the window has one.
    document: Option<Entity<Document>>,
    /// Where working copies of unsaved changes are kept, if anywhere.
    recovery: Option<Recovery>,
    /// The working copy this window last wrote, while it's there.
    kept: Option<Kept>,
    /// The update of the working copy waiting for the edits to stop for a moment.
    pending: Option<Task<()>>,
    /// When the first edit the working copy doesn't hold yet was made, while an update waits.
    unkept_since: Option<Instant>,
    /// Unsaved changes from last time, while nobody has said whether to bring them back.
    offered: Option<Found>,
    /// The copy from last time that was brought back.
    adopted: Adopted,
    /// The last change to the working copies, which the next one waits for.
    chain: Option<Task<()>>,
    /// How many clears of this window's copy have been asked for.
    clears: Arc<AtomicU64>,
    /// Whether the window is closing: nothing more is kept, and it may close whatever is
    /// unsaved.
    closing: bool,
    /// How many files had been added to the talk when this last looked.
    files_seen: u64,
    /// The files added to the talk that this window's copy holds, or is about to.
    files_kept: BTreeSet<String>,
    /// The places the talk was kept in before, whose slots may still hold a large file of the
    /// talk's, cleared again with each clear.
    left_in: Vec<TalkPlace>,
}

impl EventEmitter<KeepFailed> for WorkingCopies {}

impl WorkingCopies {
    /// The working copies of `document`, kept nowhere until [`Self::use_recovery`].
    pub fn new(document: Option<Entity<Document>>) -> WorkingCopies {
        WorkingCopies {
            document,
            recovery: None,
            kept: None,
            pending: None,
            unkept_since: None,
            offered: None,
            adopted: Adopted::default(),
            chain: None,
            clears: Arc::new(AtomicU64::new(0)),
            closing: false,
            files_seen: 0,
            files_kept: BTreeSet::new(),
            left_in: Vec::new(),
        }
    }

    /// Where working copies are kept, if anywhere.
    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }

    /// Keeps working copies of unsaved changes in `recovery`, in a slot of this window's own,
    /// and offers back the newest one left there for this talk, if any.
    pub fn use_recovery(&mut self, recovery: Recovery, cx: &mut Context<Self>) {
        let recovery = recovery.for_another_window();
        self.recovery = Some(recovery.clone());
        let Some(document) = self.document.clone() else {
            return;
        };
        // A large file added is copied into this window's own slot.
        let kept_in = recovery.clone();
        document.update(cx, |document, _| document.keep_large_files_in(kept_in));
        let place = document.read(cx).place().clone();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { recovery.find(&place) })
                .await;
            this.update(cx, |this, cx| this.offer(found, cx)).ok();
        })
        .detach();
    }

    /// Keeps working copies in `recovery` as [`Self::use_recovery`] does, for a talk made from
    /// `found`, a copy from last time: it's this window's own from the start, so it's kept
    /// again here straight away, and last time's goes only once it is. Nothing is offered.
    pub fn use_recovery_bringing_back(&mut self, recovery: Recovery, found: Found, cx: &mut Context<Self>) {
        let recovery = recovery.for_another_window();
        self.recovery = Some(recovery.clone());
        if let Some(document) = self.document.clone() {
            // Its large files are taken into this window's slot first, so they're its own.
            let large = found.large.clone();
            document.update(cx, |document, _| {
                document.keep_large_files_in(recovery);
                document.bring_back_large(large);
            });
        }
        self.brought_back(found, cx);
    }

    fn offer(&mut self, found: Option<Found>, cx: &mut Context<Self>) {
        let (Some(found), Some(document), Some(recovery)) =
            (found, &self.document, self.recovery.clone())
        else {
            return;
        };
        let adopted = self.adopted.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|adopted| adopted.is_same_copy(&found));
        if adopted {
            // Brought back already, and let go only once it's kept here.
            return;
        }
        let document = document.read(cx);
        let files = document.export_input().files;
        let has_them = found.files.iter().all(|(path, bytes)| files.read(path).is_some_and(|held| *held == **bytes));
        // A large file is never read to tell: one that lies where the copy's does, or in a file as
        // long that last changed when it did.
        let has_large = found.large.iter().all(|(path, at)| {
            let Some(lies) = files.lies_at(path) else { return false };
            let looked = |file: &std::path::Path| std::fs::metadata(file).ok().map(|meta| (meta.len(), meta.modified().ok()));
            lies == *at || looked(&lies).is_some_and(|here| Some(here) == looked(at))
        });
        let has_them = has_them && has_large;
        if found.text == document.source() && has_them {
            // Nothing in it that isn't here.
            self.enqueue(Job::Discard(recovery, found), cx);
            return;
        }
        self.offered = Some(found);
        cx.notify();
    }

    /// The unsaved changes from last time, while nobody has said whether to bring them back.
    pub fn offered(&self) -> Option<&Found> {
        self.offered.as_ref()
    }

    /// Takes the unsaved changes from last time to bring them back. They're offered no more,
    /// unless they're given back with [`Self::offer_again`].
    pub fn take_offered(&mut self) -> Option<Found> {
        self.offered.take()
    }

    /// Offers `found` again, as bringing it back was refused.
    pub fn offer_again(&mut self, found: Found, cx: &mut Context<Self>) {
        self.offered = Some(found);
        cx.notify();
    }

    /// The talk took back `found`, the changes from last time. They're kept as this window's
    /// own straight away, and last time's copy goes once they are.
    pub fn brought_back(&mut self, found: Found, cx: &mut Context<Self>) {
        *self
            .adopted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(found);
        self.pending = None;
        self.unkept_since = None;
        if let Some(job) = self.job(cx) {
            self.enqueue(job, cx);
        }
    }

    /// Lets the unsaved changes from last time go. Edits made here since are in a copy of
    /// their own, which stays.
    pub fn discard_offered(&mut self, cx: &mut Context<Self>) {
        let (Some(found), Some(recovery)) = (self.offered.take(), self.recovery.clone()) else {
            return;
        };
        self.enqueue(Job::Discard(recovery, found), cx);
        cx.notify();
    }

    /// The talk changed. Its working copy is brought up to date once the edits have stopped
    /// for a moment, or [`WORKING_COPY_MOST_WAIT`] after the first edit it doesn't hold, if
    /// that's sooner.
    pub fn talk_changed(&mut self, cx: &mut Context<Self>) {
        self.keep_added_files(cx);
        let now = cx.background_executor().now();
        let since = *self.unkept_since.get_or_insert(now);
        let left = WORKING_COPY_MOST_WAIT.saturating_sub(now.saturating_duration_since(since));
        self.keep_after(WORKING_COPY_DELAY.min(left), cx);
    }

    /// Keeps each file added to the talk since this last looked, straight away: a picture that
    /// shows on a slide is kept from the moment it's added, whether or not the text changed.
    fn keep_added_files(&mut self, cx: &mut Context<Self>) {
        let (Some(recovery), Some(document)) = (self.recovery.clone(), self.document.clone()) else {
            return;
        };
        if self.closing {
            return;
        }
        let document = document.read(cx);
        let stamp = document.files_added_stamp();
        if stamp == self.files_seen {
            return;
        }
        self.files_seen = stamp;
        let place = document.place().clone();
        let new: Vec<(String, AddedFile)> =
            document.unsaved_added().into_iter().filter(|(path, _)| !self.files_kept.contains(path)).collect();
        for (path, file) in new {
            self.files_kept.insert(path.clone());
            let job = Job::KeepFile { recovery: recovery.clone(), place: place.clone(), path, file };
            self.enqueue(job, cx);
        }
    }

    fn keep_after(&mut self, delay: Duration, cx: &mut Context<Self>) {
        if self.recovery.is_none() || self.closing {
            return;
        }
        // Replacing the task cancels the one waiting, so the wait starts again from now.
        self.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                this.unkept_since = None;
                if let Some(job) = this.job(cx) {
                    this.enqueue(job, cx);
                }
            })
            .ok();
        }));
    }

    /// Runs `job` on a background task once the changes asked for before it are made, so
    /// they're made in the order asked.
    fn enqueue(&mut self, job: Job, cx: &mut Context<Self>) {
        if matches!(job, Job::Clear { .. }) {
            // A keep still waiting is older than this.
            self.clears.fetch_add(1, Ordering::SeqCst);
        }
        let clears = self.clears.clone();
        let at = clears.load(Ordering::SeqCst);
        let previous = self.chain.take();
        self.chain = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let outcome = cx
                .background_spawn(async move { job.run(&clears, at) })
                .await;
            if outcome.is_err() {
                this.update(cx, |this, cx| this.keep_failed(cx)).ok();
            }
        }));
    }

    /// Says that the working copy couldn't be written, and tries again in a while.
    fn keep_failed(&mut self, cx: &mut Context<Self>) {
        self.kept = None;
        cx.emit(KeepFailed);
        let unsaved = self
            .document
            .as_ref()
            .is_some_and(|document| document.read(cx).has_unsaved_changes());
        if unsaved {
            self.keep_after(KEEP_RETRY_DELAY, cx);
        }
    }

    /// What brings the working copy in line with the talk: one while it has unsaved changes,
    /// and none once it hasn't, nor any file kept as it was added.
    fn job(&mut self, cx: &App) -> Option<Job> {
        let recovery = self.recovery.clone()?;
        let document = self.document.as_ref()?.read(cx);
        let place = document.place().clone();
        if !document.has_unsaved_changes() {
            // A file added and undone before any text was kept has a slot of its own to go too.
            if self.kept.take().is_none() && self.files_kept.is_empty() {
                return None;
            }
            self.files_kept.clear();
            return Some(Job::Clear {
                recovery,
                place,
                adopted: None,
                keep: Some(document.large_keep()),
                also: self.left_in.clone(),
            });
        }
        // What a save would write, an untitled talk's own files among them, but only those a
        // working copy can hold.
        let files: Vec<(String, AddedFile)> =
            document.files_to_write().into_iter().filter(|(path, _)| is_added_path(path)).collect();
        let kept = Kept {
            place: place.clone(),
            name: document.name(),
            text: document.source().to_string(),
            on_disk: document.stamp_on_disk().to_string(),
            files: files.iter().map(|(path, _)| path.clone()).collect(),
            builds: document.builds_to_enter(),
        };
        if self.kept.as_ref() == Some(&kept) {
            return None;
        }
        let job = Job::Keep {
            recovery,
            place,
            name: kept.name.clone(),
            on_disk: kept.on_disk.clone(),
            text: kept.text.clone(),
            files,
            builds: kept.builds.clone(),
            adopted: self.adopted.clone(),
            keep: Some(document.large_keep()),
        };
        self.kept = Some(kept);
        Some(job)
    }

    /// The talk was saved from `place`: what was kept is in the file now, so this window's
    /// copy and the one it brought back go. Anything edited since is kept again.
    pub fn saved(&mut self, place: TalkPlace, cx: &mut Context<Self>) {
        self.clear(place, cx);
        // A file the save didn't write, as the text doesn't use it, is the talk's still, but it
        // isn't kept until a text that uses it is: as a new presentation's own picture is.
        if let Some(document) = &self.document {
            self.files_seen = document.read(cx).files_added_stamp();
        }
    }

    /// The talk's unsaved changes at `place` are let go, so this window's copy and the one it
    /// brought back go.
    pub fn let_go(&mut self, place: TalkPlace, cx: &mut Context<Self>) {
        self.clear(place, cx);
    }

    fn clear(&mut self, place: TalkPlace, cx: &mut Context<Self>) {
        self.kept = None;
        // The slot goes, files and all.
        self.files_kept.clear();
        self.pending = None;
        self.unkept_since = None;
        let Some(recovery) = self.recovery.clone() else {
            return;
        };
        let keep = self.document.as_ref().map(|document| document.read(cx).large_keep());
        let also = self.left_in.iter().filter(|kept| **kept != place).cloned().collect();
        // A large file of the talk's that no save has written stays in this slot, and it may be
        // kept somewhere else from now on, after a Save As: this slot is cleared again later.
        if !self.left_in.contains(&place) {
            self.left_in.push(place.clone());
        }
        self.enqueue(
            Job::Clear {
                recovery,
                place,
                adopted: Some(self.adopted.clone()),
                keep,
                also,
            },
            cx,
        );
    }

    /// The window is closing with nothing unsaved: its copies go, each slot it kept anything in
    /// whole. A video added that the talk doesn't use, as its insert was undone, has stayed in
    /// the slot while the window was open, as Redo may want it; it goes now. If the app stops
    /// instead, the next start sweeps it away with the slot, as it holds no text.
    pub fn let_go_closing(&mut self, cx: &mut Context<Self>) {
        let (Some(recovery), Some(document)) = (self.recovery.clone(), self.document.clone()) else {
            return;
        };
        let document = document.read(cx);
        let lies = document.kept_large_files() || !document.large_keep().held().is_empty();
        if !(self.holds_a_copy() || lies || !self.left_in.is_empty()) {
            return;
        }
        let place = document.place().clone();
        self.kept = None;
        self.files_kept.clear();
        self.pending = None;
        self.unkept_since = None;
        let also = self.left_in.iter().filter(|kept| **kept != place).cloned().collect();
        let job = Job::Clear { recovery, place, adopted: Some(self.adopted.clone()), keep: None, also };
        self.enqueue(job, cx);
    }

    /// Whether this window's copy is there, as last written, or any file added to the talk was
    /// kept in its slot as it was added: either way the slot has to go once nothing is unsaved.
    pub fn holds_a_copy(&self) -> bool {
        self.kept.is_some() || !self.files_kept.is_empty()
    }

    /// Whether any change to the working copies has been asked for, which a window that
    /// closes has to wait for.
    pub fn has_jobs(&self) -> bool {
        self.chain.is_some()
    }

    /// Whether the window is closing, after [`Self::settled`].
    pub fn is_closing(&self) -> bool {
        self.closing
    }

    /// For a window that's closing: nothing more is kept, and the task returned is done once
    /// the changes to the working copies asked for are made, as they'd be lost with it.
    pub fn settled(&mut self) -> Task<()> {
        self.closing = true;
        self.pending = None;
        self.unkept_since = None;
        // Taken, so the close waits for it. A job asked for after this has nothing before it
        // to wait for, but none that matters can be: once closing no keep is started, and a
        // clear or discard that comes anyway removes copies of changes the window is letting
        // go, in whatever order.
        self.chain.take().unwrap_or_else(|| Task::ready(()))
    }
}
