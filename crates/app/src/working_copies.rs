//! A window's working copies of its talk's unsaved changes, with the files added to it that they
//! use: keeping one a moment after the edits stop, clearing it once the changes are saved or let go, and the copy from last time
//! that's offered back. The changes to the copies are made on background tasks, one after
//! another in the order asked, and a window that closes waits for them with [`settled`].
//!
//! [`settled`]: WorkingCopies::settled

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::document::{Document, TalkPlace};
use crate::recovery::{Found, Recovery, hash_text};

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
    place: PathBuf,
    text: String,
    on_disk: String,
    /// The files added to the talk that the text uses, by their path in it.
    files: Vec<String>,
}

/// The copy from last time that was brought back, until this window's own copy or a save
/// holds what it did. The jobs take it when they run, not when they're asked for.
type Adopted = Arc<Mutex<Option<Found>>>;

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
        on_disk: String,
        text: String,
        /// The files added to the talk that the text uses and no save has written.
        files: Vec<(String, Arc<[u8]>)>,
        adopted: Adopted,
    },
    /// Removes this window's copy, and the adopted one too if `adopted` is given.
    Clear {
        recovery: Recovery,
        place: TalkPlace,
        adopted: Option<Adopted>,
    },
    /// Keeps a file added to the talk, as it's added. Made only if no clear has been asked for
    /// since it was: a save has written it then, or the text doesn't use it yet, and the next
    /// keep of a text that does carries it.
    KeepFile {
        recovery: Recovery,
        place: TalkPlace,
        path: String,
        bytes: Arc<[u8]>,
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
                on_disk,
                text,
                files,
                adopted,
            } => {
                if clears.load(Ordering::SeqCst) == at {
                    recovery
                        .keep_with_files(&place, &hash_text(&on_disk), &text, &files)
                        .map_err(|_| ())?;
                }
                // Kept, or overtaken by a clear: either way it's no longer needed.
                if let Some(adopted) = take_adopted(&adopted) {
                    recovery.discard(&adopted);
                }
            }
            Job::Clear {
                recovery,
                place,
                adopted,
            } => {
                recovery.clear(&place);
                if let Some(adopted) = adopted.as_ref().and_then(take_adopted) {
                    recovery.discard(&adopted);
                }
            }
            Job::KeepFile {
                recovery,
                place,
                path,
                bytes,
            } => {
                if clears.load(Ordering::SeqCst) == at {
                    recovery.keep_file(&place, &path, &bytes).map_err(|_| ())?;
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
        let Some(document) = &self.document else {
            return;
        };
        let place = document.read(cx).place().clone();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { recovery.find(&place) })
                .await;
            this.update(cx, |this, cx| this.offer(found, cx)).ok();
        })
        .detach();
    }

    fn offer(&mut self, found: Option<Found>, cx: &mut Context<Self>) {
        let (Some(found), Some(document), Some(recovery)) =
            (found, &self.document, self.recovery.clone())
        else {
            return;
        };
        let document = document.read(cx);
        let files = document.export_input().files;
        let has_them = found.files.iter().all(|(path, bytes)| files.read(path).is_some_and(|held| *held == **bytes));
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
        let new: Vec<(String, Arc<[u8]>)> =
            document.unsaved_files().into_iter().filter(|(path, _)| !self.files_kept.contains(path)).collect();
        for (path, bytes) in new {
            self.files_kept.insert(path.clone());
            let job = Job::KeepFile { recovery: recovery.clone(), place: place.clone(), path, bytes };
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
    /// and none once it hasn't.
    fn job(&mut self, cx: &App) -> Option<Job> {
        let recovery = self.recovery.clone()?;
        let document = self.document.as_ref()?.read(cx);
        let place = document.place().clone();
        if !document.has_unsaved_changes() {
            self.kept.take()?;
            self.files_kept.clear();
            return Some(Job::Clear {
                recovery,
                place,
                adopted: None,
            });
        }
        let (TalkPlace::Folder(path) | TalkPlace::Packed(path)) = &place;
        let files = document.added_files();
        let kept = Kept {
            place: path.clone(),
            text: document.source().to_string(),
            on_disk: document.stamp_on_disk().to_string(),
            files: files.iter().map(|(path, _)| path.clone()).collect(),
        };
        if self.kept.as_ref() == Some(&kept) {
            return None;
        }
        let job = Job::Keep {
            recovery,
            place,
            on_disk: kept.on_disk.clone(),
            text: kept.text.clone(),
            files,
            adopted: self.adopted.clone(),
        };
        self.kept = Some(kept);
        Some(job)
    }

    /// The talk was saved from `place`: what was kept is in the file now, so this window's
    /// copy and the one it brought back go. Anything edited since is kept again.
    pub fn saved(&mut self, place: TalkPlace, cx: &mut Context<Self>) {
        self.clear(place, cx);
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
        self.enqueue(
            Job::Clear {
                recovery,
                place,
                adopted: Some(self.adopted.clone()),
            },
            cx,
        );
    }

    /// Whether this window's copy is there, as last written.
    pub fn holds_a_copy(&self) -> bool {
        self.kept.is_some()
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
