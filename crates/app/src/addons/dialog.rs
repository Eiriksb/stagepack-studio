//! The Add-ons dialog: the one add-on, Slidev, and whether it's installed, with a way to get it
//! from its releases, to check for a later version and update to it, to install it from a
//! `.spaddon` file and to take it away. Nothing is asked of the network but on a click of *Get*,
//! *Check for updates* or *Update*, and that work runs on a thread of its own, telling the dialog
//! how far it has got, until it's done or cancelled.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::fetch::{self, Index};
use super::{Status, BUSY, SLIDEV_DESCRIPTION};

pub const ADDONS_DIALOG_ID: &str = "addons-dialog";
pub const ADDON_STATE_ID: &str = "addon-state";
pub const ADDON_INSTALL_ID: &str = "addon-install";
pub const ADDON_REMOVE_ID: &str = "addon-remove";
pub const ADDON_PROBLEM_ID: &str = "addon-problem";
pub const ADDON_GET_ID: &str = "addon-get";
pub const ADDON_CHECK_ID: &str = "addon-check";
pub const ADDON_UPDATE_ID: &str = "addon-update";
pub const ADDON_CANCEL_ID: &str = "addon-cancel";
/// The line saying how far a Get, a check or an update has got.
pub const ADDON_PROGRESS_ID: &str = "addon-progress";
/// The line saying what a check found.
pub const ADDON_NEWS_ID: &str = "addon-news";
const ADDON_NAME_ID: &str = "addon-name";

/// The extension of an add-on's file.
pub const PACKAGE_EXTENSION: &str = "spaddon";
/// What the dialog says while it installs.
const INSTALLING: &str = "Installing…";
/// How often the dialog looks at how far the work it started has got.
pub const FETCH_TICK: Duration = Duration::from_millis(100);
/// What the dialog says while it asks for the add-on's index.
const CHECKING: &str = "Checking…";
/// What a check says when nothing later than what's installed is on offer.
pub const NEWEST: &str = "This is the newest version";

/// What a check says of a later `version` on offer.
pub fn available_words(version: &str) -> String {
    format!("Version {version} is available")
}

/// What the dialog says when `done` bytes of `total` are downloaded.
pub fn downloading_words(done: u64, total: u64) -> String {
    format!("Downloading… {} of {} MB", megabytes(done), megabytes(total))
}

/// `bytes` in megabytes, to one place, a little being never nothing.
fn megabytes(bytes: u64) -> String {
    if bytes > 0 && bytes < 100_000 { "0.1".to_string() } else { format!("{:.1}", bytes as f64 / 1e6) }
}

/// The downloads had, by the app folder they were for, for the tests that pin when they're let go.
#[cfg(feature = "test-hooks")]
static DOWNLOADS: std::sync::Mutex<Vec<(PathBuf, std::sync::Weak<Vec<u8>>)>> = std::sync::Mutex::new(Vec::new());

/// How many of the downloads had for `app_folder` are still held, by a dialog keeping one or a
/// job going on.
#[cfg(feature = "test-hooks")]
pub fn downloads_held_for_test(app_folder: &std::path::Path) -> usize {
    let mut downloads = DOWNLOADS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    downloads.retain(|(_, download)| download.strong_count() > 0);
    downloads.iter().filter(|(folder, _)| folder == app_folder).count()
}

/// What the dialog shows, kept while it's open.
pub struct AddOns {
    /// The app's data folder, which add-ons are kept in, if the system has one.
    app_folder: Option<PathBuf>,
    status: Status,
    /// Why the last install or removal didn't happen, or that one is going on.
    said: Option<SharedString>,
    busy: bool,
    /// What the last check found: a later version on offer, or that there is none.
    news: Option<SharedString>,
    /// The later version on offer, which *Update* gets.
    offered: Option<Index>,
    /// The Get, check or update going on, if one is.
    fetching: Option<Fetching>,
    /// A release downloaded already, kept as the add-on was busy when it was to be installed.
    kept: Option<(Index, Arc<Vec<u8>>)>,
}

