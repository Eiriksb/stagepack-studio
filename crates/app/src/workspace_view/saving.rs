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

impl WorkspaceView {
    /// Keeps working copies of unsaved changes in `recovery`, in a slot of this window's own,
    /// and offers back the newest one left there for this talk, if any.
    pub(super) fn use_recovery(&mut self, recovery: Recovery, cx: &mut Context<Self>) {
        self.recovery = Some(recovery.clone());
        self.working_copies
            .update(cx, |copies, cx| copies.use_recovery(recovery, cx));
    }

    /// Takes back the unsaved changes from last time, as one edit to undo. They're kept as
    /// this window's own straight away, and last time's copy goes once they are.
    fn bring_back(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let Some(mut found) = self
            .working_copies
            .update(cx, |copies, _| copies.take_offered())
        else {
            return;
        };
        let text = std::mem::take(&mut found.text);
        let files = found.files.clone();
        let builds = found.builds.clone();
        let large = found.large.clone();
        let recovered = document.update(cx, |document, cx| {
            let recovered = document.recover_with_builds(text.clone(), files, builds);
            if recovered.is_ok() {
                // Taken into this window's own working copy, whatever becomes of last time's.
                document.bring_back_large(large);
                // A show of the talk has to know.
                cx.notify();
            }
            recovered
        });
        if let Err(refused) = recovered {
            // Nothing was taken, so they're still there to bring back.
            found.text = text;
            self.working_copies
                .update(cx, |copies, cx| copies.offer_again(found, cx));
            self.say(refused.to_string(), cx);
            return;
        }
        self.working_copies
            .update(cx, |copies, cx| copies.brought_back(found, cx));
        let source = document.read(cx).source().to_string();
        self.markdown_panel
            .update(cx, |panel, cx| panel.set_source(&source, cx));
        self.show_slide(cx);
    }

    /// Lets the unsaved changes from last time go. Edits made here since are in a copy of
    /// their own, which stays.
    fn discard_recovered(&mut self, cx: &mut Context<Self>) {
        self.working_copies
            .update(cx, |copies, cx| copies.discard_offered(cx));
    }

    /// Closes the window once the changes to the working copies asked for are made: they'd
    /// be lost with it.
    pub(super) fn close_after_working_copies(&mut self, cx: &mut Context<Self>) {
        // A copy still running has no use now: it stops, and what it copied goes with it.
        if !self.close_anyway {
            self.stop_before_closing();
        }
        self.close_when_idle = false;
        // Nothing of the working copy goes while a save may be reading it, or a copy stopped may
        // still be writing to it: the window finishes closing once they've ended.
        if self.close_waits() {
            self.finish_closing = true;
            return;
        }
        self.working_copies.update(cx, |copies, cx| copies.let_go_closing(cx));
        let settled = self.working_copies.update(cx, |copies, _| copies.settled());
        cx.spawn(async move |this, cx| {
            settled.await;
            this.update(cx, |this, cx| this.close_window(cx)).ok();
        })
        .detach();
    }

    /// Does what the saving said to, once an event of its own has been taken in.
    fn follow(&mut self, effect: Effect, cx: &mut Context<Self>) {
        match effect {
            Effect::StartSave => self.start_save(cx),
            Effect::Reload => {
                if let Some(folder) = &self.talk_folder {
                    let file = folder.join(SLIDES_FILE);
                    self.reload_soon(file, cx);
                }
            }
            Effect::Close => self.close_after_working_copies(cx),
            Effect::LetGo => {
                if let Some(document) = &self.document {
                    let place = document.read(cx).place().clone();
                    self.working_copies
                        .update(cx, |copies, cx| copies.let_go(place, cx));
                }
            }
            Effect::Say(said) => self.say(said, cx),
            // Only events that come with a read or a window give these, and what took them
            // in follows them itself.
            Effect::TakeRead | Effect::AskAboutOutsideChange | Effect::AskBeforeClosing | Effect::AskWhereToSave => {}
        }
    }

