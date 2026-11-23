//! The start screen in the window: opening a window on it, making a new presentation, opening
//! a talk and bringing one back from it, all in this window, and showing a talk in a window that
//! has none.

use slides_engine::PackError;

use super::*;

/// Opens a window with no talk, showing the start screen. It lists the new presentations kept in
/// `recovery` that weren't saved last time, if there's one, and keeps the working copies of
/// whatever is made or opened from it there.
pub fn open_start_window(recovery: Option<Recovery>, cx: &mut App) -> Result<()> {
    let options = WorkspaceView::window_options(cx);
    gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| WorkspaceView::for_start(recovery, window, cx)))?;
    cx.activate(true);
    Ok(())
}

/// The views of the app's windows that can be looked at now: every one but the window being
/// updated, if any.
fn open_views(cx: &App) -> Vec<Entity<WorkspaceView>> {
    cx.windows()
        .into_iter()
        .filter_map(|window| window.downcast::<gpui_kit::base::Root>())
        .filter_map(|window| window.read(cx).ok())
        .filter_map(|root| root.view().clone().downcast::<WorkspaceView>().ok())
        .collect()
}

/// The new presentations open in the app's windows, but the one being updated, if any.
pub fn open_untitled_ids(cx: &App) -> Vec<UntitledId> {
    open_views(cx)
        .iter()
        .filter_map(|view| view.read(cx).document.as_ref())
        .filter_map(|document| match document.read(cx).place() {
            TalkPlace::Untitled(id) => Some(*id),
            TalkPlace::Folder(_) | TalkPlace::Packed(_) => None,
        })
        .collect()
}

/// The lowest number not in use among the new presentations in the app's windows, from 1.
fn free_untitled_number(cx: &App) -> usize {
    let taken: Vec<usize> = open_views(cx)
        .iter()
        .filter_map(|view| view.read(cx).document.as_ref())
        .filter_map(|document| document.read(cx).untitled_number())
        .collect();
    (1..).find(|number| !taken.contains(number)).unwrap_or(1)
}

/// Has every start screen read the presentations from last time again, as one has been brought
/// back or let go.
pub fn refresh_start_screens(cx: &mut App) {
    for view in open_views(cx) {
        if let Some(start) = view.read(cx).start_screen.clone() {
            start.update(cx, |start, cx| start.refresh(cx));
        }
    }
}

/// What a window is set up with besides its talk: kept as a talk is shown in it, and given to
/// a window opened from it.
#[derive(Clone)]
struct Setup {
    recovery: Option<Recovery>,
    compile: Option<Compile>,
    bundle: Option<Bundle>,
    editor_projects: Option<PathBuf>,
    /// The file the window's build permissions are kept in, read again for each window, as
    /// another may have changed it.
    permissions: Option<PathBuf>,
    /// The same for the commands always allowed to run.
    run_permissions: Option<PathBuf>,
    documents: Option<PathBuf>,
    really_watch: bool,
    stagepack: Option<Option<PathBuf>>,
}

impl WorkspaceView {
    /// The window with no talk, showing the start screen, which lists the new presentations
    /// kept in `recovery` that weren't saved last time. Its first tile has the keyboard.
    pub fn for_start(recovery: Option<Recovery>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::build(None, None, window, cx);
        if let Some(recovery) = recovery.clone() {
            view.use_recovery(recovery, cx);
        }
        let start = cx.new(|cx| StartScreen::new(recovery, window, cx));
        start.update(cx, |start, cx| start.take_keyboard(window, cx));
        view._start_asked = Some(cx.subscribe_in(&start, window, |this, _, asked: &StartEvent, window, cx| {
            this.start_asked(asked.clone(), window, cx)
        }));
        view.start_screen = Some(start);
        view
    }

    /// The start screen, while the window shows it.
    pub fn start_screen(&self) -> Option<&Entity<StartScreen>> {
        self.start_screen.as_ref().filter(|_| self.document.is_none())
    }

    /// Puts `document` in this window, which has no talk, as [`Self::open_with`] would have
    /// built it: its panels, its tools, its working copies, and for a folder its watcher. The
    /// window keeps how it was set up. A talk with a place is one opened from there, as
    /// [`Self::open`] has it. A window that has a talk already keeps it.
    pub fn show(&mut self, document: Document, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_some() {
            return;
        }
        let opened = !document.is_untitled();
        let folder = talk_folder_of(document.place());
        let view = Self::open_with(document, window, cx);
        self.take_place_of(view, window, cx);
        self.opened_from_path = opened;
        if let Some(folder) = folder {
            self.follow_saves(folder, cx);
        }
    }

    /// Becomes `view`, built for this window, keeping how this one was set up. The start
    /// screen's pictures are let go of, as nothing here shows them now.
    fn take_place_of(&mut self, view: WorkspaceView, window: &mut Window, cx: &mut Context<Self>) {
        let setup = self.setup();
        let old = std::mem::replace(self, view);
        self.set_up(setup, cx);
        if old.start_screen.is_some() {
            for picture in start_screen::tile_pictures(cx) {
                window.drop_image(picture).ok();
            }
        }
        cx.notify();
    }

