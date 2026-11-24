//! The Add-ons dialog: the one add-on, Slidev, and whether it's installed, with a way to
//! install it from a `.spaddon` file and to take it away.

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Status, SLIDEV_DESCRIPTION};

pub const ADDONS_DIALOG_ID: &str = "addons-dialog";
pub const ADDON_STATE_ID: &str = "addon-state";
pub const ADDON_INSTALL_ID: &str = "addon-install";
pub const ADDON_REMOVE_ID: &str = "addon-remove";
pub const ADDON_PROBLEM_ID: &str = "addon-problem";
const ADDON_NAME_ID: &str = "addon-name";

/// The extension of an add-on's file.
pub const PACKAGE_EXTENSION: &str = "spaddon";
/// What the dialog says while it installs.
const INSTALLING: &str = "Installing…";

/// What the dialog shows, kept while it's open.
pub struct AddOns {
    /// The app's data folder, which add-ons are kept in, if the system has one.
    app_folder: Option<PathBuf>,
    status: Status,
    /// Why the last install or removal didn't happen, or that one is going on.
    said: Option<SharedString>,
    busy: bool,
}

/// What the dialog's state line says of `status`.
pub fn state_words(status: &Status) -> String {
    match status {
        Status::NotInstalled => "Not installed".to_string(),
        Status::Installed(version) => format!("Installed, version {version}"),
        Status::Damaged(sentence) => format!("Damaged: {sentence}"),
    }
}

impl AddOns {
    pub fn new(app_folder: Option<PathBuf>) -> AddOns {
        let status = app_folder.as_deref().map_or(Status::NotInstalled, super::status);
        AddOns { app_folder, status, said: None, busy: false }
    }

    /// Asks which `.spaddon` file to install, and installs it off the window's thread.
    fn ask_to_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(app_folder) = self.app_folder.clone() else {
            self.said = Some("This system has no data folder to keep add-ons in.".into());
            cx.notify();
            return;
        };
        // The system's dialog can't be told to show only add-on files, so the choice is checked.
        let chosen = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Install".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let file = match chosen.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => paths.remove(0),
                Ok(Err(error)) => {
                    let problem = format!("Could not show the Open dialog: {error}");
                    this.update(cx, |this, cx| this.say(problem, cx)).ok();
                    return;
                }
                _ => return,
            };
            if !file.extension().is_some_and(|extension| extension.eq_ignore_ascii_case(PACKAGE_EXTENSION)) {
                this.update(cx, |this, cx| this.say("That isn't an add-on file: add-ons come as .spaddon files.", cx)).ok();
                return;
            }
            this.update(cx, |this, cx| {
                this.busy = true;
                this.say(INSTALLING, cx);
            })
            .ok();
            let installing = app_folder.clone();
            let installed = cx.background_spawn(async move { super::install_from_file(&installing, &file).map(|installed| installed.version) }).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match installed {
                    Ok(_) => this.said = None,
                    Err(problem) => this.said = Some(problem.into()),
                }
                this.status = super::status(&app_folder);
                cx.notify();
                cx.defer(crate::workspace_view::refresh_addon_status);
            })
            .ok();
        })
        .detach();
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(app_folder) = self.app_folder.clone() else { return };
        self.said = super::remove(&app_folder).err().map(Into::into);
        self.status = super::status(&app_folder);
        cx.notify();
        cx.defer(crate::workspace_view::refresh_addon_status);
    }

    fn say(&mut self, said: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.said = Some(said.into());
        cx.notify();
    }
}

impl Render for AddOns {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (can_install, can_remove) = match &self.status {
            Status::NotInstalled => (true, false),
            Status::Installed(_) => (true, true),
            Status::Damaged(_) => (false, true),
        };
        let busy = self.busy;
        v_flex()
            .id(ADDONS_DIALOG_ID)
            .test_support()
            .gap_3()
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(said(ADDON_NAME_ID, SLIDEV_DESCRIPTION))
                            .child(div().text_sm().text_color(theme.muted_foreground).child(said(ADDON_STATE_ID, state_words(&self.status)))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .when(can_remove, |this| {
                                this.child(
                                    Button::new(ADDON_REMOVE_ID)
                                        .small()
                                        .ghost()
                                        .label("Remove")
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
                                )
                            })
                            .when(can_install, |this| {
                                this.child(
                                    Button::new(ADDON_INSTALL_ID)
                                        .small()
                                        .label("Install from file…")
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, window, cx| this.ask_to_install(window, cx))),
                                )
                            }),
                    ),
            )
            .when_some(self.said.clone(), |this, said_now| {
                let colour = if busy { theme.muted_foreground } else { theme.danger };
                this.child(div().text_sm().text_color(colour).child(said(ADDON_PROBLEM_ID, said_now)))
            })
    }
}

/// Words shown, by `id`, with the same words as their name for anything that reads them out.
fn said(id: impl Into<ElementId>, words: impl Into<SharedString>) -> impl IntoElement {
    let words = words.into();
    div().id(id).test_support().aria_label(words.clone()).child(words)
}

/// Opens the Add-ons dialog in `window`, for the add-ons kept in `app_folder`.
pub fn open(app_folder: Option<PathBuf>, window: &mut Window, cx: &mut App) {
    let add_ons = cx.new(|_| AddOns::new(app_folder));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title("Add-ons").child(add_ons.clone()).footer(
            h_flex().justify_end().child(Button::new("addons-close").ghost().label("Close").on_click(|_, window, cx| window.close_dialog(cx))),
        )
    });
}
