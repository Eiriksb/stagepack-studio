//! Putting things on the slide from the window: pictures chosen in the system's dialog, dropped
//! on the slide or pasted, shapes and text boxes from the Insert tab.

use super::*;
use crate::added_files::{FileKind, MAX_ADDED_BYTES, NotAdded};
use crate::clipboard::{LargeCopied, PastedFiles};
use crate::clipboard_tools::ClipboardThing;
use crate::document::{InsertFailed, LargeCopy};
use crate::insert::{ICON_SIZE, INSERT_ICON};
use crate::large_files::{LargeSource, ask_before_copying, ask_to_copy};

/// What the window says when a file that isn't a picture is dropped on the slide.
pub const ONLY_PICTURES: &str = "Only pictures can be dropped on a slide.";
/// What the window says when the dialog to choose pictures couldn't be shown.
pub const DIALOG_FAILED: &str = "The file chooser couldn't be opened.";
/// The most files a drop, a paste of files, or the dialog puts on the slide.
pub const MOST_DROPPED: usize = 20;
/// What the window says when more files than that were dropped or chosen.
pub const ONLY_THE_FIRST: &str = "Only the first 20 pictures were added.";
/// What a picture pasted from the clipboard is named, as like as is safe.
const PASTED_PICTURE: &str = "pasted-picture.png";
/// What the status line says while a video is copied into the talk.
const COPYING_VIDEO: &str = "Copying video";

/// What pastes, once the large files a paste carries are copied in, or not.
type Paste = Box<dyn FnOnce(&mut WorkspaceView, PastedFiles, &mut Context<WorkspaceView>)>;

/// A paste waiting to be told whether to copy the large files it carries into the talk.
struct WaitingPaste {
    copies: Vec<(usize, LargeCopy)>,
    files: PastedFiles,
    paste: Paste,
}

