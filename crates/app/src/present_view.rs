//! A show's windows: the slides for the audience, and the presenter's view of them.

use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::{ActiveTheme as _, TitleBar, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{ButtonLayer, Enter, LayerTiming, ModelLayer, Moment, Rect, TerminalLayer, Transition, VideoLayer};

use crate::document::{Document, LargeOpen};
use crate::interactive::{Acted, InteractiveKind, InteractiveThing, Press, interactive_at, interactive_on, press_button};
use crate::present::{
    ENTER_SECONDS, MOMENTS_PER_SECOND, Moved, Place, ScreenInfo, Session, TRANSITION_SECONDS,
    WindowPlan, choose_windows, clock_text, layer_look, moment_time, slide_looks,
};
use crate::live_terminal::{LiveId, LiveTerminals, gives_keys_back};
use crate::model_view::{Aim, aim_at};
use crate::replay::{Aim as ReplayAim, MOST_PLAYING, Replay, Source as ReplaySource};
use crate::slide_canvas::{LivePlace, ModelLetGo, ScaleChanged, SlideCanvas, VideoLetGo, VideoOpened};
use crate::workspace_view::{APP_ID, LiveStep, RunPressed, WorkspaceView};

pub const AUDIENCE_ID: &str = "audience";
pub const CURRENT_ID: &str = "current-slide";
pub const NEXT_ID: &str = "next-slide";
pub const NOTES_ID: &str = "notes";
pub const TIMER_ID: &str = "timer";
pub const COUNTER_ID: &str = "counter";
/// The faint line around the interactive thing under the pointer.
pub const INTERACTIVE_HOVER_ID: &str = "interactive-hover";

pub use crate::interactive::PressedIn;
#[cfg(feature = "test-hooks")]
pub use crate::interactive::StandInPress;

/// How long the pointer stays still on a slide with nothing to press before it hides.
pub const POINTER_HIDES_AFTER: Duration = Duration::from_secs(2);

/// How strong the line around an interactive thing under the pointer is: the talk's accent, faint.
const HOVER_OPACITY: f32 = 0.4;

/// The shade laid over a button while the pointer is down on it: black at this strength makes it
/// darker by 15 %. It's drawn in the window only, never into the slide's bitmap.
pub const BUTTON_PRESSED_ID: &str = "button-pressed";
pub const PRESSED_DARKER: f32 = 0.15;
/// How round a button's corners are, in slide units, as the engine draws them.
const BUTTON_RADIUS: f32 = 8.0;

const KEY_CONTEXT: &str = "Show";

/// The key context of a show's window while a live terminal has the keys: none of the show's keys
/// apply, so every key goes to the program.
pub const KEYS_HELD_CONTEXT: &str = "ShowTerminal";

/// The window whose talk a show belongs to, and the live terminals it shares.
pub type Owner = (WeakEntity<WorkspaceView>, LiveTerminals);

const END_OF_THE_TALK: &str = "End of the talk";

/// Where a rehearsal goes if there's no telling how big the screen is.
const SCREEN_UNKNOWN: Size<Pixels> = size(px(1600.), px(900.));

const CLOCK_TICK: Duration = Duration::from_secs(1);

actions!(stagepack_studio, [ShowNext, ShowPrevious, EndShow]);

/// A show's keys, which work in either of its windows.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", ShowNext, Some(KEY_CONTEXT)),
        KeyBinding::new("pagedown", ShowNext, Some(KEY_CONTEXT)),
        KeyBinding::new("space", ShowNext, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", ShowNext, Some(KEY_CONTEXT)),
        KeyBinding::new("left", ShowPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("pageup", ShowPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("backspace", ShowPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", EndShow, Some(KEY_CONTEXT)),
    ]);
}

/// A click's layers coming in.
#[derive(Clone, Copy)]
struct Reveal {
    step: u32,
    began: Instant,
}

/// A slide being arrived at.
#[derive(Clone, Copy)]
struct Arrival {
    left: Place,
    /// The time the slide left was at, which it stays at as it goes.
    left_time: f32,
    transition: Transition,
    /// When its first frame was drawn. Until then none of it has been seen, however long
    /// getting the slide ready takes.
    began: Option<Instant>,
}

/// One of a show's windows, and what's in it.
struct ShowWindow<V: 'static> {
    window: AnyWindowHandle,
    view: WeakEntity<V>,
}

impl<V: 'static> ShowWindow<V> {
    fn opened((window, view): (AnyWindowHandle, Entity<V>)) -> Self {
        ShowWindow {
            window,
            view: view.downgrade(),
        }
    }

    fn get(&self) -> Option<(AnyWindowHandle, Entity<V>)> {
        Some((self.window, self.view.upgrade()?))
    }
}

/// A running show: where it is, and what's moving. Its windows draw from this.
pub struct Presentation {
    document: Entity<Document>,
    session: Session,
    started_at: Instant,
    reveal: Option<Reveal>,
    arrival: Option<Arrival>,
    /// When the slide the show is on was first drawn. Until then its time hasn't started.
    slide_began: Option<Instant>,
    audience: Option<ShowWindow<AudienceView>>,
    presenter: Option<ShowWindow<PresenterView>>,
    ended: bool,
    /// The window of the talk, if it's one that runs live terminals, and what it runs.
    owner: Option<Owner>,
    /// Whether a question about running a command is on its way to a window.
    ask_pending: bool,
    /// How many times the show has come to a slide: a button's press waiting for a yes is for the
    /// visit it was made in, and none after.
    visits: u64,
    /// A press on an interactive thing, by where its tag is written, and the window it began in,
    /// which has it until it's let go of there.
    pressed: Option<(Range<usize>, PressedIn)>,
    /// The tests' boxes, each on a slide by its place among the shown ones, and what reached them.
    #[cfg(feature = "test-hooks")]
    stand_ins: Vec<(usize, Rect)>,
    #[cfg(feature = "test-hooks")]
    stand_in_presses: Vec<StandInPress>,
    _talk_changed: Subscription,
    _window_closed: Subscription,
    _owner_changed: Option<Subscription>,
}

impl Presentation {
    fn new(document: Entity<Document>, owner: Option<Owner>, cx: &mut Context<Self>) -> Self {
        // What the window of the talk does about a terminal, its yes among it, shows here.
        let owner_changed = owner
            .as_ref()
            .and_then(|(workspace, _)| workspace.upgrade())
            .map(|workspace| cx.observe(&workspace, |_, _, cx| cx.notify()));
        let talk_changed = cx.observe(&document, |this, _, cx| this.talk_changed(cx));
        let this = cx.entity().downgrade();
        let window_closed = cx.on_window_closed(move |cx, closed| {
            this.update(cx, |this, cx| this.window_closed(closed, cx))
                .ok();
        });
        Presentation {
            document,
            session: Session::new(),
            started_at: cx.background_executor().now(),
            reveal: None,
            arrival: None,
            slide_began: None,
            audience: None,
            presenter: None,
            ended: false,
            owner,
            ask_pending: false,
            visits: 0,
            pressed: None,
            #[cfg(feature = "test-hooks")]
            stand_ins: Vec::new(),
            #[cfg(feature = "test-hooks")]
            stand_in_presses: Vec::new(),
            _talk_changed: talk_changed,
            _window_closed: window_closed,
            _owner_changed: owner_changed,
        }
    }

    /// The window the audience sees and what's in it, while it's open.
    pub fn audience(&self) -> Option<(AnyWindowHandle, Entity<AudienceView>)> {
        self.audience.as_ref()?.get()
    }

    /// The presenter's window and what's in it, if the show has one and it's open.
    pub fn presenter(&self) -> Option<(AnyWindowHandle, Entity<PresenterView>)> {
        self.presenter.as_ref()?.get()
    }

    /// Whether a live terminal has the keys: then every key goes to its program.
    fn keys_held(&self) -> bool {
        self.owner.as_ref().is_some_and(|(_, live)| live.keys().is_some())
    }

    /// A key while a live terminal may have the keys: if one has them, the key goes to it, except
    /// the one that gives them back. Says whether the key was dealt with.
    fn key_for_live(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some((_, live)) = self.owner.clone() else { return false };
        if live.keys().is_none() {
            return false;
        }
        if gives_keys_back(key) {
            live.release_keys();
            cx.notify();
            return true;
        }
        live.send(key)
    }

    /// Asks, in the window the presenter looks at, whether `command` may run, unless that's
    /// already on its way.
    fn ask_for_live(&mut self, command: Vec<String>, cx: &mut Context<Self>) {
        self.ask_in_window(cx, move |workspace, window, cx| {
            workspace.ask_to_run(command, window, cx);
        });
    }

    /// Asks, in the window the presenter looks at, what the button pressed last waits for, unless a
    /// question is already on its way: its answer has the button go on.
    fn ask_for_button(&mut self, cx: &mut Context<Self>) {
        self.ask_in_window(cx, |workspace, window, cx| workspace.ask_for_button(window, cx));
    }

