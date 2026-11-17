//! Putting things on the slide from the window: pictures chosen in the system's dialog, dropped
//! on the slide or pasted, shapes and text boxes from the Insert tab.

use super::*;
use crate::added_files::{MAX_ADDED_BYTES, NotAdded};
use crate::clipboard_tools::ClipboardThing;
use crate::document::InsertFailed;
use crate::insert::{ICON_SIZE, INSERT_ICON};

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

    /// Gives the slide the keyboard once a thing just put on it is selected there, from
    /// wherever it was, so that the next keys type in it or take it away. Left in the list of
    /// slides, Delete would take the whole slide away.
    pub(super) fn keyboard_to_the_slide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected_here = self.document.as_ref().is_some_and(|document| {
            let document = document.read(cx);
            document.selected().is_some_and(|selected| selected.slide == document.current())
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
