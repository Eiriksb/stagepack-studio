//! A show's windows: the slides for the audience, and the presenter's view of them.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{Enter, LayerTiming, Moment, Transition};

use crate::document::Document;
use crate::present::{
    ENTER_SECONDS, MOMENTS_PER_SECOND, Moved, Place, ScreenInfo, Session, TRANSITION_SECONDS,
    WindowPlan, choose_windows, clock_text, layer_look, moment_time, slide_looks,
};
use crate::slide_canvas::{ScaleChanged, SlideCanvas};
use crate::workspace_view::APP_ID;

pub const AUDIENCE_ID: &str = "audience";
pub const CURRENT_ID: &str = "current-slide";
pub const NEXT_ID: &str = "next-slide";
pub const NOTES_ID: &str = "notes";
pub const TIMER_ID: &str = "timer";
pub const COUNTER_ID: &str = "counter";

const KEY_CONTEXT: &str = "Show";

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
    _talk_changed: Subscription,
    _window_closed: Subscription,
}

impl Presentation {
    fn new(document: Entity<Document>, cx: &mut Context<Self>) -> Self {
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
            _talk_changed: talk_changed,
            _window_closed: window_closed,
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

    /// Goes forward by a click, or to the next slide once this one's clicks have all come.
    pub fn next(&mut self, cx: &mut Context<Self>) {
        let left = self.session.place();
        let left_time = self.time_at(cx.background_executor().now());
        let moved = self
            .document
            .update(cx, |document, _| self.session.next(document));
        let began = cx.background_executor().now();
        match moved {
            Moved::Revealed { step } => self.reveal = Some(Reveal { step, began }),
            Moved::Arrived { transition, .. } => {
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
        }
    }

    /// Ends the show and closes its windows.
    pub fn end(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.ended, true) {
            return;
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

    /// Whether the slide the show is on moves, and so needs drawing anew as time passes.
    fn slide_moves(&self, cx: &mut App) -> bool {
        let slide = self.session.place().slide;
        self.document
            .update(cx, |document, _| document.timeline_of(document.shown_slide(slide)).animated)
    }
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

/// The root of a show's window, with the show's keys, and a title bar if `title` says what
/// to put on one. A full-screen window has none.
fn window_root(
    show: &Entity<Presentation>,
    focus_handle: &FocusHandle,
    title: Option<&'static str>,
) -> Div {
    let (next, previous, end) = (show.clone(), show.clone(), show.clone());
    v_flex()
        .key_context(KEY_CONTEXT)
        .track_focus(focus_handle)
        .on_action(move |_: &ShowNext, _, cx| next.update(cx, |show, cx| show.next(cx)))
        .on_action(move |_: &ShowPrevious, _, cx| previous.update(cx, |show, cx| show.previous(cx)))
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
    rehearsing: bool,
    focus_handle: FocusHandle,
    _watching: [Subscription; 3],
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
            rehearsing,
            focus_handle,
            _watching: watching,
        }
    }

    /// The canvas of the slide the show is on.
    pub fn current(&self) -> &Entity<SlideCanvas> {
        &self.current
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
        self.drawn_time = time;
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
        window_root(&self.show, &self.focus_handle, title)
            .bg(black())
            .child(
                div()
                    .id(AUDIENCE_ID)
                    .test_support()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show.update(cx, |show, cx| show.next(cx));
                    }))
                    .child(self.leaving.clone())
                    .child(self.current.clone()),
            )
    }
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

    /// Draws the slide on `current` again if it moves and its time has changed since, and
    /// comes back at its next moment while it moves.
    fn animate(&mut self, cx: &mut Context<Self>) {
        // A show seen only by the presenter starts the slide's time here.
        let now = cx.background_executor().now();
        self.show.update(cx, |show, _| show.first_frame_drawn(now));
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
                    .child(self.current.clone()),
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

        window_root(&self.show, &self.focus_handle, title)
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
    let (audience_options, presenter_options) = plan_windows(rehearsing, cx);
    let show = cx.new(|cx| Presentation::new(document, cx));
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