    /// Has the window of the talk `ask` in the window the presenter looks at, once the window is free
    /// to, unless a question is already on its way.
    fn ask_in_window(&mut self, cx: &mut Context<Self>, ask: impl FnOnce(&mut WorkspaceView, &mut Window, &mut Context<WorkspaceView>) + 'static) {
        if self.ask_pending {
            return;
        }
        let Some((workspace, _)) = self.owner.clone() else { return };
        let Some(window) = self.presenter.as_ref().map(|shown| shown.window).or(self.audience.as_ref().map(|shown| shown.window)) else { return };
        self.ask_pending = true;
        // Nothing typed meanwhile reaches a program.
        if let Some((_, live)) = &self.owner {
            live.release_keys();
        }
        let show = cx.entity().downgrade();
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| ask(workspace, window, cx)).ok();
                })
                .ok();
            show.update(cx, |show, _| show.ask_pending = false).ok();
        });
    }

    /// The talk's slide the show is on, and which visit to it this is: what a button's press that
    /// waits for a yes is for.
    pub(crate) fn visit(&self, cx: &mut App) -> (usize, u64) {
        let at = self.session.place().slide;
        (self.document.update(cx, |document, _| document.shown_slide(at)), self.visits)
    }

    /// Whether the show is over, or never got a window to run in.
    pub fn has_ended(&self) -> bool {
        self.ended
    }

    /// Where the show is.
    pub fn place(&self) -> Place {
        self.session.place()
    }

    /// The seconds since the slide the show is on appeared, at `now`: from the first frame of
    /// its transition, or of the slide itself if it has none. They're held to steps of
    /// 1/[`MOMENTS_PER_SECOND`](crate::present::MOMENTS_PER_SECOND) s, so that both windows
    /// ask for the same moment and share its layout.
    pub fn time_at(&self, now: Instant) -> f32 {
        self.slide_began.map_or(0.0, |began| {
            moment_time(now.saturating_duration_since(began).as_secs_f32())
        })
    }

    /// Goes forward by a click, or to the next slide once this one's clicks have all come. On a
    /// slide whose terminal recording is still being read, whose marks aren't known yet, it waits
    /// there rather than going on past them.
    pub fn next(&mut self, cx: &mut Context<Self>) {
        let left = self.session.place();
        let left_time = self.time_at(cx.background_executor().now());
        let moved = self.document.update(cx, |document, _| {
            let leaving = self.session.upcoming(document).is_some_and(|upcoming| upcoming.slide != left.slide);
            if leaving && document.terminal_pending(document.shown_slide(left.slide)) {
                return Moved::Stayed;
            }
            self.session.next(document)
        });
        let began = cx.background_executor().now();
        match moved {
            Moved::Revealed { step } => self.reveal = Some(Reveal { step, began }),
            Moved::Arrived { transition, .. } => {
                self.visits += 1;
                self.reveal = None;
                self.slide_began = None;
                self.arrival = (transition != Transition::None).then_some(Arrival {
                    left,
                    left_time,
                    transition,
                    began: None,
                });
            }
            Moved::BackedUp | Moved::Stayed => return,
        }
        cx.notify();
    }

    /// Goes straight to click step `step` of the slide the show is on, or to its last if it has
    /// fewer: the clicks come in after it as [`Self::next`] brings them, and going back shows them
    /// at once, as [`Self::previous`] does.
    pub fn go_to_step(&mut self, step: u32, cx: &mut Context<Self>) {
        let moved = self.document.update(cx, |document, _| self.session.go_to_step(document, step));
        match moved {
            Moved::Revealed { step } => {
                let began = cx.background_executor().now();
                self.reveal = Some(Reveal { step, began });
            }
            Moved::BackedUp => {
                self.reveal = None;
                self.arrival = None;
            }
            Moved::Arrived { .. } | Moved::Stayed => return,
        }
        cx.notify();
    }

    /// Goes back by a click, or to the end of the slide before, with nothing moving.
    pub fn previous(&mut self, cx: &mut Context<Self>) {
        let left = self.session.place().slide;
        let moved = self
            .document
            .update(cx, |document, _| self.session.previous(document));
        if moved != Moved::Stayed {
            self.reveal = None;
            self.arrival = None;
            self.slide_changed_from(left);
            cx.notify();
        }
    }

    /// Starts the time again if the show is no longer on slide `left`.
    fn slide_changed_from(&mut self, left: usize) {
        if self.session.place().slide != left {
            self.slide_began = None;
            self.visits += 1;
        }
    }

    /// Ends the show and closes its windows.
    pub fn end(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.ended, true) {
            return;
        }
        // What was started ends with the show.
        if let Some((workspace, live)) = self.owner.clone() {
            live.set_show_on(false);
            if workspace.update(cx, |workspace, cx| workspace.end_live(cx)).is_err() {
                live.clone().end_all();
            }
        }
        let windows = self.windows();
        // Not yet, as the key that ended the show may still be busy in one of them.
        cx.defer(move |cx| {
            for window in windows.into_iter().flatten() {
                window
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            }
        });
    }

    fn windows(&self) -> [Option<AnyWindowHandle>; 2] {
        [
            self.audience.as_ref().map(|opened| opened.window),
            self.presenter.as_ref().map(|opened| opened.window),
        ]
    }

    fn opened(
        &mut self,
        audience: Option<ShowWindow<AudienceView>>,
        presenter: Option<ShowWindow<PresenterView>>,
    ) {
        self.ended = audience.is_none() && presenter.is_none();
        if let Some((_, live)) = &self.owner {
            live.set_show_on(!self.ended);
        }
        self.audience = audience;
        self.presenter = presenter;
    }

    /// One of the show's windows going ends the show, however it went.
    fn window_closed(&mut self, closed: WindowId, cx: &mut Context<Self>) {
        let ours = self.windows().into_iter().flatten();
        if ours.into_iter().any(|window| window.window_id() == closed) {
            self.end(cx);
        }
    }

    fn talk_changed(&mut self, cx: &mut Context<Self>) {
        let left = self.session.place().slide;
        self.document
            .update(cx, |document, _| self.session.refit(document));
        // What was on its way in may be gone by now.
        self.reveal = None;
        self.arrival = None;
        self.slide_changed_from(left);
        cx.notify();
    }

    /// Where the next press goes. `None` at the end of the talk.
    fn upcoming(&self, cx: &mut Context<Self>) -> Option<Place> {
        self.document
            .update(cx, |document, _| self.session.upcoming(document))
    }

    fn reveal_at(&self, now: Instant) -> Option<(u32, f32)> {
        let Reveal { step, began } = self.reveal?;
        let seconds = now.saturating_duration_since(began).as_secs_f32();
        (seconds < ENTER_SECONDS).then_some((step, seconds))
    }

    fn arrival_at(&self, now: Instant) -> Option<(Arrival, f32)> {
        let arrival = self.arrival?;
        let Some(began) = arrival.began else {
            return Some((arrival, 0.0));
        };
        let seconds = now.saturating_duration_since(began).as_secs_f32();
        (seconds < TRANSITION_SECONDS).then_some((arrival, seconds))
    }

    /// Starts the clock of the slide being arrived at, and the slide's own time, if this is its
    /// first frame.
    fn first_frame_drawn(&mut self, now: Instant) {
        if let Some(arrival) = &mut self.arrival {
            arrival.began.get_or_insert(now);
        }
        self.slide_began.get_or_insert(now);
    }

    /// How long until the slide's time reaches its next moment, which is when a slide that
    /// moves looks different.
    fn until_next_moment(&self, now: Instant) -> Duration {
        let moment = 1.0 / MOMENTS_PER_SECOND;
        let elapsed = self
            .slide_began
            .map_or(0.0, |began| now.saturating_duration_since(began).as_secs_f32());
        let left = moment - elapsed % moment;
        Duration::from_secs_f32(left.max(0.001))
    }

    /// Of `things`, the interactive things of the place the show is on, those that take a press
    /// now: a terminal only while its command runs. The tests' boxes are among them.
    fn taking_presses(&self, things: &[InteractiveThing]) -> Vec<InteractiveThing> {
        let live = self.owner.as_ref().map(|(_, live)| live);
        let taking: Vec<InteractiveThing> = things
            .iter()
            .filter(|thing| match &thing.kind {
                InteractiveKind::Terminal(id) => live.is_some_and(|live| live.is_running(id)),
                _ => true,
            })
            .cloned()
            .collect();
        #[cfg(feature = "test-hooks")]
        let taking = {
            let mut taking = taking;
            taking.extend(self.stand_ins_here());
            taking
        };
        taking
    }

    /// A press in `window` on `hit`, an interactive thing, or on none: the window has it until it's
    /// let go of.
    fn press_on(&mut self, hit: Option<&InteractiveThing>, window: PressedIn) {
        self.pressed = hit.map(|thing| (thing.source.clone(), window));
        #[cfg(feature = "test-hooks")]
        if hit.is_some_and(|thing| thing.kind == InteractiveKind::StandIn) {
            self.stand_in_presses.push(StandInPress::Down(window));
        }
    }

    /// The press `window` has let go of over `over`, or over nothing interactive. It's for the thing
    /// it began on only if it's let go of over that thing, and otherwise for nothing.
    fn let_go(&mut self, over: Option<&InteractiveThing>, window: PressedIn, cx: &mut Context<Self>) {
        let Some((source, _)) = self.pressed.take_if(|(_, began)| *began == window) else { return };
        let Some(thing) = over.filter(|thing| thing.source == source) else { return };
        match thing.kind {
            // A terminal took the keys as it was pressed, and a model turns as it's dragged.
            InteractiveKind::Terminal(_) | InteractiveKind::Model(_) => {}
            // A button acts, once, whichever window it was pressed in: both show what it did.
            InteractiveKind::Button => {
                let Some(button) = self.button_here(&source, cx) else { return };
                if let Acted::Refused(why) = act(self, &button, cx) {
                    self.tell(why, cx);
                }
            }
            #[cfg(feature = "test-hooks")]
            InteractiveKind::StandIn => self.stand_in_presses.push(StandInPress::Up(window)),
        }
    }

    /// The button whose tag is written at `source` on the slide the show is on, as it is now.
    fn button_here(&self, source: &Range<usize>, cx: &mut Context<Self>) -> Option<ButtonLayer> {
        let now = cx.background_executor().now();
        let moment = Moment { step: self.session.place().step, time: self.time_at(now) };
        let slide = self.session.place().slide;
        self.document.update(cx, |document, _| {
            let slide = document.shown_slide(slide);
            document.buttons_at(slide, moment).into_iter().find(|button| button.source == *source)
        })
    }

    /// Puts `sentence` on the status line of the window of the talk, if the show has one.
    fn tell(&self, sentence: String, cx: &mut Context<Self>) {
        if let Some((workspace, _)) = &self.owner {
            workspace.update(cx, |workspace, cx| workspace.tell(sentence, cx)).ok();
        }
    }

    /// Whether `window` has a press it hasn't let go of.
    fn holds_press(&self, window: PressedIn) -> bool {
        self.pressed.as_ref().is_some_and(|(_, began)| *began == window)
    }

    /// Whether the slide the show is on moves, and so needs drawing anew as time passes.
    fn slide_moves(&self, cx: &mut App) -> bool {
        let slide = self.session.place().slide;
        self.document
            .update(cx, |document, _| document.timeline_of(document.shown_slide(slide)).animated)
    }
}