    fn setup(&self) -> Setup {
        Setup {
            recovery: self.recovery.clone(),
            compile: self.compile.clone(),
            bundle: self.bundle.clone(),
            editor_projects: self.editor_projects.clone(),
            permissions: self.permissions.as_ref().map(|permissions| permissions.file().to_path_buf()),
            run_permissions: self.run_permissions.as_ref().map(|permissions| permissions.file().to_path_buf()),
            documents: self.documents.clone(),
            really_watch: self.really_watch,
            stagepack: self.stagepack.clone(),
        }
    }

    /// Sets the window up as `setup` says. Its working copies are kept where `setup` has
    /// them, unless they're kept somewhere already.
    fn set_up(&mut self, setup: Setup, cx: &mut Context<Self>) {
        self.compile = setup.compile;
        self.bundle = setup.bundle;
        self.editor_projects = setup.editor_projects;
        self.permissions = setup.permissions.map(BuildPermissions::load);
        self.always_build = self.is_always_built();
        self.run_permissions = setup.run_permissions.map(RunPermissions::load);
        self.documents = setup.documents;
        self.really_watch = setup.really_watch;
        self.stagepack = setup.stagepack;
        if let Some(recovery) = setup.recovery {
            if self.working_copies.read(cx).recovery().is_none() {
                self.use_recovery(recovery, cx);
            } else {
                self.recovery = Some(recovery);
            }
        }
    }

    /// Whether the window shows the start screen.
    pub(super) fn shows_start_screen(&self) -> bool {
        self.start_screen().is_some()
    }

    /// Does what the start screen asks.
    fn start_asked(&mut self, asked: StartEvent, window: &mut Window, cx: &mut Context<Self>) {
        match asked {
            StartEvent::Make(template) => self.make(template, window, cx),
            StartEvent::OpenFile => self.ask_to_open(false, window, cx),
            StartEvent::OpenFolder => self.ask_to_open(true, window, cx),
            StartEvent::BringBack(id) => self.bring_back_from_start(id, window, cx),
            StartEvent::OpenInStagepack => self.open_in_stagepack(cx),
        }
    }

    /// Makes a new presentation from the template known as `template` in this window, numbered
    /// the lowest not in use.
    fn make(&mut self, template: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(template) = templates::template(template) else {
            return;
        };
        match Document::new_untitled(template, UntitledId::random(), Fonts::with_system_fallback()) {
            Ok(mut document) => {
                document.set_untitled_number(free_untitled_number(cx));
                self.show(document, window, cx);
            }
            Err(problem) => self.say_on_start(problem, false, cx),
        }
    }

    /// Opens the talk at `path` in this window, from the start screen. If it can't be opened,
    /// the start screen says why.
    pub(super) fn open_here(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = std::path::absolute(&path).unwrap_or(path);
        self.stagepack_offer = None;
        match read_talk(&path, Fonts::with_system_fallback()) {
            Ok(document) => self.show(document, window, cx),
            Err(OpenProblem::SlidevTalk) => {
                let stagepack = self.stagepack.clone().unwrap_or_else(installed_stagepack);
                self.stagepack_offer = stagepack.map(|executable| StagepackOffer { file: path, executable });
                let offers = self.stagepack_offer.is_some();
                self.say_on_start(SLIDEV_TALK, offers, cx);
            }
            Err(OpenProblem::Other(_)) => self.say_on_start(start_problem(&path), false, cx),
        }
    }

    /// Brings back the new presentation `id` from last time into this window, taking its copy
    /// off the window's thread.
    fn bring_back_from_start(&mut self, id: UntitledId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(recovery) = self.start_screen().and_then(|start| start.read(cx).recovery().cloned()) else {
            return;
        };
        // Its old copy is still there for a moment after another window brings it back.
        if open_untitled_ids(cx).contains(&id) {
            self.say_on_start(NotTaken::Held.to_string(), false, cx);
            // Once every window can be looked at, this one with it.
            cx.defer(refresh_start_screens);
            return;
        }
        let taking = recovery.clone();
        cx.spawn_in(window, async move |this, cx| {
            let taken = cx.background_spawn(async move { taking.take_untitled(id) }).await;
            this.update_in(cx, |this, window, cx| this.bring_back_taken(id, taken, recovery, window, cx))
                .ok();
        })
        .detach();
    }