    /// Saves the talk where it's kept, on a background task. An untitled talk is kept nowhere
    /// yet, so this is Save As for it.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = &self.document else {
            return;
        };
        let effects = if document.read(cx).is_untitled() {
            self.save_flow.ask_save_untitled()
        } else {
            self.save_flow.ask_save()
        };
        self.follow_in(effects, window, cx);
    }

    /// Follows `effects`, asking where to save in `window` if one says to.
    fn follow_in(&mut self, effects: Vec<Effect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                Effect::AskWhereToSave => self.ask_to_save_as(window, cx),
                effect => self.follow(effect, cx),
            }
        }
    }

    /// Runs the save the saving has started, of the talk as it is now where it's kept.
    fn start_save(&mut self, cx: &mut Context<Self>) {
        self.settle_typing(cx);
        let Some(document) = &self.document else {
            return;
        };
        let document = document.read(cx);
        let job = document.save_job();
        let name = place_label(document.place());
        self.run_save(job, name, cx);
    }

    /// Asks which form to save the talk in, then where. While another dialog is showing, nothing
    /// is asked.
    pub(super) fn ask_to_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_none() {
            return;
        }
        if window.has_active_dialog(cx) {
            // Nothing is asked, so nothing waits on an answer.
            self.place_not_chosen(cx);
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
            let not_chosen = view.clone();
            let cancelled = view.clone();
            dialog
                .title("Save as")
                // Escape, a click beside it, or its close button.
                .on_cancel(move |_, _, cx| {
                    not_chosen.update(cx, |view, cx| view.place_not_chosen(cx)).ok();
                    true
                })
                .child("Keep the talk as a folder of files, or pack it into one file to send.")
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new(SAVE_AS_CANCEL_ID)
                                .ghost()
                                .label("Cancel")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    cancelled.update(cx, |view, cx| view.place_not_chosen(cx)).ok();
                                }),
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

    /// No place to save the talk in was chosen: whatever was waiting on one is let go.
    fn place_not_chosen(&mut self, cx: &mut Context<Self>) {
        for effect in self.save_flow.place_not_chosen() {
            self.follow(effect, cx);
        }
    }

    /// The folder the Save As dialog starts in, and the name it suggests, for the talk saved as
    /// a packed file if `packed`, or a folder: beside the talk, or for an untitled one in the
    /// Documents folder, under the talk's name. For tests, which can't see what the dialog is
    /// given.
    #[doc(hidden)]
    pub fn save_as_suggestion(&self, packed: bool, cx: &App) -> (PathBuf, String) {
        let Some(document) = &self.document else {
            return (PathBuf::new(), String::new());
        };
        let document = document.read(cx);
        let stem = export::file_stem_for(&document.name());
        let suggested = if packed { format!("{stem}.{PACKED_EXTENSION}") } else { stem };
        (self.save_folder(document), suggested)
    }

    /// Asks where to save the talk as `kind`, then saves it there on a background task. From
    /// then on the talk is kept there.
    fn save_as(&mut self, kind: SaveKind, cx: &mut Context<Self>) {
        self.settle_typing(cx);
        if self.document.is_none() {
            return;
        }
        let (folder, suggested) = self.save_as_suggestion(matches!(kind, SaveKind::Packed), cx);
        let chosen = cx.prompt_for_new_path(&folder, Some(&suggested));
        cx.spawn(async move |this, cx| {
            let out = match chosen.await {
                Ok(Ok(Some(out))) => out,
                Ok(Err(error)) => {
                    let problem = format!("Could not show the Save dialog: {error}");
                    this.update(cx, |this, cx| {
                        this.say(problem, cx);
                        this.place_not_chosen(cx);
                    })
                    .ok();
                    return;
                }
                _ => {
                    this.update(cx, |this, cx| this.place_not_chosen(cx)).ok();
                    return;
                }
            };
            this.update(cx, |this, cx| {
                // Typing may have gone on while the dialog was up.
                this.settle_typing(cx);
                let Some(document) = this.document.clone() else {
                    return;
                };
                let first = document.read(cx).is_untitled();
                for effect in this.save_flow.place_chosen(first) {
                    let Effect::StartSave = effect else {
                        this.follow(effect, cx);
                        continue;
                    };
                    let name = file_label(&out);
                    let target = match kind {
                        SaveKind::Folder => SaveTarget::Folder(out.clone()),
                        SaveKind::Packed => SaveTarget::Packed(out.clone()),
                    };
                    let job = document.read(cx).save_as_job(target);
                    this.run_save(job, name, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Runs `job`, which saves to the place called `name`, on a background task. One that moves a
    /// large file shows how far it's got on the status line, with a Cancel. One that replaces the
    /// talk's pack lets go of every reader of it first, as nothing may hold it open meanwhile.
    fn run_save(&mut self, job: SaveJob, name: String, cx: &mut Context<Self>) {
        let mut job = job;
        if job.replaces_the_pack()
            && let Some(document) = &self.document
        {
            let let_go = document.read(cx).let_go_of_readers();
            job = job.reading_back(let_go > 0);
        }
        let meter = job.moves_large().then(|| self.start_progress(format!("{} {name}", SAVING.doing), false, cx));
        if meter.is_none() {
            self.say(format!("{} {name}…", SAVING.doing), cx);
        }
        let running = meter.clone();
        self.saves_running += 1;
        let start = move |_: &mut Self, cx: &mut Context<Self>| {
            cx.spawn(async move |this, cx| {
                let outcome = cx
                    .background_spawn(async move {
                        match running {
                            Some(meter) => job.run_with(&|done, total| meter.tell(done, total), meter.stop()),
                            None => job.run(),
                        }
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.saves_running = this.saves_running.saturating_sub(1);
                    if let Some(meter) = &meter {
                        this.end_progress(meter, cx);
                    }
                    this.finish_save(outcome, name, cx);
                    // A window closing that waited for the save closes now.
                    this.closing_may_go_on(cx);
                })
                .ok();
            })
            .detach();
        };
        self.start_work(true, start, cx);
    }

    fn finish_save(
        &mut self,
        outcome: Result<Saved, SaveError>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.document.clone() else {
            self.save_flow = SaveFlow::Idle;
            return;
        };
        let (outcome, mut theirs) = match outcome {
            Ok(saved) => {
                let said = if saved.notices().is_empty() {
                    format!("{} {name}.", SAVING.done)
                } else {
                    format!("{} {name}. {}", SAVING.done, saved.notices().join(" · "))
                };
                let moved = saved.moved();
                let saved_from = document.read(cx).place().clone();
                let first = matches!(saved_from, TalkPlace::Untitled(_));
                // Made here, and nothing in it came from anyone else's code.
                let made_here = first && !document.read(cx).holds_component_source();
                document.update(cx, |document, cx| {
                    document.saved(saved);
                    cx.notify();
                });
                // What it kept is in the file now. Anything edited since is kept again.
                self.working_copies
                    .update(cx, |copies, cx| copies.saved(saved_from, cx));
                if moved {
                    self.moved(first, made_here, cx);
                }
                self.say_for_a_while(said, cx);
                (SaveOutcome::Saved, None)
            }
            Err(SaveError::ChangedOutside(theirs)) => (SaveOutcome::ChangedOutside, Some(theirs)),
            Err(problem) => (SaveOutcome::Failed(problem.to_string()), None),
        };
        let still_unsaved = document.read(cx).has_unsaved_changes();
        for effect in self.save_flow.finished(outcome, still_unsaved) {
            match effect {
                Effect::AskAboutOutsideChange => {
                    if let Some(theirs) = theirs.take() {
                        self.reload(Some(theirs), cx);
                    }
                }
                effect => self.follow(effect, cx),
            }
        }
    }

    /// Follows the talk to where Save As put it: its folder is the one watched and built. After
    /// its `first` save its saves are followed as an opened folder's are. A folder of a talk
    /// `made_here`, holding no component source at all, is always built from then on, as the
    /// design has it for talks made in the app. One holding any, pasted or brought back, came
    /// partly from someone else, so nothing is built without a yes: its bar asks, as for any
    /// folder opened.
    fn moved(&mut self, first: bool, made_here: bool, cx: &mut Context<Self>) {
        let Some(document) = &self.document else {
            return;
        };
        self.talk_folder = talk_folder_of(document.read(cx).place());
        if made_here && let (Some(folder), Some(permissions)) = (&self.talk_folder, &mut self.permissions) {
            // If it can't be written down, the bar offers it, as for any folder.
            permissions.set_always(folder).ok();
        }
        self.always_build = self.is_always_built();
        self.build_problems.clear();
        self.pending_reload = None;
        self.pending_components = None;
        let watched = self.followed.take().is_some() | self.watcher.take().is_some();
        let tried_to_watch = self.watch_problem.take().is_some();
        let followed = first || self.opened_from_path || watched || tried_to_watch;
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
            self.working_copies
                .update(cx, |copies, cx| copies.talk_changed(cx));
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
        let taken = document.update(cx, |document, cx| {
            let taken = document.take_theirs(theirs);
            if taken.is_ok() {
                // A show of the talk has to know.
                cx.notify();
            }
            taken
        });
        if let Err(refused) = taken {
            self.say(refused.to_string(), cx);
            return;
        }
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
        if self.working_copies.read(cx).is_closing() {
            return true;
        }
        if self.finish_closing {
            // Closing already, once what it waits for has ended. Asked again, it no longer waits
            // for copies it stopped, though it does for a save.
            self.close_anyway = true;
            self.closing_may_go_on(cx);
            return false;
        }
        // A copy into the talk running is stopped first, and the window closes once it has ended.
        // Asked again meanwhile, it goes ahead without waiting for it.
        if self.stop_before_closing() {
            return false;
        }
        for effect in self.save_flow.ask_close(unsaved) {
            match effect {
                Effect::Close => return self.close_now_or_once_settled(cx),
                Effect::AskBeforeClosing => {
                    if !window.has_active_dialog(cx) {
                        self.ask_before_closing(window, cx);
                    }
                }
                effect => self.follow(effect, cx),
            }
        }
        false
    }

    /// Whether the window, with nothing unsaved, can close now. If changes to the working
    /// copies are still to be made, it closes itself once they are.
    fn close_now_or_once_settled(&mut self, cx: &mut Context<Self>) -> bool {
        // A save running may be reading what the working copy holds: the window closes once it
        // has ended.
        if self.close_waits() {
            self.finish_closing = true;
            return false;
        }
        // Back to what's saved within the wait: the copy still there goes first, and with it any
        // video added that the talk no longer uses.
        self.working_copies.update(cx, |copies, cx| copies.let_go_closing(cx));
        // A change to the working copies still to be made, such as the clear after a save,
        // would be lost with the window.
        if self.working_copies.read(cx).has_jobs() {
            self.close_after_working_copies(cx);
            return false;
        }
        true
    }

    /// Takes in the answer to the dialog asking before the window closes. For an untitled talk,
    /// Save asks where first.
    fn answer_close(&mut self, answer: CloseAnswer, window: &mut Window, cx: &mut Context<Self>) {
        let untitled = self.document.as_ref().is_some_and(|document| document.read(cx).is_untitled());
        let effects = if untitled {
            self.save_flow.answer_close_untitled(answer)
        } else {
            self.save_flow.answer_close(answer)
        };
        self.follow_in(effects, window, cx);
    }

    fn ask_before_closing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let answer = |answer: CloseAnswer| {
                let view = view.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    view.update(cx, |view, cx| view.answer_close(answer, window, cx))
                        .ok();
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
                                .on_click(answer(CloseAnswer::Cancel)),
                        )
                        .child(
                            Button::new(CLOSE_DONT_SAVE_ID)
                                .label("Don't save")
                                .on_click(answer(CloseAnswer::DontSave)),
                        )
                        .child(
                            Button::new(CLOSE_SAVE_ID)
                                .primary()
                                .label("Save")
                                .on_click(answer(CloseAnswer::Save)),
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
                .border_b_1()
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
        let found = self.working_copies.read(cx).offered()?;
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
                .border_b_1()
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
    match place {
        TalkPlace::Folder(path) | TalkPlace::Packed(path) => file_label(path),
        // Never saved where it is: it's saved as, to a place of its own.
        TalkPlace::Untitled(_) => place.name(),
    }
}

fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
