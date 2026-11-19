//! Putting things on the slide from the window: pictures, videos and 3D models chosen in the
//! system's dialog, dropped on the slide or pasted, shapes and text boxes from the Insert tab.

use super::*;
use crate::added_files::{FileKind, MAX_ADDED_BYTES, NotAdded};
use crate::clipboard::{LargeCopied, PastedFiles};
use crate::clipboard_tools::ClipboardThing;
use crate::document::{InsertFailed, LargeCopy};
use crate::insert::{ICON_SIZE, INSERT_ICON};
use crate::large_files::{LargeSource, ask_before_copying, ask_to_copy_of};
use std::sync::Arc;
use slides_model::{CantDraw, ModelKind, View};
use slides_video::CantPlay;

/// What the window says when a file that isn't a picture, a video or a 3D model is dropped on the
/// slide.
pub const ONLY_PICTURES: &str = "Only pictures, videos and 3D models can be dropped on a slide.";
/// What the window says when the dialog to choose pictures couldn't be shown.
pub const DIALOG_FAILED: &str = "The file chooser couldn't be opened.";
/// The most files a drop, a paste of files, or the dialog puts on the slide.
pub const MOST_DROPPED: usize = 20;
/// What the window says when more files than that were dropped or chosen.
pub const ONLY_THE_FIRST: &str = "Only the first 20 files were added.";
/// What a picture pasted from the clipboard is named, as like as is safe.
const PASTED_PICTURE: &str = "pasted-picture.png";
/// What the status line says while a large file called `noun`, as a video or a model, is copied
/// into the talk.
fn copying(noun: &str) -> String {
    format!("Copying {noun}")
}

/// What the files being copied by `copies` are called, in one word: their kind's if they're all of
/// one, otherwise a plain "file".
fn noun_of(copies: &[(usize, LargeCopy)]) -> &'static str {
    crate::large_files::noun_of_kinds(copies.iter().map(|(_, copy)| copy.kind()))
}

/// A video or a 3D model, of `kind`, waiting to be copied in, from the file at `path`, and put on
/// slide `slide`, the slide shown when it was asked for.
pub(super) struct WaitingLarge {
    kind: FileKind,
    path: PathBuf,
    slide: usize,
}

/// What looking at a large file before it's copied in found: a video's size, or the view a
/// model is framed whole from.
enum Found {
    Video((u32, u32)),
    Model(View),
}

