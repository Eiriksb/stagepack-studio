//! The things on a slide in the show that a press is for, rather than the show: a press, a drag or
//! the wheel on one goes to it, and the show doesn't move. Both of a show's windows find them the
//! same way. And what an interactive component of the talk's keeps while its slide is shown.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::{Context, Entity, Point, Task};
use slides_engine::{BUTTON_STEP_PAST, ButtonAction, ButtonOn, ComponentAnswer, ComponentFeed, Moment, Rect};
use toolkit::{Event, PointerKind};

use crate::document::Document;
use crate::live_terminal::{LiveId, untypable};
use crate::model_view::inside_turned;

/// What an interactive thing is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum InteractiveKind {
    /// A button, which acts when it's pressed and let go of over it.
    Button,
    /// A terminal that runs a command: it takes the keys while it runs.
    Terminal(LiveId),
    /// A model, by the place of its layer among the slide's: a drag turns it, the wheel zooms it.
    Model(usize),
    /// A talk's own component that says it's interactive: what the pointer does on it, and a
    /// button's `do`, are sent to it. `hits` are what it named under the pointer, and where.
    Component { hits: Vec<(String, Rect)> },
    /// A plain box the tests put on a slide, which says what reached it.
    #[cfg(feature = "test-hooks")]
    StandIn,
}

/// An interactive thing on a slide, in slide units: where its tag is written, where it is, turned by
/// `rotate` degrees, what it is, and the `id` its tag gives, by which a button names it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InteractiveThing {
    pub source: Range<usize>,
    pub rect: Rect,
    pub rotate: f32,
    pub kind: InteractiveKind,
    pub id: Option<String>,
}

/// The interactive thing at `at`, in slide units, among `things`, which are in the order they're
/// drawn: the one in front.
pub(crate) fn interactive_at(things: &[InteractiveThing], at: Point<f32>) -> Option<&InteractiveThing> {
    things.iter().rev().find(|thing| inside_turned(thing.rect, thing.rotate, (at.x, at.y)))
}

/// The interactive things of the talk's slide `slide` at `moment`, in the order they're drawn: only
/// those shown by then. A terminal is among them whether or not its command runs yet.
pub(crate) fn interactive_on(document: &mut Document, slide: usize, moment: Moment) -> Vec<InteractiveThing> {
    let shown = |click: Option<u32>| click.is_none_or(|click| click <= moment.step);
    let terminals = document.terminals_at(slide, moment).into_iter().filter(|terminal| shown(terminal.click)).filter_map(|terminal| {
        let source = terminal.source?;
        let live = LiveId::new(source.clone(), terminal.on.run?);
        Some((terminal.layer, InteractiveThing { source, rect: terminal.on.rect, rotate: terminal.on.rotate, kind: InteractiveKind::Terminal(live), id: terminal.id }))
    });
    let models = document.models_at(slide, moment).into_iter().filter(|model| shown(model.click)).filter_map(|model| {
        let source = model.source?;
        Some((model.layer, InteractiveThing { source, rect: model.on.rect, rotate: model.on.rotate, kind: InteractiveKind::Model(model.layer), id: model.id }))
    });
    let buttons = document.buttons_at(slide, moment).into_iter().filter(|button| shown(button.click)).map(|button| {
        let on = button.on;
        (button.layer, InteractiveThing { source: button.source, rect: on.rect, rotate: on.rotate, kind: InteractiveKind::Button, id: on.id })
    });
    let components = document.components_at(slide, moment).into_iter().filter(|component| shown(component.click)).map(|component| {
        let on = component.on;
        (component.layer, InteractiveThing { source: component.source, rect: on.rect, rotate: on.rotate, kind: InteractiveKind::Component { hits: on.hits }, id: component.id })
    });
    let mut things: Vec<(usize, InteractiveThing)> = terminals.chain(models).chain(buttons).chain(components).collect();
    things.sort_by_key(|(layer, _)| *layer);
    things.into_iter().map(|(_, thing)| thing).collect()
}

/// What's said of a button whose `run` text is empty.
pub const BUTTON_TEXT_EMPTY: &str = "This button's text is empty.";