/// A Get, a check or an update going on, on a thread of its own.
struct Fetching {
    /// Set to stop it.
    cancel: Arc<AtomicBool>,
    /// What the row says of it now.
    progress: SharedString,
    /// What the row said before it began, which *Cancel* puts back.
    said: Option<SharedString>,
    news: Option<SharedString>,
    offered: Option<Index>,
}

/// What is asked of the add-on's releases.
enum Job {
    /// The release the index of the app's line names, downloaded and installed.
    Get,
    /// Whether the release the index names is later than what's installed.
    Check,
    /// The release a check found, downloaded and installed; or installed from the bytes kept
    /// of it, downloaded already, when an export or an import held the add-on last time.
    Update(Index, Option<Arc<Vec<u8>>>),
}

/// What the thread doing a [`Job`] tells the dialog.
enum Told {
    Progress(u64, u64),
    Installing,
    Checked(Index),
    Installed,
    Failed(String),
    /// The download of the release an Update was for was had, but an export or an import held
    /// the add-on, so it is kept for the next *Update*.
    Busy(Index, Arc<Vec<u8>>),
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
        AddOns { app_folder, status, said: None, busy: false, news: None, offered: None, fetching: None, kept: None }
    }

    /// Starts `job` on a thread of its own, the row saying how far it has got.
    fn start(&mut self, job: Job, cx: &mut Context<Self>) {
        if self.fetching.is_some() || self.busy {
            return;
        }
        let Some(app_folder) = self.app_folder.clone() else {
            self.say("This system has no data folder to keep add-ons in.", cx);
            return;
        };
        let progress = match &job {
            Job::Update(_, Some(_)) => INSTALLING.to_string(),
            Job::Update(index, None) => downloading_words(0, index.size),
            Job::Get | Job::Check => CHECKING.to_string(),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let (tell, told) = mpsc::channel();
        // Asked for here, on the click, and nowhere else.
        let source = fetch::source();
        let stop = cancel.clone();
        let spawned = std::thread::Builder::new().name("addon-get".into()).spawn(move || {
            let last = run(job, &app_folder, &source, &stop, &tell);
            let _ = tell.send(last);
        });
        if spawned.is_err() {
            self.say("The download couldn't be started.", cx);
            return;
        }
        self.fetching = Some(Fetching { cancel: cancel.clone(), progress: progress.into(), said: self.said.take(), news: self.news.take(), offered: self.offered.clone() });
        // Looked at now and then, until the thread is done, even once cancelled: one that got as
        // far as installing has changed what's installed.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FETCH_TICK).await;
                let mut said = Vec::new();
                let ended = loop {
                    match told.try_recv() {
                        Ok(one) => said.push(one),
                        Err(mpsc::TryRecvError::Empty) => break false,
                        Err(mpsc::TryRecvError::Disconnected) => break true,
                    }
                };
                let taken = this.update(cx, |this, cx| {
                    for one in said {
                        this.told(&cancel, one, cx);
                    }
                });
                if ended || taken.is_err() {
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Takes in what the thread doing the job stopped by `cancel` said.
    fn told(&mut self, cancel: &Arc<AtomicBool>, said: Told, cx: &mut Context<Self>) {
        let Some(fetching) = self.fetching.as_mut().filter(|fetching| Arc::ptr_eq(&fetching.cancel, cancel)) else {
            // A cancelled one that got as far as installing: what's installed is looked at again.
            if matches!(said, Told::Installed) {
                self.look_again(cx);
            }
            return;
        };
        match said {
            Told::Progress(done, total) => fetching.progress = downloading_words(done, total).into(),
            Told::Installing => fetching.progress = INSTALLING.into(),
            Told::Checked(index) => {
                self.fetching = None;
                (self.news, self.offered) = match &self.status {
                    Status::Installed(installed) if fetch::newer(installed, &index.version) => (Some(available_words(&index.version).into()), Some(index)),
                    Status::Installed(_) => (Some(NEWEST.into()), None),
                    Status::NotInstalled | Status::Damaged(_) => (None, None),
                };
                // A download kept of another release than the one on offer now is no use.
                if self.kept.as_ref().is_some_and(|(kept, _)| self.offered.as_ref() != Some(kept)) {
                    self.kept = None;
                }
            }
            Told::Installed => {
                self.fetching = None;
                (self.said, self.news, self.offered, self.kept) = (None, None, None, None);
                self.look_again(cx);
            }
            Told::Failed(sentence) => {
                if let Some(before) = self.fetching.take() {
                    (self.news, self.offered) = (before.news, before.offered);
                }
                self.said = Some(sentence.replace(['\n', '\r'], " ").into());
                // Another window may have installed meanwhile.
                self.look_again(cx);
            }
            // Only an Update says this: it is the only job that would use what's kept.
            Told::Busy(index, bytes) => {
                if let Some(before) = self.fetching.take() {
                    (self.news, self.offered) = (before.news, before.offered);
                }
                self.said = Some(BUSY.into());
                self.kept = Some((index, bytes));
            }
        }
        cx.notify();
    }

    /// Stops the job going on, and puts the row back as it was before it.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(before) = self.fetching.take() else { return };
        before.cancel.store(true, Ordering::SeqCst);
        (self.said, self.news, self.offered) = (before.said, before.news, before.offered);
        cx.notify();
    }

    /// Looks again at what's installed, here and in every window.
    fn look_again(&mut self, cx: &mut Context<Self>) {
        if let Some(app_folder) = &self.app_folder {
            self.status = super::status(app_folder);
        }
        // An update on offer is no longer one once what's installed is as new.
        let still_later = |index: &Index| matches!(&self.status, Status::Installed(installed) if fetch::newer(installed, &index.version));
        if self.offered.as_ref().is_some_and(|index| !still_later(index)) {
            (self.news, self.offered) = (None, None);
        }
        if self.kept.as_ref().is_some_and(|(index, _)| !still_later(index)) {
            self.kept = None;
        }
        cx.notify();
        cx.defer(crate::workspace_view::refresh_addon_status);
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
                (this.news, this.offered, this.kept) = (None, None, None);
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
        (self.news, self.offered, self.kept) = (None, None, None);
        self.status = super::status(&app_folder);
        cx.notify();
        cx.defer(crate::workspace_view::refresh_addon_status);
    }

    fn say(&mut self, said: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.said = Some(said.into());
        cx.notify();
    }
}

impl Drop for AddOns {
    /// Closing the dialog stops what it was getting.
    fn drop(&mut self) {
        if let Some(fetching) = &self.fetching {
            fetching.cancel.store(true, Ordering::SeqCst);
        }
    }
}

/// Does `job` for the add-on under `app_folder`, from `source`, telling `tell` how far it has got,
/// until `cancel` is set; and gives what it came to.
fn run(job: Job, app_folder: &std::path::Path, source: &fetch::Source, cancel: &AtomicBool, tell: &mpsc::Sender<Told>) -> Told {
    // Only an Update keeps a download the add-on was too busy for: a Get never looks for one.
    let keeps = matches!(job, Job::Update(..));
    let (index, kept) = match job {
        Job::Check => return fetch::check(source, cancel).map_or_else(Told::Failed, Told::Checked),
        Job::Get => match fetch::check(source, cancel) {
            Ok(index) => (index, None),
            Err(sentence) => return Told::Failed(sentence),
        },
        Job::Update(index, kept) => {
            // Said before anything is downloaded, rather than after.
            if super::is_busy(app_folder) {
                return match kept {
                    Some(bytes) => Told::Busy(index, bytes),
                    None => Told::Failed(BUSY.to_string()),
                };
            }
            (index, kept)
        }
    };
    // Never one that isn't later than what is installed by now, from another window or a file:
    // looked at first, so as not to download what would be refused, and again under the lock.
    if let Status::Installed(installed) = super::status(app_folder)
        && !fetch::newer(&installed, &index.version)
    {
        return Told::Failed(format!("Version {installed} is installed already, which is no older than the one on offer."));
    }
    let bytes = match kept {
        Some(bytes) => bytes,
        None => {
            let _ = tell.send(Told::Progress(0, index.size));
            let progress = |done, total| {
                let _ = tell.send(Told::Progress(done, total));
            };
            match fetch::download(source, &index, cancel, &progress) {
                Ok(bytes) => {
                    let bytes = Arc::new(bytes);
                    #[cfg(feature = "test-hooks")]
                    DOWNLOADS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((app_folder.to_path_buf(), Arc::downgrade(&bytes)));
                    bytes
                }
                Err(sentence) => return Told::Failed(sentence),
            }
        }
    };
    if cancel.load(Ordering::SeqCst) {
        return Told::Failed(fetch::CANCELLED.to_string());
    }
    let _ = tell.send(Told::Installing);
    match super::update_from_bytes(app_folder, &bytes) {
        Ok(_) => Told::Installed,
        Err(sentence) if sentence == BUSY && keeps => Told::Busy(index, bytes),
        Err(sentence) => Told::Failed(sentence),
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
        let fetching = self.fetching.as_ref().map(|fetching| fetching.progress.clone());
        let running = fetching.is_some();
        let installed = matches!(self.status, Status::Installed(_));
        let not_installed = matches!(self.status, Status::NotInstalled);
        // A damaged install, as an app update to another Slidev leaves one, is replaced by Get.
        let damaged = matches!(self.status, Status::Damaged(_));
        let can_update = !running && installed && self.offered.is_some();
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
                            .when(running, |this| {
                                this.child(
                                    Button::new(ADDON_CANCEL_ID).small().label("Cancel").on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                                )
                            })
                            .when(!running && installed, |this| {
                                this.child(
                                    Button::new(ADDON_CHECK_ID)
                                        .small()
                                        .ghost()
                                        .label("Check for updates")
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.start(Job::Check, cx))),
                                )
                            })
                            .when(!running && can_remove, |this| {
                                this.child(
                                    Button::new(ADDON_REMOVE_ID)
                                        .small()
                                        .ghost()
                                        .label("Remove")
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
                                )
                            })
                            .when(!running && can_install, |this| {
                                this.child(
                                    Button::new(ADDON_INSTALL_ID)
                                        .small()
                                        .when(!not_installed, |this| this.ghost())
                                        .label("Install from file…")
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, window, cx| this.ask_to_install(window, cx))),
                                )
                            })
                            .when(!running && (not_installed || damaged), |this| {
                                this.child(
                                    Button::new(ADDON_GET_ID)
                                        .small()
                                        .primary()
                                        .label(if damaged { "Get again" } else { "Get" })
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.start(Job::Get, cx))),
                                )
                            }),
                    ),
            )
            .when_some(fetching, |this, progress| {
                this.child(div().text_sm().text_color(theme.muted_foreground).child(said(ADDON_PROGRESS_ID, progress)))
            })
            .when_some(self.news.clone().filter(|_| !running), |this, news| {
                this.child(
                    h_flex()
                        .gap_2()
                        .justify_between()
                        .child(div().text_sm().child(said(ADDON_NEWS_ID, news)))
                        .when(can_update, |this| {
                            this.child(Button::new(ADDON_UPDATE_ID).small().primary().label("Update").disabled(busy).on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(index) = this.offered.clone() {
                                        // What was downloaded already of it, if the add-on was busy.
                                        let kept = this.kept.as_ref().filter(|(kept, _)| *kept == index).map(|(_, bytes)| bytes.clone());
                                        this.start(Job::Update(index, kept), cx);
                                    }
                                },
                            )))
                        }),
                )
            })
            .when_some(self.said.clone().filter(|_| !running), |this, said_now| {
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