#[cfg(feature = "test-hooks")]
impl Presentation {
    /// Puts a plain box at `rect`, in slide units, on the show's slide `slide`, by its place among
    /// the shown ones. It takes presses as an interactive thing does, and says what reached it.
    pub fn insert_for_tests(&mut self, slide: usize, rect: Rect, cx: &mut Context<Self>) {
        self.stand_ins.push((slide, rect));
        cx.notify();
    }

    /// What reached the tests' boxes, first to last.
    pub fn stand_in_presses(&self) -> &[StandInPress] {
        &self.stand_in_presses
    }

    /// The tests' boxes on the slide the show is on, in front of everything else on it. Each is
    /// told from the others by a place no tag can be written at.
    fn stand_ins_here(&self) -> impl Iterator<Item = InteractiveThing> + '_ {
        let slide = self.session.place().slide;
        self.stand_ins.iter().enumerate().filter(move |(_, (on, _))| *on == slide).map(|(at, (_, rect))| InteractiveThing {
            source: usize::MAX - at..usize::MAX - at,
            rect: *rect,
            rotate: 0.0,
            kind: InteractiveKind::StandIn,
            id: None,
        })
    }
}

/// Does what pressing `button`, on the slide `show` is on, does: goes to its step, or types its text
/// into its terminal, starting that first, once each has its yes. What it can't do comes back, with
/// the sentence the status line shows.
pub(crate) fn act(show: &mut Presentation, button: &ButtonLayer, cx: &mut Context<Presentation>) -> Acted {
    let at = show.session.place().slide;
    let (slide, steps) = show.document.update(cx, |document, _| {
        let slide = document.shown_slide(at);
        (slide, document.timeline_of(slide).steps)
    });
    match press_button(&button.on, steps) {
        Press::Nothing => Acted::Done,
        Press::Refused(why) => Acted::Refused(why),
        Press::Step(step) => {
            show.go_to_step(step, cx);
            Acted::Done
        }
        Press::Run { text, terminal } => {
            let Some((workspace, _)) = show.owner.clone() else { return Acted::Refused(BUTTON_NO_TERMINALS.to_string()) };
            let visit = show.visits;
            match workspace.update(cx, |workspace, cx| workspace.press_run_button(slide, visit, &terminal, &text, cx)) {
                Ok(RunPressed::Typed) | Err(_) => Acted::Done,
                Ok(RunPressed::Refused(why)) => Acted::Refused(why),
                Ok(RunPressed::Ask) => {
                    show.ask_for_button(cx);
                    Acted::Asked
                }
            }
        }
    }
}

/// What's said of a button that types, in a show that runs no live terminals.
const BUTTON_NO_TERMINALS: &str = "This show runs no live terminals, so this button types nothing.";

/// The interactive things of the talk at `place`, `time` seconds after its slide appeared, whether
/// or not they take a press now.
fn interactive_things_at(document: &Entity<Document>, place: Place, time: f32, cx: &mut App) -> Vec<InteractiveThing> {
    let moment = Moment { step: place.step, time };
    document.update(cx, |document, _| {
        let slide = document.shown_slide(place.slide);
        interactive_on(document, slide, moment)
    })
}

/// Where `position`, in the window, is on the slide drawn on `canvas`, in slide units. `None` until
/// the slide has been drawn.
fn slide_point(canvas: &Entity<SlideCanvas>, position: Point<Pixels>, cx: &App) -> Option<Point<f32>> {
    let slide = canvas.read(cx).slide_bounds()?;
    let unit = f32::from(slide.size.width) / slides_engine::SLIDE_WIDTH;
    (unit > 0.0).then(|| point(f32::from(position.x - slide.origin.x) / unit, f32::from(position.y - slide.origin.y) / unit))
}

/// The box a box at `rect`, turned by `rotate` degrees about its middle, fits in.
fn turned_bounds(rect: Rect, rotate: f32) -> Rect {
    let (sin, cos) = rotate.to_radians().sin_cos();
    let (w, h) = (rect.w * cos.abs() + rect.h * sin.abs(), rect.w * sin.abs() + rect.h * cos.abs());
    Rect { x: rect.x + (rect.w - w) / 2., y: rect.y + (rect.h - h) / 2., w, h }
}

/// The faint line around `thing`, on the slide drawn on `canvas`, to lay over the canvas: the talk's
/// accent, at [`HOVER_OPACITY`]. It's the window's alone, and never in the slide's pictures.
fn hover_outline(canvas: &Entity<SlideCanvas>, document: &Entity<Document>, thing: &InteractiveThing, cx: &App) -> Option<AnyElement> {
    let canvas = canvas.read(cx);
    let (slide, area) = (canvas.slide_bounds()?, canvas.area()?);
    let unit = f32::from(slide.size.width) / slides_engine::SLIDE_WIDTH;
    let across = slide.origin - area.origin;
    let bounds = turned_bounds(thing.rect, thing.rotate);
    let accent = document.read(cx).theme().accent;
    let color = Rgba {
        r: f32::from(accent.r) / 255.,
        g: f32::from(accent.g) / 255.,
        b: f32::from(accent.b) / 255.,
        a: f32::from(accent.a) / 255.,
    }
    .opacity(HOVER_OPACITY);
    let outline = div()
        .id(INTERACTIVE_HOVER_ID)
        .test_support()
        .absolute()
        .left(across.x + px(bounds.x * unit))
        .top(across.y + px(bounds.y * unit))
        .w(px(bounds.w * unit))
        .h(px(bounds.h * unit))
        .border_1()
        .border_color(color);
    Some(outline.into_any_element())
}

/// The shade over `thing`, under the pointer in `window`, while it's a button the press `window`
/// holds began on: darker by [`PRESSED_DARKER`], within its rounded box, over the slide drawn on
/// `canvas` and never in it. A button turned on the slide gets none, as its box can't be shaded
/// turned.
fn pressed_look(canvas: &Entity<SlideCanvas>, show: &Presentation, thing: &InteractiveThing, window: PressedIn, cx: &App) -> Option<AnyElement> {
    let held = show.pressed.as_ref().is_some_and(|(source, began)| *began == window && *source == thing.source);
    if !held || thing.kind != InteractiveKind::Button || thing.rotate != 0.0 {
        return None;
    }
    let canvas = canvas.read(cx);
    let (slide, area) = (canvas.slide_bounds()?, canvas.area()?);
    let unit = f32::from(slide.size.width) / slides_engine::SLIDE_WIDTH;
    let across = slide.origin - area.origin;
    let rect = thing.rect;
    let shade = div()
        .id(BUTTON_PRESSED_ID)
        .test_support()
        .absolute()
        .left(across.x + px(rect.x * unit))
        .top(across.y + px(rect.y * unit))
        .w(px(rect.w * unit))
        .h(px(rect.h * unit))
        .rounded(px(BUTTON_RADIUS * unit))
        .bg(black().opacity(PRESSED_DARKER));
    Some(shade.into_any_element())
}

/// Puts a moment of the talk on `canvas`: `place`, `time` seconds after its slide appeared. It
/// returns when each of its layers comes in. Before the canvas has measured itself there's no
/// scale to draw at, and nothing is drawn: its first paint brings the caller back through
/// [`ScaleChanged`].
fn show_on(
    canvas: &Entity<SlideCanvas>,
    document: &Entity<Document>,
    place: Place,
    time: f32,
    cx: &mut App,
) -> Vec<LayerTiming> {
    let Some(scale) = canvas.read(cx).scale() else {
        return Vec::new();
    };
    let moment = Moment {
        step: place.step,
        time,
    };
    // Both from the one layout, as a component can give a layer at one moment and none at another.
    let (layers, timings) = document.update(cx, |document, _| {
        let slide = document.shown_slide(place.slide);
        (
            document.layers_at(slide, moment, scale),
            document.timings_at(slide, moment),
        )
    });
    let looks = timings
        .iter()
        .map(|timing| layer_look(*timing, place.step, None))
        .collect();
    canvas.update(cx, |canvas, cx| {
        canvas.set_layers(layers, cx);
        canvas.set_looks(looks, cx);
    });
    timings
}

/// Puts the show's place on `canvas` again if its time at `now` isn't `drawn_time`, the time
/// the canvas was last drawn at, and returns when each layer comes in if it did.
fn catch_up(
    show: &Entity<Presentation>,
    canvas: &Entity<SlideCanvas>,
    drawn_time: &mut f32,
    now: Instant,
    cx: &mut App,
) -> Option<Vec<LayerTiming>> {
    let show = show.read(cx);
    let (document, place, time) = (
        show.document.clone(),
        show.session.place(),
        show.time_at(now),
    );
    if time == *drawn_time {
        return None;
    }
    *drawn_time = time;
    Some(show_on(canvas, &document, place, time, cx))
}