/// What pressing a button is to do, from what it says and the clicks its slide takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Press {
    /// Nothing: it has no action, or more than one, and its notice says so.
    Nothing,
    /// Go to this click step of the slide.
    Step(u32),
    /// Type `text`, then Enter, into the live terminal on the slide whose `id` is `terminal`.
    Run { text: String, terminal: String },
    /// Send the action `name`, with `value`, to the interactive component on the slide whose `id`
    /// is `target`.
    Do { name: String, target: String, value: String },
    /// Nothing, for this, which the status line says.
    Refused(String),
}

/// What came of acting on a button's press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Acted {
    /// It did what it does, or there was nothing for it to do.
    Done,
    /// It waits for a yes, which is being asked.
    Asked,
    /// It did nothing, for this, the one line the status line shows.
    Refused(String),
}

/// What pressing `button` is to do, on a slide that takes `steps` clicks. A text that can't be typed
/// is refused here, before anything is asked.
pub(crate) fn press_button(button: &ButtonOn, steps: u32) -> Press {
    let Ok(action) = &button.action else { return Press::Nothing };
    match action {
        ButtonAction::Step(step) if *step > steps => Press::Refused(BUTTON_STEP_PAST.to_string()),
        ButtonAction::Step(step) => Press::Step(*step),
        ButtonAction::Do { action, target, value } => Press::Do { name: action.clone(), target: target.clone(), value: value.clone() },
        ButtonAction::Run { text, .. } if text.is_empty() => Press::Refused(BUTTON_TEXT_EMPTY.to_string()),
        ButtonAction::Run { text, terminal } => match untypable(text) {
            Some(why) => Press::Refused(why.to_string()),
            None => Press::Run { text: text.clone(), terminal: terminal.clone() },
        },
    }
}

/// What the pointer doing `kind` at `at` on the slide, in slide units, is to the interactive
/// component `thing`: where it is in the component's own units, with its box's top-left at 0,0, and
/// what the component named in front under it. `None` if `thing` is no component.
pub(crate) fn pointer_event(thing: &InteractiveThing, at: Point<f32>, kind: PointerKind) -> Option<Event> {
    let InteractiveKind::Component { hits } = &thing.kind else { return None };
    let rect = thing.rect;
    let (middle_x, middle_y) = (rect.x + rect.w / 2., rect.y + rect.h / 2.);
    let (sin, cos) = (-thing.rotate).to_radians().sin_cos();
    let (dx, dy) = (at.x - middle_x, at.y - middle_y);
    let (x, y) = (dx * cos - dy * sin + rect.w / 2., dx * sin + dy * cos + rect.h / 2.);
    let inside = |hit: &Rect| x >= hit.x && x <= hit.x + hit.w && y >= hit.y && y <= hit.y + hit.h;
    let hit = hits.iter().rev().find(|(_, rect)| inside(rect)).map(|(name, _)| name.clone());
    Some(Event::Pointer { kind, x, y, hit })
}

/// The most events that go to one render of a component.
pub const MAX_EVENTS: usize = 64;

/// The least time between two renders of one component from what reached it: 60 a second.
pub const RENDER_EVERY: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// What one interactive component has, while its slide is shown.
#[derive(Default)]
struct Held {
    /// Its tag as it was written when something was first sent to it: an edit to it starts it afresh.
    text: String,
    /// What its last render kept.
    state: Option<serde_json::Value>,
    /// What reached it since it was last fed.
    queue: Vec<Event>,
    /// What it was last given, which it's drawn from until it's given more.
    fed: Option<ComponentFeed>,
    /// Whether what came of that is yet to be known: it's given no more until it is.
    waiting: bool,
    last_fed: Option<Instant>,
    /// Whether its render failed while it was fed: it's sent nothing more, and shows its problem.
    failed: bool,
    /// How many times it's been drawn from what it was fed.
    renders: usize,
}

/// What [`StateStore::feed_due`] did: whether it gave a component more to draw from, and how long
/// until it can give one that has to wait.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fed {
    pub fed: bool,
    pub wait: Option<Duration>,
}

/// What a slide's interactive components have, by where their tags are written: the state each
/// keeps, and what reached it since its last render. It lives while the slide is shown: a show holds
/// one, and so does whatever else tries them. Nothing of it is written to the talk.
#[derive(Default)]
pub struct StateStore {
    held: HashMap<Range<usize>, Held>,
}