/// Why a large file wasn't copied in: what's said of it, and whether it was cancelled.
struct NotCopied {
    said: String,
    cancelled: bool,
}

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

    /// Asks for videos in the system's dialog, and puts each on the slide.
    pub(super) fn ask_for_videos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                    this.update(cx, |this, cx| this.chosen_videos(paths, cx)).ok();
                }
                Ok(Err(_)) => {
                    this.update(cx, |this, cx| this.say(DIALOG_FAILED, cx)).ok();
                }
                _ => {}
            }
        })
        .detach();
    }

    /// Asks for 3D models in the system's dialog, and puts each on the slide.
    pub(super) fn ask_for_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_none() {
            return;
        }
        // The system's dialog can't be told which kinds to show: a file that isn't a model is
        // said once it's chosen.
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Insert".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            match chosen.await {
                Ok(Ok(Some(paths))) => {
                    this.update(cx, |this, cx| this.chosen_models(paths, cx)).ok();
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
    /// picture is said, and left out. A video among them is copied in, and put on the slide
    /// once it's there.
    pub fn chosen_pictures(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.insert_files(paths, false, cx);
    }

    /// Puts the videos in the files at `paths`, as the Video tool's dialog gives them, the first
    /// [`MOST_DROPPED`] of them, on the slide, each once it's copied into the talk, the status
    /// line showing how far that's got. A file that isn't a video is said, and left out.
    pub fn chosen_videos(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let more = paths.len() > MOST_DROPPED;
        for path in paths.into_iter().take(MOST_DROPPED) {
            if is_kind(&path, FileKind::Video) {
                self.insert_large_file(FileKind::Video, path, cx);
            } else {
                self.say(CantPlay::NotAVideo.to_string(), cx);
            }
        }
        if more {
            self.say_later(ONLY_THE_FIRST, cx);
        }
    }

    /// Puts the 3D models in the files at `paths`, as the Model tool's dialog gives them, the
    /// first [`MOST_DROPPED`] of them, on the slide, each once it's copied into the talk, framed
    /// whole, the status line showing how far that's got. A file that isn't a model is said, and
    /// left out.
    pub fn chosen_models(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let more = paths.len() > MOST_DROPPED;
        for path in paths.into_iter().take(MOST_DROPPED) {
            if is_kind(&path, FileKind::Model) {
                self.insert_large_file(FileKind::Model, path, cx);
            } else {
                self.say(CantDraw::NotAModel.to_string(), cx);
            }
        }
        if more {
            self.say_later(ONLY_THE_FIRST, cx);
        }
    }

    /// Puts the pictures, videos and 3D models in the files at `paths`, dropped on the slide from
    /// outside the app, on it, each in turn, one step each, the first [`MOST_DROPPED`] of them. A
    /// file that's none of them is said, and left out.
    pub(super) fn dropped(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.insert_files(paths, true, cx);
    }

    /// Puts the pictures, videos and 3D models in the first [`MOST_DROPPED`] of the files at
    /// `paths` on the slide, and says if there were more. A picture goes on at once; a video or a
    /// model once it's copied in.
    fn insert_files(&mut self, paths: Vec<PathBuf>, dropped: bool, cx: &mut Context<Self>) {
        let more = paths.len() > MOST_DROPPED;
        for path in paths.iter().take(MOST_DROPPED) {
            if let Some(kind) = [FileKind::Video, FileKind::Model].into_iter().find(|kind| is_kind(path, *kind)) {
                self.insert_large_file(kind, path.clone(), cx);
            } else {
                self.insert_file(path, dropped, cx);
            }
        }
        if more {
            self.say_later(ONLY_THE_FIRST, cx);
        }
    }

    /// Puts `said` on the status line once the line has heard what's selected, so that this is
    /// what it says: once the videos and models being copied in are all in, if any are.
    fn say_later(&mut self, said: &'static str, cx: &mut Context<Self>) {
        if self.large_copying {
            self.said_after_large = Some(said);
            return;
        }
        cx.spawn(async move |this, cx| {
            this.update(cx, |this, cx| this.say(said, cx)).ok();
        })
        .detach();
    }

    /// Copies the video or the 3D model, of `kind`, in the file at `path`, from outside the talk,
    /// into it, the status line showing how far that's got, with a Cancel, and then puts it on
    /// the slide shown now, selected: that slide is shown again then, as any edit shows its
    /// slide. A video goes on at its own size, and a model framed whole. They're copied one at a
    /// time, in the order they were asked for. The file is looked at first, off the window's
    /// thread: one that isn't of its kind is said, and nothing is copied. Cancelled or refused,
    /// nothing is put on the slide; cancelled, the files waiting after it aren't copied either.
    fn insert_large_file(&mut self, kind: FileKind, path: PathBuf, cx: &mut Context<Self>) {
        let Some(document) = &self.document else {
            return;
        };
        let slide = document.read(cx).current();
        self.large_waiting.push_back(WaitingLarge { kind, path, slide });
        self.copy_next_large(cx);
    }

    /// Copies in the next video or model waiting, unless one is being copied. In a window
    /// closing, none is.
    fn copy_next_large(&mut self, cx: &mut Context<Self>) {
        if self.large_copying {
            return;
        }
        if self.is_closing_window() {
            self.large_waiting.clear();
            self.said_after_large = None;
            return;
        }
        let Some(WaitingLarge { kind, path, slide }) = self.large_waiting.pop_front() else {
            if let Some(said) = self.said_after_large.take() {
                self.say_later(said, cx);
            }
            return;
        };
        self.large_copying = true;
        let finished = move |this: &mut Self, copied: Result<(String, Found), NotCopied>, cx: &mut Context<Self>| {
            this.large_copying = false;
            match copied {
                Ok((name, found)) => {
                    this.edit(
                        |document| match found {
                            Found::Video(size) => document.insert_video_on(slide, name, size),
                            Found::Model(view) => document.insert_model_on(slide, name, Some(view)),
                        },
                        cx,
                    );
                    // Its Delete and its arrow keys are the slide's, as a picture's are once it's put on.
                    let (window, view) = (this.window_handle, cx.weak_entity());
                    cx.defer(move |cx| {
                        window
                            .update(cx, |_, window, cx| view.update(cx, |view, cx| view.keyboard_to_the_slide(window, cx)).ok())
                            .ok();
                    });
                }
                Err(NotCopied { said, cancelled }) => {
                    if cancelled {
                        this.large_waiting.clear();
                        this.said_after_large = None;
                    }
                    this.say(said, cx);
                }
            }
            this.copy_next_large(cx);
        };
        match kind {
            FileKind::Model => self.copy_in_looked_at(kind, path, |path, meter| model_framing(path, meter).map(Found::Model), finished, cx),
            _ => self.copy_in_looked_at(kind, path, |path, _| video_shape(path).map(Found::Video), finished, cx),
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
        let finished = move |this: &mut Self, copied: Result<(String, ()), NotCopied>, cx: &mut Context<Self>| match copied {
            Ok((path, ())) => then(this, path, cx),
            Err(why) => this.say(why.said, cx),
        };
        self.copy_in_looked_at(kind, path, |_, _| Ok(()), finished, cx);
    }

    /// [`Self::copy_in_large_file`], looking at the file with `look` first, on the same
    /// background task, before anything is copied, with the copy's meter to tell how far it's got
    /// and to see whether it's been stopped: what it finds is handed to `finished` with the path,
    /// or why the file wasn't copied: `look` refused it, or the copy was refused or cancelled.
    /// Whatever comes of it, `finished` hears of it once, unless the window has gone.
    fn copy_in_looked_at<Found: Send + 'static>(
        &mut self,
        kind: FileKind,
        path: PathBuf,
        look: impl FnOnce(&Path, &Arc<Meter>) -> Result<Found, String> + Send + 'static,
        finished: impl FnOnce(&mut Self, Result<(String, Found), NotCopied>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let looked_at = path.clone();
        let suggested = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let copy = match document.read(cx).large_copy(kind, &suggested, LargeSource::File(path), None) {
            Ok(copy) => copy,
            Err(why) => return finished(self, Err(NotCopied { said: why.to_string(), cancelled: false }), cx),
        };
        let meter = self.start_progress(copying(kind.noun()), true, cx);
        let running = meter.clone();
        let start = move |_: &mut Self, cx: &mut Context<Self>| cx.spawn(async move |this, cx| {
            let copied = cx
                .background_spawn(async move {
                    // Refused, the copy is dropped, and with it the room kept for it.
                    let found = look(&looked_at, &running)?;
                    Ok::<_, String>((found, copy.run(&|done, total| running.tell(done, total), running.stop())))
                })
                .await;
            this.update(cx, |this, cx| {
                // Stopped, by its Cancel or the window closing, it counts as cancelled however it
                // came back: nothing waiting after it is copied.
                let stopped = meter.stop.load(std::sync::atomic::Ordering::SeqCst);
                this.end_progress(&meter, cx);
                let (found, copied) = match copied {
                    Ok(copied) => copied,
                    Err(said) => return finished(this, Err(NotCopied { said, cancelled: stopped }), cx),
                };
                let taken = document.update(cx, |document, cx| {
                    let taken = document.take_large_copy(copied);
                    cx.notify();
                    taken
                });
                let copied = match taken {
                    Ok(path) => Ok((path, found)),
                    Err(why) => Err(NotCopied { said: why.to_string(), cancelled: stopped || why == NotAdded::Cancelled }),
                };
                finished(this, copied, cx);
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
        let noun = noun_of(&waiting.copies);
        let asked = ask_to_copy_of(waiting.copies.len(), bytes, noun);
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
                .title(format!("Copy the {noun}?"))
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
        let meter = self.start_progress(copying(noun_of(&copies)), true, cx);
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

/// Whether the file at `path` is named as a file of `kind` is.
fn is_kind(path: &Path, kind: FileKind) -> bool {
    let extension = path.extension().map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    extension.is_some_and(|extension| kind.extensions().contains(&extension.as_str()))
}

/// The view the 3D model in the file at `path`, outside the talk, is seen whole from, or why
/// it's refused: for a background task. A mesh's is found from its headers alone; a point
/// cloud's header doesn't say where its points lie, so a cloud is read whole, within the limits
/// a model is read with, `meter` told how far that's got. A file that isn't a model, or that no
/// computer could draw, as one damaged, too large or needing other files, is refused before
/// anything is copied, saying why. Stopped by `meter`, by its Cancel or the window closing, the
/// read stops, and it's cancelled.
fn model_framing(path: &Path, meter: &Arc<Meter>) -> Result<View, String> {
    let open = || {
        let file = std::fs::File::open(path).map_err(|_| NotAdded::Unreadable.to_string())?;
        let len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        let reader = Watched { inner: std::io::BufReader::new(file), meter: meter.clone(), len };
        Ok::<_, String>(Box::new(reader) as Box<dyn slides_model::ReadSeek>)
    };
    let said = |why: CantDraw| {
        if meter.stop().load(std::sync::atomic::Ordering::SeqCst) {
            return NotAdded::Cancelled.to_string();
        }
        match why {
            CantDraw::CouldNotRead => NotAdded::Unreadable.to_string(),
            why => why.to_string(),
        }
    };
    let facts = slides_model::facts(open()?).map_err(said)?;
    if facts.kind == ModelKind::Mesh {
        return Ok(View::framing(&facts));
    }
    let model = slides_model::read(open()?).map_err(said)?;
    Ok(View::framing(&model.facts))
}

/// A model's file read before it's copied in, telling `meter` how far the read has got, and
/// failing once `meter` is stopped.
struct Watched {
    inner: std::io::BufReader<std::fs::File>,
    meter: Arc<Meter>,
    len: u64,
}

impl Watched {
    fn stopped(&self) -> std::io::Result<()> {
        if self.meter.stop().load(std::sync::atomic::Ordering::SeqCst) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "stopped"));
        }
        Ok(())
    }
}

impl std::io::Read for Watched {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io::Seek as _;
        self.stopped()?;
        let read = self.inner.read(buf)?;
        if let Ok(at) = self.inner.stream_position() {
            self.meter.tell(at.min(self.len), self.len);
        }
        Ok(read)
    }
}

impl std::io::Seek for Watched {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.stopped()?;
        self.inner.seek(to)
    }
}

/// The size of the video in the file at `path`, outside the talk, from its headers alone, or why
/// it isn't one, as a file with no video track isn't: for a background task. A video whose size
/// can't be read, as one damaged or of a kind that isn't played, is still a video, and says why
/// once it's on the slide: it's given the room a video's put in.
fn video_shape(path: &Path) -> Result<(u32, u32), String> {
    let mut file = std::fs::File::open(path).map_err(|_| NotAdded::Unreadable.to_string())?;
    match slides_video::video_size(&mut file) {
        Ok(size) => Ok(size),
        // A file with no pictures in it is no video.
        Err(CantPlay::NotAVideo | CantPlay::NoPicture) => Err(CantPlay::NotAVideo.to_string()),
        Err(CantPlay::CouldNotRead) => Err(NotAdded::Unreadable.to_string()),
        Err(_) => Ok((crate::insert::VIDEO_ROOM.0 as u32, crate::insert::VIDEO_ROOM.1 as u32)),
    }
}

/// What the window says when a file wasn't added for `why`, dropped on the slide if `dropped`.
fn not_added(why: NotAdded, dropped: bool) -> String {
    match why {
        NotAdded::NotAPicture | NotAdded::NotThatKind if dropped => ONLY_PICTURES.to_string(),
        why => why.to_string(),
    }
}