/// Notifies the view once the slide's time reaches its next moment, unless that's already
/// on its way. A slide that moves changes 30 times a second, so a frame for every one the
/// display could show would mostly draw the same picture again.
fn redraw_at_next_moment<V: 'static>(
    pending: &mut Option<Task<()>>,
    show: &Entity<Presentation>,
    now: Instant,
    cx: &mut Context<V>,
    clear: fn(&mut V) -> &mut Option<Task<()>>,
) {
    if pending.is_some() {
        return;
    }
    let wait = show.read(cx).until_next_moment(now);
    *pending = Some(cx.spawn(async move |this, cx| {
        cx.background_executor().timer(wait).await;
        this.update(cx, |view, cx| {
            *clear(view) = None;
            cx.notify();
        })
        .ok();
    }));
}

/// Whether a question is open in one of the show's `windows`, `here` being one of them. While one
/// is, the show's keys and clicks don't move the show.
fn dialog_in(windows: [Option<AnyWindowHandle>; 2], here: &mut Window, cx: &mut App) -> bool {
    windows.into_iter().flatten().any(|handle| {
        if handle == here.window_handle() {
            here.has_active_dialog(cx)
        } else {
            handle.update(cx, |_, window, cx| window.has_active_dialog(cx)).unwrap_or(false)
        }
    })
}

/// The root of a show's window, with the show's keys, and a title bar if `title` says what
/// to put on one. A full-screen window has none.
fn window_root(
    show: &Entity<Presentation>,
    focus_handle: &FocusHandle,
    title: Option<&'static str>,
    keys_held: bool,
) -> Div {
    let (next, previous, end, keyed) = (show.clone(), show.clone(), show.clone(), show.clone());
    v_flex()
        .key_context(if keys_held { KEYS_HELD_CONTEXT } else { KEY_CONTEXT })
        .track_focus(focus_handle)
        // While a live terminal has the keys, none of the show's own bindings apply: this has every
        // key, the arrows and Escape included, but the one that gives them back.
        .on_key_down(move |event, window, cx| {
            // While a question is open no key goes to a program.
            let windows = keyed.read(cx).windows();
            if dialog_in(windows, window, cx) {
                return;
            }
            if keyed.update(cx, |show, cx| show.key_for_live(&event.keystroke, cx)) {
                cx.stop_propagation();
                return;
            }
            // The program ended since this window was drawn: the key is the show's, as it would be.
            if keys_held && !keyed.read(cx).keys_held() && !event.keystroke.modifiers.modified() {
                let action: Option<Box<dyn Action>> = match event.keystroke.key.as_str() {
                    "right" | "pagedown" | "space" | "enter" => Some(Box::new(ShowNext)),
                    "left" | "pageup" | "backspace" => Some(Box::new(ShowPrevious)),
                    "escape" => Some(Box::new(EndShow)),
                    _ => None,
                };
                if let Some(action) = action {
                    window.dispatch_action(action, cx);
                    cx.stop_propagation();
                }
            }
        })
        .on_action(move |_: &ShowNext, window, cx| {
            let windows = next.read(cx).windows();
            if !dialog_in(windows, window, cx) {
                next.update(cx, |show, cx| show.next(cx))
            }
        })
        .on_action(move |_: &ShowPrevious, window, cx| {
            let windows = previous.read(cx).windows();
            if !dialog_in(windows, window, cx) {
                previous.update(cx, |show, cx| show.previous(cx))
            }
        })
        .on_action(move |_: &EndShow, _, cx| end.update(cx, |show, cx| show.end(cx)))
        .size_full()
        .when_some(title, |this, title| {
            this.child(TitleBar::new().child(div().text_sm().child(title)))
        })
}

/// The slide, filling its window, with nothing but black around it.
pub struct AudienceView {
    show: Entity<Presentation>,
    /// The canvas of the slide the show is on.
    current: Entity<SlideCanvas>,
    /// The canvas of the slide being left, and empty the rest of the time. The two swap roles,
    /// so that both have measured themselves and a slide arrives with its layers already drawn.
    leaving: Entity<SlideCanvas>,
    /// The slide on `current`.
    slide: usize,
    timings: Vec<LayerTiming>,
    /// Whether the slide on `current` moves, and the time it was last drawn at.
    moves: bool,
    drawn_time: f32,
    /// The wait for the next moment of a slide that moves.
    next_moment: Option<Task<()>>,
    /// The slide whose videos were last started, which start again from their start once it's
    /// left and shown again.
    videos_of: Option<usize>,
    /// The models on the show's place, and the slide whose models were last shown, which go
    /// back to their views once it's left.
    models: Vec<ModelLayer>,
    models_of: Option<usize>,
    /// A press on a model: the model, where the pointer last was, and whether it has moved, which
    /// has the model drawn live and turned.
    turn_drag: Option<(ModelLayer, Point<Pixels>, bool)>,
    /// Whether the press last let go of turned a model, so that it's no click of the show's.
    turned: bool,
    /// The place whose terminal recordings were last played, which tells a click back from one on,
    /// and whether it was come to going back: so it stays until the place changes.
    replayed_at: Option<Place>,
    replayed_back: bool,
    /// The recordings of the slide the next click brings, readied at their start, each with the
    /// slide: the slide shows with their first pictures already drawn.
    ready: Vec<(usize, Replay)>,
    /// The terminals of the show's place that run a command, and the slide they're on.
    live_on: Vec<TerminalLayer>,
    live_slide: usize,
    /// Whether what they may do has been looked at since the place or the window's say changed.
    live_checked: bool,
    /// Whether the press last let go of was on an interactive thing, or took a terminal's keys back,
    /// and so is no click of the show's.
    swallow_click: bool,
    /// The interactive things of the show's place, whether or not they take a press now.
    things: Vec<InteractiveThing>,
    /// Where the pointer is in the window, and the interactive thing it was last drawn over.
    pointer: Option<Point<Pixels>>,
    hovered: Option<Range<usize>>,
    /// When the pointer last moved, whether it's hidden for having been still, and the wait until
    /// it may be.
    pointer_moved: Instant,
    pointer_hidden: bool,
    pointer_wait: Option<Task<()>>,
    rehearsing: bool,
    focus_handle: FocusHandle,
    _watching: [Subscription; 9],
}

impl AudienceView {
    fn new(
        show: Entity<Presentation>,
        rehearsing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = cx.new(|_| SlideCanvas::default());
        let leaving = cx.new(|_| SlideCanvas::default());
        let watching = [
            cx.observe(&show, |this, _, cx| this.show_place(cx)),
            cx.subscribe(&current, |this, _, _: &ScaleChanged, cx| {
                this.show_place(cx)
            }),
            cx.subscribe(&leaving, |this, _, _: &ScaleChanged, cx| {
                this.show_place(cx)
            }),
            // A save let go of the file of a video playing: it's opened again once that's done.
            cx.subscribe(&current, |this, _, _: &VideoLetGo, cx| this.show_place(cx)),
            cx.subscribe(&leaving, |this, _, _: &VideoLetGo, cx| this.show_place(cx)),
            // A video opened to play may have been unpacked: a still that waited for that is made.
            cx.subscribe(&current, |this, _, _: &VideoOpened, cx| this.videos_opened(cx)),
            cx.subscribe(&leaving, |this, _, _: &VideoOpened, cx| this.videos_opened(cx)),
            // A save let go of the file of a model being read: it's read again once that's done.
            cx.subscribe(&current, |this, _, _: &ModelLetGo, cx| this.show_place(cx)),
            cx.subscribe(&leaving, |this, _, _: &ModelLetGo, cx| this.show_place(cx)),
        ];
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        AudienceView {
            show,
            current,
            leaving,
            slide: 0,
            timings: Vec::new(),
            moves: false,
            drawn_time: 0.0,
            next_moment: None,
            videos_of: None,
            models: Vec::new(),
            models_of: None,
            turn_drag: None,
            turned: false,
            replayed_at: None,
            replayed_back: false,
            ready: Vec::new(),
            live_on: Vec::new(),
            live_slide: 0,
            live_checked: false,
            swallow_click: false,
            things: Vec::new(),
            pointer: None,
            hovered: None,
            pointer_moved: cx.background_executor().now(),
            pointer_hidden: false,
            pointer_wait: None,
            rehearsing,
            focus_handle,
            _watching: watching,
        }
    }

    /// The canvas of the slide the show is on.
    pub fn current(&self) -> &Entity<SlideCanvas> {
        &self.current
    }

    /// Plays the videos of the show's place: each video on its slide that's there at its step,
    /// one with a click once that's reached, and nothing else. On a slide come to, every video
    /// starts from its start, and none plays on the slide left.
    fn play_videos(&mut self, document: &Entity<Document>, place: Place, time: f32, cx: &mut Context<Self>) {
        let moment = Moment {
            step: place.step,
            time,
        };
        let videos: Vec<VideoLayer> = document.update(cx, |document, _| {
            let slide = document.shown_slide(place.slide);
            document.videos_at(slide, moment)
        });
        let wanted: Vec<VideoLayer> = videos
            .into_iter()
            .filter(|video| video.click.is_none_or(|click| click <= place.step))
            .collect();
        let afresh = self.videos_of != Some(place.slide);
        self.videos_of = Some(place.slide);
        if afresh {
            // The slide left keeps its videos' last pictures while it goes.
            self.leaving.update(cx, |canvas, cx| canvas.hold_videos(cx));
        }
        // Only the videos to be opened are looked for, and one whose file a save let go of is
        // opened again once the save is done.
        let saving = document.read(cx).saving_the_pack();
        let waiting = if saving { Vec::new() } else { self.current.read(cx).videos_let_go() };
        let to_open: Vec<String> = self.current.read(cx).videos_to_open(&wanted, afresh).into_iter().map(|video| video.on.src.clone()).collect();
        let mut opened: Vec<(String, LargeOpen)> = to_open
            .iter()
            .chain(&waiting)
            .filter_map(|src| Some((src.clone(), document.read(cx).open_large(src)?)))
            .collect();
        let mut take = move |src: &str| {
            let at = opened.iter().position(|(file, _)| file == src)?;
            Some(opened.swap_remove(at).1)
        };
        self.current.update(cx, |canvas, cx| {
            canvas.show_videos(&wanted, afresh, &mut take, cx);
            canvas.open_videos_again(&mut take, cx);
        });
    }

