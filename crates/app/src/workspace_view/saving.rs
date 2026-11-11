//! Saving the talk from the window: Save and Save As, the working copy of unsaved changes and
//! bringing it back after a crash, a change to the file from outside, and asking before the
//! window closes with changes unsaved.

use super::*;

const SAVING: Wording = Wording {
    doing: "Saving",
    done: "Saved",
};

/// The two forms Save As offers.
#[derive(Clone, Copy)]
enum SaveKind {
    Folder,
    Packed,
}

/// The working copy last written for the talk, so the same one isn't written again.
#[derive(PartialEq)]
pub(super) struct Kept {
    place: PathBuf,
    text: String,
    on_disk: String,
}

/// A change to the working copies, made on a background task, one after another.
enum WorkingCopyJob {
    /// Keeps this window's copy, then lets go of `adopted`, a copy from last time it now
    /// holds. Made only if no clear has been asked for since it was.
    Keep {
        recovery: Recovery,
        place: TalkPlace,
        on_disk: String,
        text: String,
        adopted: Option<Found>,
    },
    /// Removes this window's copy, and `adopted`.
    Clear {
        recovery: Recovery,
        place: TalkPlace,
        adopted: Option<Found>,
    },
    /// Removes a copy from last time.
    Discard(Recovery, Found),
}

impl WorkingCopyJob {
    /// Makes the change. `at` is how many clears had been asked for when it was asked for,
    /// and `clears` how many have been by now. A keep that fails gives back what it held.
    fn run(self, clears: &AtomicU64, at: u64) -> Result<(), Option<Found>> {
        match self {
            WorkingCopyJob::Keep {
                recovery,
                place,
                on_disk,
                text,
                adopted,
            } => {
                if clears.load(Ordering::SeqCst) != at {
                    return Ok(());
                }
                if recovery.keep(&place, &hash_text(&on_disk), &text).is_err() {
                    return Err(adopted);
                }
                if let Some(adopted) = adopted {
                    recovery.discard(&adopted);
                }
            }
            WorkingCopyJob::Clear {
                recovery,
                place,
                adopted,
            } => {
                recovery.clear(&place);
                if let Some(adopted) = adopted {
                    recovery.discard(&adopted);
                }
            }
            WorkingCopyJob::Discard(recovery, found) => recovery.discard(&found),
        }
        Ok(())
    }
}

impl WorkspaceView {
    /// Keeps working copies of unsaved changes in `recovery`, in a slot of this window's own,
    /// and offers back the newest one left there for this talk, if any.
    pub(super) fn use_recovery(&mut self, recovery: Recovery, cx: &mut Context<Self>) {
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
        if found.text == document.read(cx).source() {
            // Nothing in it that isn't here.
            self.enqueue_working_copy_job(WorkingCopyJob::Discard(recovery, found), cx);
            return;
        }
        self.offered = Some(found);
        cx.notify();
    }

    /// Takes back the unsaved changes from last time, as one edit to undo. They're kept as
    /// this window's own straight away, and last time's copy goes once they are.
    fn bring_back(&mut self, cx: &mut Context<Self>) {
        let (Some(mut found), Some(document)) = (self.offered.take(), self.document.clone()) else {
            return;
        };
        let text = std::mem::take(&mut found.text);
        document.update(cx, |document, cx| {
            document.recover(text);
            // A show of the talk has to know.
            cx.notify();
        });
        self.adopted = Some(found);
        self.pending_working_copy = None;
        if let Some(job) = self.working_copy_job(cx) {
            self.enqueue_working_copy_job(job, cx);
        }
        let source = document.read(cx).source().to_string();
        self.markdown_panel
            .update(cx, |panel, cx| panel.set_source(&source, cx));
        self.show_slide(cx);
    }

    /// Lets the unsaved changes from last time go. Edits made here since are in a copy of
    /// their own, which stays.
    fn discard_recovered(&mut self, cx: &mut Context<Self>) {
        let (Some(found), Some(recovery)) = (self.offered.take(), self.recovery.clone()) else {
            return;
        };
        self.enqueue_working_copy_job(WorkingCopyJob::Discard(recovery, found), cx);
        cx.notify();
    }

    /// Brings the working copy up to date once the edits have stopped for a moment.
    pub(super) fn keep_working_copy_soon(&mut self, cx: &mut Context<Self>) {
        self.keep_working_copy_after(WORKING_COPY_DELAY, cx);
    }