/// How many feeds every store has made: each is told from every other by it, so no two feeds are
/// alike, in one show, the next, or the editor's.
static FEEDS_MADE: AtomicU64 = AtomicU64::new(0);

impl StateStore {
    /// Sends `event` to the component whose tag, written `text`, is at `source`. Of the moves since
    /// it was last fed only the last is kept, and at most [`MAX_EVENTS`]: a move goes first, and
    /// past them what comes is dropped. A component whose render failed is sent nothing.
    pub fn send(&mut self, source: &Range<usize>, text: &str, event: Event) {
        let held = self.held.entry(source.clone()).or_insert_with(|| Held { text: text.to_string(), ..Held::default() });
        if held.failed {
            return;
        }
        let moves = |event: &Event| matches!(event, Event::Pointer { kind: PointerKind::Move, .. });
        if moves(&event) {
            held.queue.retain(|queued| !moves(queued));
        }
        held.queue.push(event);
        if held.queue.len() > MAX_EVENTS {
            match held.queue.iter().position(moves) {
                Some(at) => held.queue.remove(at),
                None => held.queue.pop().expect("the queue isn't empty"),
            };
        }
    }

    /// Gives each component what reached it, if it's been at least [`RENDER_EVERY`] since it was
    /// last given something, at `now`, and what came of that is known.
    pub fn feed_due(&mut self, now: Instant) -> Fed {
        let mut done = Fed::default();
        for held in self.held.values_mut() {
            if held.queue.is_empty() || held.waiting || held.failed {
                continue;
            }
            if let Some(since) = held.last_fed.map(|at| now.saturating_duration_since(at))
                && since < RENDER_EVERY
            {
                let wait = RENDER_EVERY - since;
                done.wait = Some(done.wait.map_or(wait, |other| other.min(wait)));
                continue;
            }
            let made = FEEDS_MADE.fetch_add(1, Ordering::Relaxed) + 1;
            let events = std::mem::take(&mut held.queue);
            let mut hasher = DefaultHasher::new();
            (made, serde_json::to_string(&(&held.state, &events)).unwrap_or_default()).hash(&mut hasher);
            held.fed = Some(ComponentFeed { state: held.state.clone(), events, hash: hasher.finish().max(1) });
            held.waiting = true;
            held.last_fed = Some(now);
            done.fed = true;
        }
        done
    }

    /// What each component is drawn from, for the deck.
    pub fn feeds(&self) -> HashMap<Range<usize>, ComponentFeed> {
        self.held.iter().filter_map(|(source, held)| Some((source.clone(), held.fed.clone()?))).collect()
    }

    /// The state the slide is drawn at, for its [`Moment`]: 0 while nothing has been fed.
    pub fn state(&self) -> u64 {
        let mut fed: Vec<(usize, usize, u64)> = self.held.iter().filter_map(|(source, held)| Some((source.start, source.end, held.fed.as_ref()?.hash))).collect();
        if fed.is_empty() {
            return 0;
        }
        fed.sort_unstable();
        let mut hasher = DefaultHasher::new();
        fed.hash(&mut hasher);
        hasher.finish().max(1)
    }

    /// Takes in what came of drawing components from their feeds: the state each keeps, or that it
    /// failed. Says whether any of them can be fed again.
    pub fn answered(&mut self, answers: Vec<ComponentAnswer>) -> bool {
        let mut changed = false;
        for answer in answers {
            let Some(held) = self.held.get_mut(&answer.source) else { continue };
            if held.fed.as_ref().is_none_or(|fed| fed.hash != answer.hash) {
                continue;
            }
            held.renders += 1;
            // Drawn again from the same feed, it says what it said before.
            if !std::mem::take(&mut held.waiting) {
                continue;
            }
            changed = true;
            match answer.state {
                Ok(Some(state)) => held.state = Some(state),
                Ok(None) => {}
                Err(_) => {
                    held.failed = true;
                    held.queue.clear();
                }
            }
        }
        changed
    }

    /// Forgets everything: the slide was left.
    pub fn clear(&mut self) {
        self.held.clear();
    }