    /// Draws live the models of the show's place that move: each that turns on its own, and
    /// each a click has sent to one of its views, which flies there. On a slide come to, every
    /// model is back at its view, and none is drawn live on the slide left.
    fn show_models(&mut self, document: &Entity<Document>, place: Place, time: f32, cx: &mut Context<Self>) {
        let moment = Moment {
            step: place.step,
            time,
        };
        let models: Vec<ModelLayer> = document.update(cx, |document, _| {
            let slide = document.shown_slide(place.slide);
            document.models_at(slide, moment)
        });
        let models: Vec<ModelLayer> = models.into_iter().filter(|model| model.click.is_none_or(|click| click <= place.step)).collect();
        let afresh = self.models_of != Some(place.slide);
        self.models_of = Some(place.slide);
        if afresh {
            self.turn_drag = None;
            self.leaving.update(cx, |canvas, cx| canvas.stop_models(cx));
            self.current.update(cx, |canvas, cx| canvas.stop_models(cx));
        }
        self.current.update(cx, |canvas, cx| canvas.place_models(&models, cx));
        for model in &models {
            let aim = aim_at(model, place.step);
            let shown = self.current.read(cx).model_aim(model.layer);
            match shown {
                Some(shown) if shown != aim => self.current.update(cx, |canvas, cx| canvas.fly_model(model.layer, aim, cx)),
                Some(_) => {}
                // Shown at its view, and turning on its own if it does; one a click sends on
                // flies there from its view.
                None if model.on.turn || aim != Aim::Home => {
                    let start = if afresh { aim } else { Aim::Home };
                    self.turn_model(document, model, start, cx);
                    if !afresh {
                        self.current.update(cx, |canvas, cx| canvas.fly_model(model.layer, aim, cx));
                    }
                }
                None => {}
            }
        }
        // One whose file a save let go of is read again once the save is done.
        let waiting = if document.read(cx).saving_the_pack() { Vec::new() } else { self.current.read(cx).models_let_go() };
        if !waiting.is_empty() {
            let mut opened: Vec<(String, LargeOpen)> = waiting.iter().filter_map(|src| Some((src.clone(), document.read(cx).open_large(src)?))).collect();
            self.current.update(cx, |canvas, cx| {
                canvas.read_models_again(|src| opened.iter().position(|(at, _)| at == src).map(|at| opened.swap_remove(at).1), cx)
            });
        }
        self.models = models;
    }

    /// Plays the terminal recordings of the show's place: each on its slide that's there at its
    /// step, from when the slide shows or its click comes, to the next of its stops after the
    /// clicks come since, where it waits for the next click. A click back sends it back to the
    /// stop before, still. On a slide come to going on, each starts from its start, readied before
    /// if it could be; on one come back to from a later one, each shows where its clicks have
    /// brought it, its end being its still. None plays on the slide left. One still being read, or
    /// that can't be played, isn't started. No more than [`MOST_PLAYING`] play or are readied at
    /// once: the first of the slide shown, by their order on it, and then those readied for the
    /// next; the rest show their stills.
    fn play_recordings(&mut self, document: &Entity<Document>, place: Place, time: f32, cx: &mut Context<Self>) {
        let moment = Moment {
            step: place.step,
            time,
        };
        let (terminals, pending, slide): (Vec<TerminalLayer>, bool, usize) = document.update(cx, |document, _| {
            let slide = document.shown_slide(place.slide);
            (document.terminals_at(slide, moment), document.terminal_pending(slide), slide)
        });
        self.live_on = terminals
            .iter()
            .filter(|terminal| terminal.on.run.is_some() && terminal.source.is_some() && place.step >= terminal.click.unwrap_or(0))
            .cloned()
            .collect();
        self.live_slide = slide;
        self.live_checked = false;
        let was = self.replayed_at.replace(place);
        let afresh = was.is_none_or(|was| was.slide != place.slide);
        // Drawn again at the same place, it was come to as it was.
        let back = match was {
            Some(was) if was == place => self.replayed_back,
            Some(was) => (was.slide, was.step) > (place.slide, place.step),
            None => false,
        };
        self.replayed_back = back;
        let mut ready: Vec<Replay> = Vec::new();
        for (at, replay) in std::mem::take(&mut self.ready) {
            if at == place.slide && afresh && !back {
                ready.push(replay);
            } else {
                self.ready.push((at, replay));
            }
        }
        if afresh {
            // The slide left keeps its recordings' last pictures while it goes.
            self.leaving.update(cx, |canvas, cx| canvas.hold_recordings(cx));
            self.current.update(cx, |canvas, cx| canvas.stop_recordings(cx));
        }
        self.current.update(cx, |canvas, cx| canvas.place_recordings(&terminals, cx));
        for terminal in &terminals {
            let Some(source) = terminal.source.clone() else { continue };
            // One that only runs a command has no recording to play.
            if terminal.on.src.is_empty() {
                continue;
            }
            let click = terminal.click.unwrap_or(0);
            if place.step < click {
                // Back before its click, it stops.
                self.current.update(cx, |canvas, cx| canvas.stop_recording_on(terminal.layer, cx));
                continue;
            }
            // How many of its stops the clicks since it appeared have passed.
            let passed = (place.step - click) as usize;
            let aim = if back { ReplayAim::At(passed) } else { ReplayAim::PlayTo(Some(passed)) };
            match self.current.read(cx).recording_aim(terminal.layer) {
                Some(aimed) if aimed == aim => {}
                Some(_) => self.current.update(cx, |canvas, cx| canvas.aim_recording(terminal.layer, aim, cx)),
                // At its end, still, it's its still.
                None if aim == ReplayAim::At(passed) && passed >= terminal.on.stops => {}
                None => {
                    if let Some(at) = ready.iter().position(|replay| replay.layer == terminal.layer && replay.on.src == terminal.on.src) {
                        let mut replay = ready.swap_remove(at);
                        replay.place(terminal.on.clone());
                        replay.aim(aim);
                        self.current.update(cx, |canvas, cx| canvas.adopt_recording(replay, cx));
                        continue;
                    }
                    if pending || document.update(cx, |document, _| document.terminal_problem_on(slide, &source)).is_some() {
                        continue;
                    }
                    // No more than the most play or are readied at once: this slide's first, by
                    // their order on it, before the next slide's. Past them, one shows its still.
                    let readied = ready.len() + self.ready.len();
                    if self.current.read(cx).playing_recordings().len() + readied >= MOST_PLAYING && self.ready.pop().is_none() {
                        continue;
                    }
                    let Some(open) = document.read(cx).open_large(&terminal.on.src) else { continue };
                    let fonts = document.update(cx, |document, _| document.fonts_seed());
                    let (layer, on) = (terminal.layer, terminal.on.clone());
                    self.current.update(cx, |canvas, cx| canvas.play_recording(layer, on, aim, ReplaySource { open, fonts }, true, cx));
                }
            }
        }
    }

    /// Has the window of the talk start the terminals of the slide shown that run a command, those
    /// with a yes, and asks about those without one. Looks once for each place and each change of
    /// what the window says.
    fn drive_live(&mut self, cx: &mut Context<Self>) {
        if self.live_checked {
            return;
        }
        self.live_checked = true;
        let Some((workspace, _)) = self.show.read(cx).owner.clone() else { return };
        let (slide, terminals) = (self.live_slide, self.live_on.clone());
        for terminal in terminals {
            let step = workspace.update(cx, |workspace, cx| workspace.try_start_live(slide, &terminal, cx));
            if let Ok(LiveStep::Ask(command)) = step {
                self.show.update(cx, |show, cx| show.ask_for_live(command, cx));
            }
        }
    }

    /// Lays the running terminals of the slide shown over their layers, those that run, and takes
    /// them off when none does.
    fn apply_live(&mut self, cx: &mut Context<Self>) {
        let Some((_, live)) = self.show.read(cx).owner.clone() else { return };
        // Keys held by a terminal this slide doesn't show are given back.
        if let Some(held) = live.keys()
            && !self.live_on.iter().any(|terminal| terminal.source.clone().zip(terminal.on.run.clone()).is_some_and(|(source, run)| LiveId::new(source, run) == held))
        {
            live.release_keys();
            self.show.update(cx, |_, cx| cx.notify());
        }
        let keys = live.keys();
        let places: Vec<LivePlace> = self
            .live_on
            .iter()
            .filter_map(|terminal| {
                let id = LiveId::new(terminal.source.clone()?, terminal.on.run.clone()?);
                Some(LivePlace {
                    layer: terminal.layer,
                    session: live.weak(&id)?,
                    rect: terminal.on.rect,
                    rotate: terminal.on.rotate,
                    keys: keys.as_ref() == Some(&id),
                    follows: false,
                })
            })
            .collect();
        if places.is_empty() {
            self.current.update(cx, |canvas, cx| canvas.clear_live(cx));
            return;
        }
        let document = self.show.read(cx).document.clone();
        let fonts = document.update(cx, |document, _| document.fonts_seed());
        self.current.update(cx, |canvas, cx| canvas.place_live(places, fonts, cx));
    }

    /// Whether the pointer is hidden, for having been still on a slide with nothing to press.
    pub fn pointer_hidden(&self) -> bool {
        self.pointer_hidden
    }