impl WorkspaceView {
    /// Asks for pictures in the system's dialog, and puts each on the slide.
    pub(super) fn ask_for_pictures(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_none() {
            return;
        }
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Insert".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            match chosen.await {
                Ok(Ok(Some(paths))) => {
                    this.update_in(cx, |this, window, cx| {
                        this.chosen_pictures(paths, cx);
                        this.keyboard_to_the_slide(window, cx);
                    })
                    .ok();
                }
                Ok(Err(_)) => {
                    this.update(cx, |this, cx| this.say(DIALOG_FAILED, cx)).ok();
                }
                _ => {}
            }
        })
        .detach();
    }

    /// Puts the pictures in the files at `paths` on the slide, each in turn, one step each, as
    /// the dialog to choose them gives them, the first [`MOST_DROPPED`] of them. What isn't a
    /// picture is said, and left out.
    pub fn chosen_pictures(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.insert_files(paths, false, cx);
    }

    /// Puts the pictures in the files at `paths`, dropped on the slide from outside the app, on
    /// it, each in turn, one step each, the first [`MOST_DROPPED`] of them. A file that isn't a
    /// picture is said, and left out.
    pub(super) fn dropped(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.insert_files(paths, true, cx);
    }

    /// Puts the pictures in the first [`MOST_DROPPED`] of the files at `paths` on the slide, and
    /// says if there were more.
    fn insert_files(&mut self, paths: Vec<PathBuf>, dropped: bool, cx: &mut Context<Self>) {
        let more = paths.len() > MOST_DROPPED;
        for path in paths.iter().take(MOST_DROPPED) {
            self.insert_file(path, dropped, cx);
        }
        if more {
            // After the status line has heard what's selected, so that this is what it says.
            cx.spawn(async move |this, cx| {
                this.update(cx, |this, cx| this.say(ONLY_THE_FIRST, cx)).ok();
            })
            .detach();
        }
    }

    /// Puts the picture in the file at `path`, from outside the talk, on the slide.
    fn insert_file(&mut self, path: &Path, dropped: bool, cx: &mut Context<Self>) {
        let suggested = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        match Document::read_outside_file(path) {
            Ok(bytes) => self.insert_picture(&suggested, bytes, dropped, cx),
            Err(why) => self.say(not_added(why, dropped), cx),
        }
    }

    /// Puts the picture on the clipboard on the slide.
    pub(super) fn paste_picture(&mut self, picture: Image, cx: &mut Context<Self>) {
        // Checked before it's looked at any closer.
        if picture.bytes.len() > MAX_ADDED_BYTES {
            return self.say(NotAdded::TooLarge.to_string(), cx);
        }
        self.insert_picture(PASTED_PICTURE, picture.bytes, false, cx);
    }

    /// Puts what's on the clipboard, a picture or the files it names, on the slide.
    pub(super) fn paste_thing(&mut self, thing: ClipboardThing, cx: &mut Context<Self>) {
        match thing {
            ClipboardThing::Picture(picture) => self.paste_picture(picture, cx),
            ClipboardThing::Paths(paths) => self.dropped(paths, cx),
        }
    }

    /// Adds `bytes`, a picture named `suggested`, to the talk, and puts it on the slide, as one
    /// edit. Why it couldn't be is said, as dropped on the slide if `dropped`.
    fn insert_picture(&mut self, suggested: &str, bytes: Vec<u8>, dropped: bool, cx: &mut Context<Self>) {
        let mut not_a_file = None;
        // The edit's notice is what has the working copy keep the file added.
        self.edit(
            |document| match document.insert_picture(suggested, bytes) {
                // What changed is seen in the talk itself.
                Ok(()) => Ok(Vec::new()),
                Err(InsertFailed::Edit(refused)) => Err(refused),
                Err(InsertFailed::File(why)) => {
                    not_a_file = Some(why);
                    Ok(Vec::new())
                }
            },
            cx,
        );
        if let Some(why) = not_a_file {
            self.say(not_added(why, dropped), cx);
        }
    }

    /// Copies the large file at `path`, of `kind`, such as a video, into the talk on a background
    /// task, the status line showing how far it's got, with a Cancel, and then hands its path in
    /// the talk to `then`. Why it wasn't copied is said instead, and a copy cancelled or refused
    /// leaves nothing.
    pub fn copy_in_large_file(
        &mut self,
        kind: FileKind,
        path: PathBuf,
        then: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let suggested = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let copy = match document.read(cx).large_copy(kind, &suggested, LargeSource::File(path), None) {
            Ok(copy) => copy,
            Err(why) => return self.say(why.to_string(), cx),
        };
        let meter = self.start_progress(COPYING_VIDEO, true, cx);
        let running = meter.clone();
        let start = move |_: &mut Self, cx: &mut Context<Self>| cx.spawn(async move |this, cx| {
            let copied = cx.background_spawn(async move { copy.run(&|done, total| running.tell(done, total), running.stop()) }).await;
            this.update(cx, |this, cx| {
                this.end_progress(&meter, cx);
                let taken = document.update(cx, |document, cx| {
                    let taken = document.take_large_copy(copied);
                    cx.notify();
                    taken
                });
                match taken {
                    Ok(path) => then(this, path, cx),
                    Err(why) => this.say(why.to_string(), cx),
                }
            })
            .ok();
        })
        .detach();
        self.start_work(false, start, cx);
    }

    /// Copies the large files `files` carries that `markdown`, a pasted thing's or slide's, uses
    /// into the talk first, then pastes with `paste`, each marked with what became of it. One
    /// that's gone, or changed, since it was copied isn't copied. More than
    /// [`ask_before_copying`] is copied only once whoever pasted says to; told not to, the paste
    /// goes on without them. Cancelled while they're copied, nothing is pasted, and nothing is
    /// left.
    pub(super) fn bring_large_first(
        &mut self,
        markdown: &str,
        mut files: PastedFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
        paste: impl FnOnce(&mut Self, PastedFiles, &mut Context<Self>) + 'static,
    ) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let mut copies = Vec::new();
        for at in document.read(cx).large_to_bring(markdown, &files) {
            match document.read(cx).large_copy_for_paste(&files.large[at]) {
                Ok(copy) => copies.push((at, copy)),
                // Not where the copy said, or not as it was.
                Err(NotAdded::Unreadable | NotAdded::Changed) => files.large[at].copied = LargeCopied::Gone,
                Err(why) => files.large[at].copied = LargeCopied::Refused(why),
            }
        }
        if copies.is_empty() {
            return paste(self, files, cx);
        }
        let bytes: u64 = copies.iter().map(|(_, copy)| copy.len()).sum();
        let waiting = WaitingPaste { copies, files, paste: Box::new(paste) };
        if bytes <= ask_before_copying() {
            return self.copy_large_then(waiting, cx);
        }
        if window.has_active_dialog(cx) {
            // Nothing is asked while another question is up, so nothing is pasted.
            return self.say(crate::clipboard::ANSWER_FIRST, cx);
        }
        let asked = ask_to_copy(waiting.copies.len(), bytes);
        let waiting = std::rc::Rc::new(std::cell::RefCell::new(Some(waiting)));
        let view = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let answer = |copy: bool| {
                let (view, waiting) = (view.clone(), waiting.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    let Some(mut waiting) = waiting.borrow_mut().take() else { return };
                    view.update(cx, |view, cx| {
                        if copy {
                            view.copy_large_then(waiting, cx);
                        } else {
                            for (at, _) in std::mem::take(&mut waiting.copies) {
                                waiting.files.large[at].copied = LargeCopied::Declined;
                            }
                            (waiting.paste)(view, waiting.files, cx);
                        }
                    })
                    .ok();
                }
            };
            let cancelled = waiting.clone();
            dialog
                .title("Copy the video?")
                // Escape, a click beside it, or its close button: nothing is pasted.
                .on_cancel(move |_, _, _| {
                    cancelled.borrow_mut().take();
                    true
                })
                .child(asked.clone())
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(Button::new(DONT_COPY_LARGE_ID).ghost().label("Don't copy").on_click(answer(false)))
                        .child(Button::new(COPY_LARGE_ID).primary().label("Copy").on_click(answer(true))),
                )
        });
    }

    /// Copies the large files `waiting` holds into the talk on a background task, the status line
    /// showing how far it's got, with a Cancel, then pastes as it says.
    fn copy_large_then(&mut self, waiting: WaitingPaste, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let WaitingPaste { copies, mut files, paste } = waiting;
        let meter = self.start_progress(COPYING_VIDEO, true, cx);
        let running = meter.clone();
        let start = move |_: &mut Self, cx: &mut Context<Self>| cx.spawn(async move |this, cx| {
            let copied = cx
                .background_spawn(async move {
                    let total: u64 = copies.iter().map(|(_, copy)| copy.len()).sum();
                    let mut before = 0u64;
                    let mut copied = Vec::new();
                    for (at, copy) in copies {
                        let len = copy.len();
                        // A copy that hashes first counts its bytes twice: told as a share of its own.
                        let told = |done: u64, of: u64| {
                            let share = (u128::from(done) * u128::from(len)).checked_div(u128::from(of)).unwrap_or(0) as u64;
                            running.tell(before + share, total);
                        };
                        let one = copy.run(&told, running.stop());
                        before += len;
                        let cancelled = one.refused() == Some(NotAdded::Cancelled);
                        copied.push((at, one));
                        if cancelled {
                            break;
                        }
                    }
                    copied
                })
                .await;
            this.update(cx, |this, cx| {
                this.end_progress(&meter, cx);
                if copied.iter().any(|(_, one)| one.refused() == Some(NotAdded::Cancelled)) {
                    // Dropped, what was copied goes with it.
                    drop(copied);
                    return this.say(NotAdded::Cancelled.to_string(), cx);
                }
                for (at, one) in copied {
                    let taken = document.update(cx, |document, _| document.take_large_copy(one));
                    files.large[at].copied = match taken {
                        Ok(path) => LargeCopied::To(path),
                        Err(NotAdded::Changed) => LargeCopied::Gone,
                        Err(why) => LargeCopied::Refused(why),
                    };
                }
                paste(this, files, cx);
            })
            .ok();
        })
        .detach();
        self.start_work(false, start, cx);
    }

    /// Gives the slide the keyboard once a thing just put on it is selected there, from
    /// wherever it was, so that the next keys type in it or take it away. Left in the list of
    /// slides, Delete would take the whole slide away.
    pub(super) fn keyboard_to_the_slide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected_here = self.document.as_ref().is_some_and(|document| {
            let document = document.read(cx);
            document.selection().is_some_and(|selection| selection.slide == document.current())
        });
        if selected_here {
            let slide = self.slide_editor.read(cx).focus_handle().clone();
            slide.focus(window, cx);
        }
    }

    /// Puts a shape of kind `kind` on the slide.
    pub(super) fn insert_shape(&mut self, kind: &str, cx: &mut Context<Self>) {
        self.edit(|document| document.insert_shape(kind), cx);
    }

    /// Puts icon `name` on the slide, as big as a new icon is.
    pub(super) fn insert_icon(&mut self, name: &str, cx: &mut Context<Self>) {
        self.edit(|document| document.insert_tag(INSERT_ICON, "Icon", &[("name", name), ("size", ICON_SIZE)], None), cx);
    }

    /// Starts typing in a new text box on the slide, which isn't written until something is
    /// typed in it. The editor hears of it as it hears of any change to the document.
    pub(super) fn insert_text_box(&mut self, cx: &mut Context<Self>) {
        self.edit(Document::insert_text_box, cx);
        if let Some(document) = &self.document {
            document.update(cx, |_, cx| cx.notify());
        }
    }
}

/// What the window says when a file wasn't added for `why`, dropped on the slide if `dropped`.
fn not_added(why: NotAdded, dropped: bool) -> String {
    match why {
        NotAdded::NotAPicture | NotAdded::NotThatKind if dropped => ONLY_PICTURES.to_string(),
        why => why.to_string(),
    }
}