    /// After an edit of the talk, now `source`: keeps what each component has only if it's still
    /// one of `interactive` and its tag is written as it was, where it was.
    pub fn keep_unchanged(&mut self, source: &str, interactive: &[Range<usize>]) {
        self.held.retain(|at, held| interactive.contains(at) && source.get(at.clone()) == Some(held.text.as_str()));
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// How many times the component at `source` has been drawn from what it was fed.
    pub fn renders(&self, source: &Range<usize>) -> usize {
        self.held.get(source).map_or(0, |held| held.renders)
    }

    /// How many events wait for the component at `source`.
    pub fn queued(&self, source: &Range<usize>) -> usize {
        self.held.get(source).map_or(0, |held| held.queue.len())
    }

    /// What the component at `source` was last given.
    pub fn last_fed(&self, source: &Range<usize>) -> Option<&ComponentFeed> {
        self.held.get(source)?.fed.as_ref()
    }

    /// What the component at `source` keeps.
    pub fn state_of(&self, source: &Range<usize>) -> Option<&serde_json::Value> {
        self.held.get(source)?.state.as_ref()
    }
}

/// What a slide's interactive components keep, and the wait until one of them can be given what
/// reached it: a show holds one for the slide it's on, and the editor one for what it tries.
#[derive(Default)]
pub(crate) struct Components {
    pub store: StateStore,
    feeding: Option<Task<()>>,
}

/// What holds [`Components`] for the talk `components_document`, if it has one, and draws them
/// again once they're given what reached them.
pub(crate) trait HoldsComponents: Sized + 'static {
    fn components_mut(&mut self) -> &mut Components;
    fn components_document(&self) -> Option<&Entity<Document>>;
    /// The components were given what reached them: they're to be drawn from it.
    fn components_fed(&mut self, cx: &mut Context<Self>);
}

/// Sends `event` to the interactive component whose tag is written at `source`. It's drawn from
/// what reached it at most 60 times a second.
pub(crate) fn send_to<T: HoldsComponents>(this: &mut T, source: &Range<usize>, event: Event, cx: &mut Context<T>) {
    let Some(document) = this.components_document() else { return };
    let text = document.read(cx).source().get(source.clone()).unwrap_or_default().to_string();
    this.components_mut().store.send(source, &text, event);
    feed(this, cx);
}

/// Gives each component what reached it, if it's time to, and waits for when it is if not.
pub(crate) fn feed<T: HoldsComponents>(this: &mut T, cx: &mut Context<T>) {
    let now = cx.background_executor().now();
    let fed = this.components_mut().store.feed_due(now);
    if fed.fed {
        give_feeds(this, cx);
        this.components_fed(cx);
    }
    if let Some(wait) = fed.wait
        && this.components_mut().feeding.is_none()
    {
        let waiting = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| {
                this.components_mut().feeding = None;
                feed(this, cx);
            })
            .ok();
        });
        this.components_mut().feeding = Some(waiting);
    }
}

/// Has the deck draw the components from what the store gives them.
pub(crate) fn give_feeds<T: HoldsComponents>(this: &mut T, cx: &mut Context<T>) {
    let feeds = this.components_mut().store.feeds();
    if let Some(document) = this.components_document() {
        document.update(cx, |document, _| document.feed_components(feeds));
    }
}

/// Takes in what came of drawing the components from what they were given, and gives them what
/// reached them since.
pub(crate) fn take_answers<T: HoldsComponents>(this: &mut T, cx: &mut Context<T>) {
    let Some(document) = this.components_document() else { return };
    let answers = document.update(cx, |document, _| document.take_component_answers());
    if !answers.is_empty() && this.components_mut().store.answered(answers) {
        feed(this, cx);
    }
}

/// Forgets what the components kept, and what reached them.
pub(crate) fn forget<T: HoldsComponents>(this: &mut T, cx: &mut Context<T>) {
    let components = this.components_mut();
    components.feeding = None;
    if !components.store.is_empty() {
        components.store.clear();
        give_feeds(this, cx);
    }
}

/// What reached a test's box, and in which window.
#[cfg(feature = "test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandInPress {
    /// A press on it.
    Down(PressedIn),
    /// The press let go of over it.
    Up(PressedIn),
}

/// Which of a show's windows a press began in: that window has it until it's let go of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressedIn {
    Audience,
    Presenter,
}