    /// The interactive thing at `position` in the window that takes a press now, if there's one.
    fn thing_at(&self, position: Point<Pixels>, cx: &App) -> Option<InteractiveThing> {
        let at = slide_point(&self.current, position, cx)?;
        interactive_at(&self.show.read(cx).taking_presses(&self.things), at).cloned()
    }

    /// The pointer moved to `position`: it shows, and what it's over is outlined.
    fn pointer_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pointer = Some(position);
        self.pointer_moved = cx.background_executor().now();
        let hovered = self.thing_at(position, cx).map(|thing| thing.source);
        if self.pointer_hidden || hovered != self.hovered {
            cx.notify();
        }
    }

    /// Whether the pointer is to be hidden now, there being `interactive` things on the slide or
    /// none, and a wait for when it may be if it isn't yet.
    fn hide_pointer_when_still(&mut self, interactive: bool, cx: &mut Context<Self>) {
        let still_for = cx.background_executor().now().saturating_duration_since(self.pointer_moved);
        self.pointer_hidden = !interactive && still_for >= POINTER_HIDES_AFTER;
        if interactive || self.pointer_hidden || self.pointer_wait.is_some() {
            return;
        }
        let wait = POINTER_HIDES_AFTER - still_for;
        self.pointer_wait = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| {
                this.pointer_wait = None;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Lays out the slide the next click brings, if it's another, at the scale the audience sees
    /// it at, so that what it shows is ready by the time it's there: a busy recording's screen,
    /// drawn aside, has been, rather than its plain panel showing first. Its recordings there from
    /// its start are readied, each held at its start with its first picture drawn; those readied
    /// for a slide the next click no longer brings end. They're readied only while fewer than
    /// [`MOST_PLAYING`] play or are readied.
    fn ready_next(&mut self, document: &Entity<Document>, cx: &mut Context<Self>) {
        let here = self.show.read(cx).session.place().slide;
        let upcoming = self.show.update(cx, |show, cx| show.upcoming(cx)).filter(|upcoming| upcoming.slide != here);
        self.ready.retain(|(at, _)| upcoming.is_some_and(|upcoming| upcoming.slide == *at));
        let (Some(scale), Some(upcoming)) = (self.current.read(cx).scale(), upcoming) else { return };
        let moment = Moment { step: upcoming.step, time: 0.0 };
        let (terminals, pending, slide) = document.update(cx, |document, _| {
            let slide = document.shown_slide(upcoming.slide);
            document.layers_at(slide, moment, scale);
            (document.terminals_at(slide, moment), document.terminal_pending(slide), slide)
        });
        if pending {
            return;
        }
        for terminal in terminals {
            // Within the most that play or are readied at once, the slide shown's first.
            if self.current.read(cx).playing_recordings().len() + self.ready.len() >= MOST_PLAYING {
                break;
            }
            let Some(source) = terminal.source.clone() else { continue };
            if terminal.on.src.is_empty() {
                continue;
            }
            let readied = self.ready.iter().any(|(_, replay)| replay.layer == terminal.layer && replay.on.src == terminal.on.src);
            if readied || terminal.click.unwrap_or(0) > upcoming.step {
                continue;
            }
            if document.update(cx, |document, _| document.terminal_problem_on(slide, &source)).is_some() {
                continue;
            }
            let Some(open) = document.read(cx).open_large(&terminal.on.src) else { continue };
            let fonts = document.update(cx, |document, _| document.fonts_seed());
            let replay = Replay::start(terminal.layer, terminal.on, ReplayAim::Start, ReplaySource { open, fonts }, true, Some(scale));
            self.ready.push((upcoming.slide, replay));
        }
    }

    /// Has `model` drawn live on the slide shown, from `aim`, turning on its own if it does. One
    /// whose still already says it can't be drawn isn't read.
    fn turn_model(&mut self, document: &Entity<Document>, model: &ModelLayer, aim: Aim, cx: &mut Context<Self>) {
        let Some(source) = model.source.clone() else { return };
        let slide = self.slide;
        let problem = document.update(cx, |document, _| {
            let slide = document.shown_slide(slide);
            document.model_problem_on(slide, &source)
        });
        if problem.is_some() {
            return;
        }
        let Some(open) = document.read(cx).open_large(&model.on.src) else { return };
        let (layer, on) = (model.layer, model.on.clone());
        self.current.update(cx, |canvas, cx| canvas.turn_model(layer, on.clone(), open, on.turn, aim, cx));
    }

    /// Has `model` drawn live, if it isn't.
    fn make_live(&mut self, model: &ModelLayer, cx: &mut Context<Self>) {
        if self.current.read(cx).model_aim(model.layer).is_none() {
            let document = self.show.read(cx).document.clone();
            let place = self.show.read(cx).session.place();
            self.turn_model(&document, model, aim_at(model, place.step), cx);
        }
    }

    /// A press on the slide: on an interactive thing it's that thing's, and no click of the show's. On
    /// a running terminal it gives that terminal the keys. A press on a model may begin turning it
    /// by hand: once it moves.
    fn press(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.pointer_to(event.position, cx);
        let on_slide = slide_point(&self.current, event.position, cx).is_some();
        let hit = self.thing_at(event.position, cx);
        let keys = on_slide && press_for_keys(&self.show, hit.as_ref(), cx);
        if keys {
            // The keys are asked for by this window, which has them from now on.
            self.focus_handle.focus(window, cx);
        }
        self.swallow_click = keys || hit.is_some();
        self.show.update(cx, |show, _| show.press_on(hit.as_ref(), PressedIn::Audience));
        // A button under it looks pressed.
        cx.notify();
        self.turned = false;
        // Only a model in front of everything else under the pointer turns.
        self.turn_drag = self.model_in_front(hit.as_ref()).map(|model| (model, event.position, false));
    }

    /// The model `hit` is, if the interactive thing in front under the pointer is one.
    fn model_in_front(&self, hit: Option<&InteractiveThing>) -> Option<ModelLayer> {
        let Some(InteractiveKind::Model(layer)) = hit.map(|thing| &thing.kind) else { return None };
        self.models.iter().find(|model| model.layer == *layer).cloned()
    }

    /// A drag on a model turns it, and isn't a click of the show's. Any move shows the pointer, and
    /// outlines what it's over.
    fn drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.pointer_to(event.position, cx);
        if event.pressed_button != Some(MouseButton::Left) {
            // A press let go of where this window didn't see it is for nothing.
            if self.show.read(cx).holds_press(PressedIn::Audience) {
                self.show.update(cx, |show, cx| show.let_go(None, PressedIn::Audience, cx));
            }
            self.end_turn(cx);
            return;
        }
        let Some((model, from, moved)) = self.turn_drag.clone() else { return };
        let (dx, dy) = (f32::from(event.position.x - from.x), f32::from(event.position.y - from.y));
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let layer = model.layer;
        if !moved {
            self.make_live(&model, cx);
            self.current.update(cx, |canvas, cx| canvas.hold_model(layer, true, cx));
        }
        self.turned = true;
        self.turn_drag = Some((model, event.position, true));
        self.current.update(cx, |canvas, cx| canvas.turn_by(layer, dx, dy, cx));
    }

    /// The button let go of at `position`: a press on an interactive thing is for it if it's let go
    /// of over it, and it ends a drag on a model.
    fn let_go(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let over = self.thing_at(position, cx);
        self.show.update(cx, |show, cx| show.let_go(over.as_ref(), PressedIn::Audience, cx));
        self.end_turn(cx);
        cx.notify();
    }

    /// Ends a drag on a model.
    fn end_turn(&mut self, cx: &mut Context<Self>) {
        if let Some((model, _, true)) = self.turn_drag.take() {
            self.current.update(cx, |canvas, cx| canvas.hold_model(model.layer, false, cx));
        }
    }

    /// The wheel over a model brings it nearer, or takes it further away.
    fn wheel(&mut self, event: &ScrollEvent, window: &Window, cx: &mut Context<Self>) {
        let steps = f32::from(event.delta.pixel_delta(window.line_height()).y) / f32::from(window.line_height());
        if steps == 0.0 {
            return;
        }
        let hit = self.thing_at(event.position, cx);
        let Some(model) = self.model_in_front(hit.as_ref()) else { return };
        self.make_live(&model, cx);
        let layer = model.layer;
        self.current.update(cx, |canvas, cx| canvas.zoom_by(layer, steps, cx));
    }

    /// Makes the stills that waited for a video to be unpacked to play, or a model to turn.
    fn videos_opened(&mut self, cx: &mut Context<Self>) {
        let document = self.show.read(cx).document.clone();
        document.update(cx, |document, cx| {
            if document.try_waiting_stills_again() {
                cx.notify();
            }
        });
    }

    /// The canvas of the slide being left, while a transition is holding on to one.
    pub fn outgoing(&self, cx: &App) -> Option<&Entity<SlideCanvas>> {
        (!self.leaving.read(cx).images().is_empty()).then_some(&self.leaving)
    }

    fn show_place(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let show = self.show.read(cx);
        let (document, place, arrival, time) = (
            show.document.clone(),
            show.session.place(),
            show.arrival_at(now),
            show.time_at(now),
        );
        if arrival.is_some() && place.slide != self.slide {
            std::mem::swap(&mut self.current, &mut self.leaving);
        }
        self.slide = place.slide;
        self.moves = self.show.update(cx, |show, cx| show.slide_moves(cx));
        self.timings = show_on(&self.current, &document, place, time, cx);
        self.things = interactive_things_at(&document, place, time, cx);
        self.drawn_time = time;
        self.play_videos(&document, place, time, cx);
        self.show_models(&document, place, time, cx);
        self.play_recordings(&document, place, time, cx);
        self.ready_next(&document, cx);
        if let Some((arrival, _)) = arrival {
            // It has this already, unless the window has just changed size.
            show_on(
                &self.leaving,
                &document,
                arrival.left,
                arrival.left_time,
                cx,
            );
        }
        cx.notify();
    }

    /// Brings the canvases to this moment, and asks for another frame if anything is moving.
    fn animate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.show.update(cx, |show, _| show.first_frame_drawn(now));
        self.drive_live(cx);
        self.apply_live(cx);
        if self.moves
            && let Some(timings) =
                catch_up(&self.show, &self.current, &mut self.drawn_time, now, cx)
        {
            self.timings = timings;
        }
        let show = self.show.read(cx);
        let step = show.session.place().step;
        let arrival = show.arrival_at(now);
        // A layer that only appears is done at once, so there's nothing to draw frames for.
        let revealing = show.reveal_at(now).filter(|(revealed, _)| {
            self.timings
                .iter()
                .any(|timing| timing.click == Some(*revealed) && timing.enter != Enter::Appear)
        });

        let looks = self
            .timings
            .iter()
            .map(|timing| layer_look(*timing, step, revealing))
            .collect();
        let (outgoing, incoming) = arrival.map_or_else(Default::default, |(arrival, seconds)| {
            slide_looks(arrival.transition, seconds)
        });
        self.current.update(cx, |canvas, cx| {
            canvas.set_looks(looks, cx);
            canvas.set_slide_look(incoming, cx);
        });
        self.leaving.update(cx, |canvas, cx| {
            canvas.set_slide_look(outgoing, cx);
            if arrival.is_none() {
                canvas.set_layers(Vec::new(), cx);
                canvas.stop_videos(cx);
                canvas.stop_models(cx);
                canvas.stop_recordings(cx);
                canvas.clear_live(cx);
            }
        });
        if arrival.is_some() || revealing.is_some() {
            window.request_animation_frame();
        } else if self.moves {
            redraw_at_next_moment(&mut self.next_moment, &self.show, now, cx, |view: &mut Self| {
                &mut view.next_moment
            });
        }
    }
}