    fn keep_working_copy_after(&mut self, delay: Duration, cx: &mut Context<Self>) {
        if self.recovery.is_none() || self.close_anyway {
            return;
        }
        // Replacing the task cancels the one waiting, so the wait starts again from now.
        self.pending_working_copy = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                if let Some(job) = this.working_copy_job(cx) {
                    this.enqueue_working_copy_job(job, cx);
                }
            })
            .ok();
        }));
    }

    /// Runs `job` on a background task once the changes asked for before it are made, so
    /// they're made in the order asked.
    fn enqueue_working_copy_job(&mut self, job: WorkingCopyJob, cx: &mut Context<Self>) {
        if matches!(job, WorkingCopyJob::Clear { .. }) {
            // A keep still waiting is older than this.
            self.clears.fetch_add(1, Ordering::SeqCst);
        }
        let clears = self.clears.clone();
        let at = clears.load(Ordering::SeqCst);
        let previous = self.working_copy_chain.take();
        self.working_copy_chain = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let outcome = cx
                .background_spawn(async move { job.run(&clears, at) })
                .await;
            if let Err(adopted) = outcome {
                this.update(cx, |this, cx| this.keep_failed(adopted, cx))
                    .ok();
            }
        }));
    }

    /// Says that the working copy couldn't be written, and tries again in a while.
    fn keep_failed(&mut self, adopted: Option<Found>, cx: &mut Context<Self>) {
        self.kept = None;
        if self.adopted.is_none() {
            self.adopted = adopted;
        }
        self.say(KEEP_FAILED, cx);
        let unsaved = self
            .document
            .as_ref()
            .is_some_and(|document| document.read(cx).has_unsaved_changes());
        if unsaved {
            self.keep_working_copy_after(KEEP_RETRY_DELAY, cx);
        }
    }

    /// What brings the working copy in line with the talk: one while it has unsaved changes,
    /// and none once it hasn't.
    fn working_copy_job(&mut self, cx: &App) -> Option<WorkingCopyJob> {
        let recovery = self.recovery.clone()?;
        let document = self.document.as_ref()?.read(cx);
        let place = document.place().clone();
        if !document.has_unsaved_changes() {
            self.kept.take()?;
            return Some(WorkingCopyJob::Clear {
                recovery,
                place,
                adopted: None,
            });
        }
        let (TalkPlace::Folder(path) | TalkPlace::Packed(path)) = &place;
        let kept = Kept {
            place: path.clone(),
            text: document.source().to_string(),
            on_disk: document.stamp_on_disk().to_string(),
        };
        if self.kept.as_ref() == Some(&kept) {
            return None;
        }
        let job = WorkingCopyJob::Keep {
            recovery,
            place,
            on_disk: kept.on_disk.clone(),
            text: kept.text.clone(),
            adopted: self.adopted.take(),
        };
        self.kept = Some(kept);
        Some(job)
    }

    /// Removes this window's working copy of the talk at `place`, and the one it brought
    /// back, as their changes are saved or let go.
    fn clear_working_copies(&mut self, place: TalkPlace, cx: &mut Context<Self>) {
        self.kept = None;
        self.pending_working_copy = None;
        let Some(recovery) = self.recovery.clone() else {
            return;
        };
        let adopted = self.adopted.take();
        self.enqueue_working_copy_job(
            WorkingCopyJob::Clear {
                recovery,
                place,
                adopted,
            },
            cx,
        );
    }

    /// Closes the window once the changes to the working copies asked for are made: they'd
    /// be lost with it.
    fn close_after_working_copies(&mut self, cx: &mut Context<Self>) {
        self.close_anyway = true;
        self.pending_working_copy = None;
        let chain = self.working_copy_chain.take();
        cx.spawn(async move |this, cx| {
            if let Some(chain) = chain {
                chain.await;
            }
            this.update(cx, |this, cx| this.close_window(cx)).ok();
        })
        .detach();
    }

    /// Saves the talk where it's kept, on a background task.
    pub(super) fn save(&mut self, cx: &mut Context<Self>) {
        let Some(document) = &self.document else {
            return;
        };
        if self.saving {
            self.save_again = true;
            return;
        }
        let document = document.read(cx);
        let job = document.save_job();
        let name = place_label(document.place());
        self.run_save(job, name, cx);
    }

    /// Asks which form to save the talk in, then where.
    pub(super) fn ask_to_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_none() || window.has_active_dialog(cx) {
            return;
        }
        let view = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let choose = |kind: SaveKind| {
                let view = view.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    view.update(cx, |view, cx| view.save_as(kind, cx)).ok();
                }
            };
            dialog
                .title("Save as")
                .child("Keep the talk as a folder of files, or pack it into one file to send.")
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new(SAVE_AS_CANCEL_ID)
                                .ghost()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new(SAVE_AS_FOLDER_ID)
                                .label("Talk folder")
                                .on_click(choose(SaveKind::Folder)),
                        )
                        .child(
                            Button::new(SAVE_AS_PACKED_ID)
                                .primary()
                                .label("Packed talk (.sptx)")
                                .on_click(choose(SaveKind::Packed)),
                        ),
                )
        });
    }

    /// Asks where to save the talk as `kind`, then saves it there on a background task. From
    /// then on the talk is kept there.
    fn save_as(&mut self, kind: SaveKind, cx: &mut Context<Self>) {
        let Some(document) = &self.document else {
            return;
        };
        let document = document.read(cx);
        let stem = export::file_stem_for(&document.name());
        let suggested = match kind {
            SaveKind::Folder => stem,
            SaveKind::Packed => format!("{stem}.{PACKED_EXTENSION}"),
        };
        let chosen = cx.prompt_for_new_path(document.place().directory(), Some(&suggested));
        cx.spawn(async move |this, cx| {
            let out = match chosen.await {
                Ok(Ok(Some(out))) => out,
                Ok(Err(error)) => {
                    let problem = format!("Could not show the Save dialog: {error}");
                    this.update(cx, |this, cx| this.say(problem, cx)).ok();
                    return;
                }
                _ => return,
            };
            this.update(cx, |this, cx| {
                let Some(document) = &this.document else {
                    return;
                };
                if this.saving {
                    this.say("The talk is still being saved. Try Save As again in a moment.", cx);
                    return;
                }
                let name = file_label(&out);
                let target = match kind {
                    SaveKind::Folder => SaveTarget::Folder(out),
                    SaveKind::Packed => SaveTarget::Packed(out),
                };
                let job = document.read(cx).save_as_job(target);
                this.run_save(job, name, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Runs `job`, which saves to the place called `name`, on a background task.
    fn run_save(&mut self, job: SaveJob, name: String, cx: &mut Context<Self>) {
        self.saving = true;
        self.say(format!("{} {name}…", SAVING.doing), cx);
        cx.spawn(async move |this, cx| {
            let outcome = cx.background_spawn(async move { job.run() }).await;
            this.update(cx, |this, cx| this.finish_save(outcome, name, cx))
                .ok();
        })
        .detach();
    }

    fn finish_save(
        &mut self,
        outcome: Result<Saved, SaveError>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.saving = false;
        let Some(document) = self.document.clone() else {
            return;
        };
        match outcome {
            Ok(saved) => {
                let said = if saved.notices().is_empty() {
                    format!("{} {name}.", SAVING.done)
                } else {
                    format!("{} {name}. {}", SAVING.done, saved.notices().join(" · "))
                };
                let moved = saved.moved();
                let saved_from = document.read(cx).place().clone();
                document.update(cx, |document, cx| {
                    document.saved(saved);
                    cx.notify();
                });
                // What it kept is in the file now. Anything edited since is kept again.
                self.clear_working_copies(saved_from, cx);
                if moved {
                    self.moved(cx);
                }
                self.say(said, cx);
                if self.close_after_save {
                    if !document.read(cx).has_unsaved_changes() {
                        self.close_after_save = false;
                        self.close_after_working_copies(cx);
                        return;
                    }
                    // Edited while it saved: those are saved too before the window goes.
                    self.save_again = true;
                }
            }
            Err(SaveError::ChangedOutside(theirs)) => {
                self.close_after_save = false;
                self.save_again = false;
                self.say(SaveError::ChangedOutside(String::new()).to_string(), cx);
                // Asked about as any change from outside is: nothing has been written.
                self.reload(Some(theirs), cx);
            }
            Err(problem) => {
                self.close_after_save = false;
                self.say(problem.to_string(), cx);
            }
        }
        if std::mem::take(&mut self.reload_after_save)
            && let Some(folder) = &self.talk_folder
        {
            let file = folder.join(SLIDES_FILE);
            self.reload_soon(file, cx);
        }
        if std::mem::take(&mut self.save_again) && document.read(cx).has_unsaved_changes() {
            self.save(cx);
        }
    }

    /// Follows the talk to where Save As put it: its folder is the one watched and built.
    fn moved(&mut self, cx: &mut Context<Self>) {
        let Some(document) = &self.document else {
            return;
        };
        self.talk_folder = match document.read(cx).place() {
            TalkPlace::Folder(folder) => Some(folder.clone()),
            TalkPlace::Packed(_) => None,
        };
        self.always_build = self.is_always_built();
        self.build_problems.clear();
        self.pending_reload = None;
        self.pending_components = None;
        let watched = self.watcher.take().is_some();
        let tried_to_watch = self.watch_problem.take().is_some();
        let followed = watched || tried_to_watch;
        if followed && let Some(folder) = self.talk_folder.clone() {
            self.follow_saves(folder, cx);
        }
        self.show_slide(cx);
    }

    /// Keeps the edits made here over the file's change.
    fn keep_mine(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            document.update(cx, |document, _| document.keep_mine());
            // The edits now start from what the file holds.
            self.keep_working_copy_soon(cx);
            cx.notify();
        }
    }

    /// Loads the file as it was changed, in place of the edits made here.
    fn load_theirs(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let Some(theirs) = document.read(cx).outside_change().map(str::to_string) else {
            return;
        };
        document.update(cx, |document, cx| {
            document.take_theirs(theirs);
            // A show of the talk has to know.
            cx.notify();
        });
        let source = document.read(cx).source().to_string();
        self.markdown_panel
            .update(cx, |panel, cx| panel.set_source(&source, cx));
        self.show_slide(cx);
    }

    /// Whether the window can close now. With unsaved changes it asks first, and closes
    /// itself once that's answered.
    pub(super) fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let unsaved = self
            .document
            .as_ref()
            .is_some_and(|document| document.read(cx).has_unsaved_changes());
        if self.close_anyway {
            return true;
        }
        if !unsaved {
            // Back to what's saved within the wait: the copy still there goes first.
            if self.kept.is_some()
                && let Some(document) = &self.document
            {
                let place = document.read(cx).place().clone();
                self.clear_working_copies(place, cx);
                self.close_after_working_copies(cx);
                return false;
            }
            return true;
        }
        if !window.has_active_dialog(cx) {
            self.ask_before_closing(window, cx);
        }
        false
    }

    fn ask_before_closing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let answer = |answer: fn(&mut WorkspaceView, &mut Context<WorkspaceView>)| {
                let view = view.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    view.update(cx, answer).ok();
                }
            };
            dialog
                .title("Save before closing?")
                .child(UNSAVED_ON_CLOSE)
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new(CLOSE_CANCEL_ID)
                                .ghost()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new(CLOSE_DONT_SAVE_ID)
                                .label("Don't save")
                                .on_click(answer(|view, cx| {
                                    if let Some(document) = &view.document {
                                        let place = document.read(cx).place().clone();
                                        view.clear_working_copies(place, cx);
                                    }
                                    view.close_after_working_copies(cx);
                                })),
                        )
                        .child(
                            Button::new(CLOSE_SAVE_ID)
                                .primary()
                                .label("Save")
                                .on_click(answer(|view, cx| {
                                    view.close_after_save = true;
                                    view.save(cx);
                                })),
                        ),
                )
        });
    }

    /// Closes this window, once nothing else is using it.
    fn close_window(&mut self, cx: &mut Context<Self>) {
        let window = self.window_handle;
        cx.defer(move |cx| {
            window.update(cx, |_, window, _| window.remove_window()).ok();
        });
    }

    /// The bar that asks what to do about a change to the file made outside the app, while
    /// the talk has changes of its own.
    pub(super) fn outside_change_bar(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        self.document.as_ref()?.read(cx).outside_change()?;
        Some(
            h_flex()
                .id(OUTSIDE_CHANGE_ID)
                .test_support()
                .aria_label(OUTSIDE_CHANGE)
                .w_full()
                .gap_2()
                .px_2()
                .py_1()
                .items_center()
                .bg(cx.theme().secondary)
                .border_t_1()
                .border_color(cx.theme().border)
                .child(div().text_sm().child(OUTSIDE_CHANGE))
                .child(
                    Button::new(KEEP_MINE_ID)
                        .small()
                        .primary()
                        .label("Keep my changes")
                        .on_click(cx.listener(|this, _, _, cx| this.keep_mine(cx))),
                )
                .child(
                    Button::new(LOAD_THEIRS_ID)
                        .small()
                        .ghost()
                        .label("Load the changed file")
                        .on_click(cx.listener(|this, _, _, cx| this.load_theirs(cx))),
                ),
        )
    }

    /// The bar that offers back the unsaved changes from last time.
    pub(super) fn recovery_bar(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let found = self.offered.as_ref()?;
        let said: SharedString = if found.base_matches {
            RECOVERY_OFFER.into()
        } else {
            format!("{RECOVERY_OFFER} {RECOVERY_CHANGED_SINCE}").into()
        };
        Some(
            h_flex()
                .id(RECOVERY_ID)
                .test_support()
                .aria_label(said.clone())
                .w_full()
                .gap_2()
                .px_2()
                .py_1()
                .items_center()
                .bg(cx.theme().secondary)
                .border_t_1()
                .border_color(cx.theme().border)
                .child(div().text_sm().child(said))
                .child(
                    Button::new(RECOVER_ID)
                        .small()
                        .primary()
                        .label("Bring them back")
                        .on_click(cx.listener(|this, _, _, cx| this.bring_back(cx))),
                )
                .child(
                    Button::new(DISCARD_RECOVERY_ID)
                        .small()
                        .ghost()
                        .label("Discard")
                        .on_click(cx.listener(|this, _, _, cx| this.discard_recovered(cx))),
                ),
        )
    }
}

/// What to call the place a talk is saved to: the name of its folder or file.
fn place_label(place: &TalkPlace) -> String {
    let (TalkPlace::Folder(path) | TalkPlace::Packed(path)) = place;
    file_label(path)
}

fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