    fn bring_back_taken(
        &mut self,
        id: UntitledId,
        taken: Result<Found, NotTaken>,
        recovery: Recovery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.shows_start_screen() {
            // Something else was shown here in the meantime. The copy stays where it is.
            return;
        }
        // Another start screen may have brought it back while this one took it: nothing holds
        // a copy that is only being read.
        let taken = taken.and_then(|found| match open_untitled_ids(cx).contains(&id) {
            true => Err(NotTaken::Held),
            false => Ok(found),
        });
        let found = match taken {
            Ok(found) => found,
            // Still there, for Discard to let go of.
            Err(NotTaken::Unreadable) => return self.say_on_start(NotTaken::Unreadable.to_string(), false, cx),
            // Gone, or open elsewhere: it comes off the list.
            Err(not_taken) => {
                self.say_on_start(not_taken.to_string(), false, cx);
                cx.defer(refresh_start_screens);
                return;
            }
        };
        let number = free_untitled_number(cx);
        match Self::bring_back_untitled(id, found, number, Fonts::with_system_fallback(), recovery, window, cx) {
            Ok(view) => {
                self.take_place_of(view, window, cx);
                // Once every window can be looked at, this one with it.
                cx.defer(refresh_start_screens);
            }
            // What it says is about the text inside, which isn't for the start screen.
            Err(_) => self.say_on_start(NotTaken::Unreadable.to_string(), false, cx),
        }
    }

    /// Says `problem` on the start screen, under its buttons, offering to open the file in
    /// Stagepack too if `offers_stagepack`.
    pub(super) fn say_on_start(&mut self, problem: impl Into<SharedString>, offers_stagepack: bool, cx: &mut Context<Self>) {
        let problem = problem.into();
        if let Some(start) = self.start_screen().cloned() {
            start.update(cx, |start, cx| start.set_problem(Some(problem), offers_stagepack, cx));
        } else {
            self.say(problem, cx);
        }
    }

    /// Opens a start screen in a window of its own, set up as this one is.
    pub(super) fn new_presentation(&mut self, cx: &mut Context<Self>) {
        let setup = self.setup();
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            let options = WorkspaceView::window_options(cx);
            let opened = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| {
                    let mut view = WorkspaceView::for_start(setup.recovery.clone(), window, cx);
                    view.set_up(setup, cx);
                    view
                })
            });
            if let Err(error) = opened {
                let problem = format!("Could not open a window for a new presentation: {error}");
                this.update(cx, |this, cx| this.say_on_start(problem, false, cx)).ok();
            }
        });
    }

    /// The window while it shows the start screen: the title bar, with the File menu, and the
    /// screen under it.
    pub(super) fn render_start(&self, start: Entity<StartScreen>, cx: &mut Context<Self>) -> impl IntoElement {
        let title: SharedString = "Stagepack Studio".into();
        let muted_text = cx.theme().muted_foreground;
        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &NewPresentation, _, cx| this.new_presentation(cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, window, cx| this.ask_to_open(true, window, cx)))
            .on_action(cx.listener(|this, _: &OpenPacked, window, cx| this.ask_to_open(false, window, cx)))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(file_menu(None, false))
                        .child(export_menu(false))
                        .child(
                            div()
                                .id(TITLE_ID)
                                .test_support()
                                .aria_label(title.clone())
                                .ml_4()
                                .text_sm()
                                .child(title),
                        ),
                ),
            )
            .child(div().flex_1().min_h_0().w_full().child(start))
            .when_some(self.status.clone(), |this, status| {
                this.child(footer_line(STATUS_ID, status, muted_text))
            })
    }
}

/// Why the talk at `path` couldn't be opened, as the start screen and a window opened on it say
/// it: one plain sentence for each kind of failure, with no path, file name or detail from
/// inside the file. A file is read again to tell which kind it is, as only a failure comes here.
pub(super) fn start_problem(path: &Path) -> &'static str {
    if path.is_dir() {
        return if path.join(SLIDES_FILE).is_file() {
            "That folder's presentation couldn't be read."
        } else {
            "That folder has no presentation in it."
        };
    }
    if !path.exists() {
        return "That file isn't there any more.";
    }
    // A packed presentation is a zip file, which starts so.
    let mut start = [0u8; 4];
    match std::fs::File::open(path).and_then(|mut file| std::io::Read::read_exact(&mut file, &mut start)) {
        Ok(()) if start == *b"PK\x03\x04" => {}
        Err(error) if error.kind() != std::io::ErrorKind::UnexpectedEof => return "That file couldn't be read.",
        _ => return NOT_A_PRESENTATION,
    }
    match slides_engine::open_packed(path).and_then(|pack| pack.slides()) {
        Err(PackError::NotATalk) => NOT_A_PRESENTATION,
        Err(PackError::TooNew { .. }) => "That presentation was made by a newer version of this app. Update the app to open it.",
        Err(PackError::TooLarge(_)) => "That presentation is too large to open.",
        Err(PackError::Io(_)) => "That file couldn't be read.",
        // Damaged, in one way or another, or holding something that can't be read.
        _ => "That file is damaged, so it can't be opened.",
    }
}

const NOT_A_PRESENTATION: &str = "That file isn't a presentation made with this app.";