impl Render for AudienceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.animate(window, cx);
        let title = self.rehearsing.then_some("Audience");
        let taking = self.show.read(cx).taking_presses(&self.things);
        self.hide_pointer_when_still(!taking.is_empty(), cx);
        let hovered = self
            .pointer
            .and_then(|pointer| slide_point(&self.current, pointer, cx))
            .and_then(|at| interactive_at(&taking, at).cloned());
        self.hovered = hovered.as_ref().map(|thing| thing.source.clone());
        let document = self.show.read(cx).document.clone();
        let pressed = hovered.as_ref().and_then(|thing| pressed_look(&self.current, self.show.read(cx), thing, PressedIn::Audience, cx));
        let outline = hovered.and_then(|thing| hover_outline(&self.current, &document, &thing, cx));
        window_root(&self.show, &self.focus_handle, title, self.show.read(cx).keys_held())
            .bg(black())
            .child(
                div()
                    .id(AUDIENCE_ID)
                    .test_support()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    // A press on an interactive thing is that thing's: on a model a drag turns it and
                    // the wheel zooms it. None of it is a click.
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, window, cx| this.press(event, window, cx)))
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| this.drag(event, cx)))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, event: &MouseUpEvent, _, cx| this.let_go(event.position, cx)))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(|this, event: &MouseUpEvent, _, cx| this.let_go(event.position, cx)))
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| this.wheel(event, window, cx)))
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        let turned = std::mem::take(&mut this.turned);
                        let swallowed = std::mem::take(&mut this.swallow_click);
                        if turned || swallowed || this.show.read(cx).holds_press(PressedIn::Audience) {
                            return;
                        }
                        // Let go of over an interactive thing, it's no click, wherever it began.
                        if event.mouse_position().is_some_and(|at| this.thing_at(at, cx).is_some()) {
                            return;
                        }
                        // While a question is open, a click on the slide is not the show's.
                        let windows = this.show.read(cx).windows();
                        if dialog_in(windows, window, cx) {
                            return;
                        }
                        this.show.update(cx, |show, cx| show.next(cx));
                    }))
                    .child(self.leaving.clone())
                    .child(self.current.clone())
                    .children(pressed)
                    .children(outline),
            )
    }
}

/// A press on the slide on `hit`, the interactive thing in front under it that takes a press now, or
/// on none: on a running terminal it gives that terminal the keys, and anywhere else with the keys
/// given elsewhere it takes them back. Says whether it was one.
fn press_for_keys(show: &Entity<Presentation>, hit: Option<&InteractiveThing>, cx: &mut App) -> bool {
    let Some((_, live)) = show.read(cx).owner.clone() else { return false };
    let handled = match hit.map(|thing| &thing.kind) {
        Some(InteractiveKind::Terminal(id)) => live.give_keys(id),
        _ => live.release_keys(),
    };
    if handled {
        show.update(cx, |_, cx| cx.notify());
    }
    handled
}

/// The terminals of `slide` at `moment` that run a command and are there at the place's step.
fn live_terminals_at(document: &Entity<Document>, place: Place, time: f32, cx: &mut App) -> (Vec<TerminalLayer>, usize) {
    let moment = Moment { step: place.step, time };
    document.update(cx, |document, _| {
        let slide = document.shown_slide(place.slide);
        let on = document
            .terminals_at(slide, moment)
            .into_iter()
            .filter(|terminal| terminal.on.run.is_some() && terminal.source.is_some() && place.step >= terminal.click.unwrap_or(0))
            .collect();
        (on, slide)
    })
}

/// What the presenter sees: the slide as it stands, what's next, the notes and the time.
pub struct PresenterView {
    show: Entity<Presentation>,
    current: Entity<SlideCanvas>,
    upcoming: Entity<SlideCanvas>,
    notes: SharedString,
    notes_scroll: ScrollHandle,
    /// The slide whose notes are showing.
    slide: usize,
    /// Whether the slide on `current` moves, and the time it was last drawn at.
    moves: bool,
    drawn_time: f32,
    /// The wait for the next moment of a slide that moves.
    next_moment: Option<Task<()>>,
    at_the_end: bool,
    /// The terminals of the current slide that run a command: they show what the audience's window
    /// draws of them.
    live_on: Vec<TerminalLayer>,
    /// The interactive things of the current slide, whether or not they take a press now: a press
    /// on one is routed as in the audience's window.
    things: Vec<InteractiveThing>,
    /// Where the pointer is in the window, and the interactive thing it was last drawn over.
    pointer: Option<Point<Pixels>>,
    hovered: Option<Range<usize>>,
    rehearsing: bool,
    focus_handle: FocusHandle,
    _watching: [Subscription; 3],
    _clock: Task<()>,
}

impl PresenterView {
    fn new(
        show: Entity<Presentation>,
        rehearsing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = cx.new(|_| SlideCanvas::default());
        let upcoming = cx.new(|_| SlideCanvas::default());
        let watching = [
            cx.observe(&show, |this, _, cx| this.show_place(cx)),
            cx.subscribe(&current, |this, _, _: &ScaleChanged, cx| {
                this.show_place(cx)
            }),
            cx.subscribe(&upcoming, |this, _, _: &ScaleChanged, cx| {
                this.show_place(cx)
            }),
        ];
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK_TICK).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let mut view = PresenterView {
            show,
            current,
            upcoming,
            notes: SharedString::default(),
            notes_scroll: ScrollHandle::new(),
            slide: 0,
            moves: false,
            drawn_time: 0.0,
            next_moment: None,
            at_the_end: false,
            live_on: Vec::new(),
            things: Vec::new(),
            pointer: None,
            hovered: None,
            rehearsing,
            focus_handle,
            _watching: watching,
            _clock: clock,
        };
        view.show_place(cx);
        view
    }

    /// The canvas of the slide as it stands.
    pub fn current(&self) -> &Entity<SlideCanvas> {
        &self.current
    }

    /// The canvas of what the next press will bring.
    pub fn upcoming(&self) -> &Entity<SlideCanvas> {
        &self.upcoming
    }

    fn show_place(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let (upcoming, moves) = self
            .show
            .update(cx, |show, cx| (show.upcoming(cx), show.slide_moves(cx)));
        self.at_the_end = upcoming.is_none();
        self.moves = moves;
        let show = self.show.read(cx);
        let (document, place, time) = (
            show.document.clone(),
            show.session.place(),
            show.time_at(now),
        );
        let talk = document.read(cx);
        self.notes = talk
            .notes_of(talk.shown_slide(place.slide))
            .unwrap_or_default()
            .into();
        if place.slide != self.slide {
            self.slide = place.slide;
            self.notes_scroll.set_offset(Point::default());
        }
        show_on(&self.current, &document, place, time, cx);
        self.live_on = live_terminals_at(&document, place, time, cx).0;
        self.things = interactive_things_at(&document, place, time, cx);
        self.drawn_time = time;
        match upcoming {
            // What the next press brings, as it first looks.
            Some(upcoming) => {
                show_on(&self.upcoming, &document, upcoming, 0.0, cx);
            }
            None => self
                .upcoming
                .update(cx, |canvas, cx| canvas.set_layers(Vec::new(), cx)),
        }
        cx.notify();
    }

    /// Lays the running terminals of the current slide over their layers, to show the pictures the
    /// audience's window has them draw, and takes them off when none runs.
    fn apply_live(&mut self, cx: &mut Context<Self>) {
        let Some((_, live)) = self.show.read(cx).owner.clone() else { return };
        let keys = live.keys();
        let places: Vec<LivePlace> = self
            .live_on
            .iter()
            .filter_map(|terminal| {
                let id = LiveId::new(terminal.source.clone()?, terminal.on.run.clone()?);
                Some(LivePlace {
                    layer: terminal.layer,
                    session: live.weak(&id)?,
                    rect: terminal.on.rect,
                    rotate: terminal.on.rotate,
                    keys: keys.as_ref() == Some(&id),
                    follows: true,
                })
            })
            .collect();
        if places.is_empty() {
            self.current.update(cx, |canvas, cx| canvas.clear_live(cx));
            return;
        }
        let document = self.show.read(cx).document.clone();
        let fonts = document.update(cx, |document, _| document.fonts_seed());
        self.current.update(cx, |canvas, cx| canvas.place_live(places, fonts, cx));
    }

    /// The interactive thing at `position` in the window that takes a press now, if there's one.
    fn thing_at(&self, position: Point<Pixels>, cx: &App) -> Option<InteractiveThing> {
        let at = slide_point(&self.current, position, cx)?;
        interactive_at(&self.show.read(cx).taking_presses(&self.things), at).cloned()
    }

    /// A press on the slide, routed as in the audience's window: on a running terminal it takes the
    /// keys.
    fn press(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let hit = self.thing_at(event.position, cx);
        let on_slide = slide_point(&self.current, event.position, cx).is_some();
        if on_slide && press_for_keys(&self.show, hit.as_ref(), cx) {
            self.focus_handle.focus(window, cx);
        }
        self.show.update(cx, |show, _| show.press_on(hit.as_ref(), PressedIn::Presenter));
        cx.notify();
    }

    /// The pointer moved: what it's over is outlined, and a press let go of where this window didn't
    /// see it is for nothing.
    fn pointer_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.pointer = Some(event.position);
        if event.pressed_button != Some(MouseButton::Left) && self.show.read(cx).holds_press(PressedIn::Presenter) {
            self.show.update(cx, |show, cx| show.let_go(None, PressedIn::Presenter, cx));
        }
        if self.thing_at(event.position, cx).map(|thing| thing.source) != self.hovered {
            cx.notify();
        }
    }

    /// The button let go of at `position`: a press on an interactive thing is for it if it's let go
    /// of over it.
    fn let_go(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let over = self.thing_at(position, cx);
        self.show.update(cx, |show, cx| show.let_go(over.as_ref(), PressedIn::Presenter, cx));
        cx.notify();
    }

    /// Draws the slide on `current` again if it moves and its time has changed since, and
    /// comes back at its next moment while it moves.
    fn animate(&mut self, cx: &mut Context<Self>) {
        // A show seen only by the presenter starts the slide's time here.
        let now = cx.background_executor().now();
        self.show.update(cx, |show, _| show.first_frame_drawn(now));
        self.apply_live(cx);
        if !self.moves {
            return;
        }
        catch_up(&self.show, &self.current, &mut self.drawn_time, now, cx);
        redraw_at_next_moment(&mut self.next_moment, &self.show, now, cx, |view: &mut Self| {
            &mut view.next_moment
        });
    }
}

/// A figure at the foot of the presenter's slide.
fn readout(id: &'static str, text: String) -> impl IntoElement {
    let text = SharedString::from(text);
    div()
        .id(id)
        .test_support()
        .aria_label(text.clone())
        .text_3xl()
        .child(text)
}

impl Render for PresenterView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.animate(cx);
        let show = self.show.read(cx);
        let elapsed = cx
            .background_executor()
            .now()
            .saturating_duration_since(show.started_at)
            .as_secs();
        let counter = format!(
            "{} / {}",
            show.session.place().slide + 1,
            show.document.read(cx).shown_count()
        );
        let at_the_end = self.at_the_end;
        let title = self.rehearsing.then_some("Presenter");
        let document = show.document.clone();
        let hovered = self
            .pointer
            .and_then(|pointer| slide_point(&self.current, pointer, cx))
            .and_then(|at| interactive_at(&show.taking_presses(&self.things), at).cloned());
        self.hovered = hovered.as_ref().map(|thing| thing.source.clone());
        let pressed = hovered.as_ref().and_then(|thing| pressed_look(&self.current, show, thing, PressedIn::Presenter, cx));
        let outline = hovered.and_then(|thing| hover_outline(&self.current, &document, &thing, cx));

        let slide_and_figures = v_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(
                div()
                    .id(CURRENT_ID)
                    .test_support()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .bg(black())
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, window, cx| this.press(event, window, cx)))
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| this.pointer_to(event, cx)))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, event: &MouseUpEvent, _, cx| this.let_go(event.position, cx)))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(|this, event: &MouseUpEvent, _, cx| this.let_go(event.position, cx)))
                    .child(self.current.clone())
                    .children(pressed)
                    .children(outline),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(readout(TIMER_ID, clock_text(elapsed)))
                    .child(readout(COUNTER_ID, counter)),
            );
        let next = div()
            .id(NEXT_ID)
            .test_support()
            .aria_label(if at_the_end { END_OF_THE_TALK } else { "Next" })
            .relative()
            .flex_none()
            .w_full()
            .aspect_ratio(16. / 9.)
            .bg(black())
            .child(self.upcoming.clone())
            .when(at_the_end, |this| {
                this.child(
                    h_flex()
                        .absolute()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .text_color(white())
                        .child(END_OF_THE_TALK),
                )
            });
        let notes = div()
            .id(NOTES_ID)
            .test_support()
            .aria_label(self.notes.clone())
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.notes_scroll)
            .text_lg()
            .child(self.notes.clone());

        window_root(&self.show, &self.focus_handle, title, self.show.read(cx).keys_held())
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .items_stretch()
                    .gap_4()
                    .p_4()
                    .child(slide_and_figures)
                    .child(
                        v_flex()
                            .flex_none()
                            .w_1_3()
                            .gap_3()
                            .child(next)
                            .child(notes),
                    ),
            )
    }
}

/// Options for a window filling `screen`. With no screen known, it's an ordinary window.
fn full_screen(screen: Option<&Rc<dyn PlatformDisplay>>) -> WindowOptions {
    WindowOptions {
        window_bounds: screen.map(|screen| WindowBounds::Fullscreen(screen.bounds())),
        display_id: screen.map(|screen| screen.id()),
        titlebar: None,
        app_id: Some(APP_ID.into()),
        ..Default::default()
    }
}

fn ordinary_window(
    title: &'static str,
    bounds: Bounds<Pixels>,
    screen: Option<DisplayId>,
) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..TitleBar::title_bar_options()
        }),
        display_id: screen,
        app_id: Some(APP_ID.into()),
        ..TitleBar::window_options()
    }
}

/// The screen the app's active window is on, or else the primary one.
fn current_screen(cx: &mut App) -> Option<Rc<dyn PlatformDisplay>> {
    let active = cx.active_window().and_then(|active| {
        active
            .update(cx, |_, window, cx| window.display(cx))
            .ok()
            .flatten()
    });
    active.or_else(|| cx.primary_display())
}

/// How to open the audience's window and, if there's to be one, the presenter's.
fn plan_windows(rehearsing: bool, cx: &mut App) -> (WindowOptions, Option<WindowOptions>) {
    let screens = cx.displays();
    let primary = cx.primary_display().map(|screen| screen.id());
    // Numbered by place in the list, as a platform's own number for a screen may not fit a u32.
    let infos: Vec<ScreenInfo> = screens
        .iter()
        .zip(0..)
        .map(|(screen, id)| ScreenInfo {
            id,
            primary: Some(screen.id()) == primary,
        })
        .collect();
    let numbered = |id: u32| screens.get(id as usize);

    match choose_windows(&infos, rehearsing) {
        WindowPlan::Both {
            audience,
            presenter,
        } => (
            full_screen(numbered(audience)),
            Some(full_screen(numbered(presenter))),
        ),
        WindowPlan::AudienceOnly { screen } => (full_screen(screen.and_then(numbered)), None),
        WindowPlan::Rehearsal => {
            let screen = current_screen(cx);
            let area = screen.as_ref().map_or(
                Bounds {
                    origin: Point::default(),
                    size: SCREEN_UNKNOWN,
                },
                |screen| screen.visible_bounds(),
            );
            let id = screen.map(|screen| screen.id());
            let half = size(area.size.width / 2., area.size.height);
            let audience = Bounds {
                origin: area.origin,
                size: half,
            };
            let presenter = Bounds {
                origin: area.origin + point(half.width, px(0.)),
                size: half,
            };
            (
                ordinary_window("Audience", audience, id),
                Some(ordinary_window("Presenter", presenter, id)),
            )
        }
    }
}

/// Starts a show of `document`, from its first slide. It has one or two windows of its own, and
/// goes on until it's ended or one of them is closed. If no window can be opened the show
/// comes back already ended.
pub fn start_show(
    document: Entity<Document>,
    rehearsing: bool,
    cx: &mut App,
) -> Entity<Presentation> {
    start_show_for(document, rehearsing, None, cx)
}

/// [`start_show`] for a talk open in a window, `owner`, which has the say over what a terminal
/// on a slide runs, and shares the live terminals with the show. A show without one runs nothing:
/// every terminal shows its recording or still.
pub fn start_show_for(
    document: Entity<Document>,
    rehearsing: bool,
    owner: Option<Owner>,
    cx: &mut App,
) -> Entity<Presentation> {
    let (audience_options, presenter_options) = plan_windows(rehearsing, cx);
    // A show of a talk no window shows makes its stills itself.
    crate::video_player::make_stills(&document, cx);
    let show = cx.new(|cx| Presentation::new(document, owner, cx));
    let audience = gpui_kit::open_window(audience_options, cx, |window, cx| {
        cx.new(|cx| AudienceView::new(show.clone(), rehearsing, window, cx))
    })
    .ok()
    .map(ShowWindow::opened);
    // The presenter's goes last, to be the one with the keyboard.
    let presenter = presenter_options.and_then(|options| {
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| PresenterView::new(show.clone(), rehearsing, window, cx))
        })
        .ok()
        .map(ShowWindow::opened)
    });
    show.update(cx, |show, _| show.opened(audience, presenter));
    cx.activate(true);
    show
}
