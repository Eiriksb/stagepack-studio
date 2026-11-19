//! A 3D model drawn live on a slide: read from its file and put on the graphics card on a
//! background task, drawn off-screen by the app's one drawer on a thread of its own, and the
//! newest picture given to the canvas each time the window draws.
//!
//! The window's thread never waits for the drawer: it asks for a picture and looks, each time it
//! draws, whether it has come. Nothing here runs while nothing is turned: a turner asks the window
//! for another frame only while its view is moving or a picture it asked for is still to come.

use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::{App, RenderImage};
use slides_engine::{ModelOn, Pixmap, Rect, bgra_frame, picture_layer, to_bgra_frame};
use slides_model::{Answer, CantDraw, Drawer, ModelFacts, NotDrawn, Picture, Scene, View};

use crate::document::LargeOpen;
use crate::large_files::{HeldReader, NotOpened};
use crate::video_player::{Placed, Shown};

/// How long a model takes to fly from one view to the next, in the show.
pub const FLIGHT: Duration = Duration::from_secs(1);
/// How fast a model set to turn on its own goes round: once a minute.
pub const ORBIT_DEGREES_PER_SECOND: f32 = 6.0;
/// How far a model turns for each pixel it's dragged, in degrees.
pub const DEGREES_PER_PIXEL: f32 = 0.3;
/// How much nearer each step of the wheel brings a model.
pub const ZOOM_PER_STEP: f32 = 0.9;
/// How far up or down a model can be turned to look at it from, in degrees.
const MOST_PITCH: f32 = 89.0;
/// The longest side a live picture is drawn at.
const MOST_SIDE: f32 = 4096.0;
/// How long closing the drawer waits for its thread to let go of its device.
const CLOSING_WITHIN: Duration = Duration::from_secs(10);

// The app's one drawer.

/// The drawer every model is drawn with, once one is wanted.
static DRAWER: Mutex<Option<Drawer>> = Mutex::new(None);
/// The drawer being closed as the app quits, and the thread closing it.
static CLOSING: Mutex<Option<std::thread::JoinHandle<bool>>> = Mutex::new(None);

fn lock<T>(held: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The app's drawer, started the first time a model is to be drawn: its device is made on its
/// own thread, so this never waits.
pub fn drawer() -> Drawer {
    lock(&DRAWER).get_or_insert_with(Drawer::start).clone()
}

/// Draws with `drawer` from now on, as a test does to slow it down or take its adapter away. The
/// drawer it replaces is closed off this thread, and given back for a test to wait on.
#[doc(hidden)]
pub fn use_drawer(drawer: Drawer) -> Option<std::thread::JoinHandle<bool>> {
    let old = lock(&DRAWER).replace(drawer)?;
    std::thread::Builder::new().name("closing a model drawer".into()).spawn(move || old.close(CLOSING_WITHIN)).ok()
}

/// Closes the app's drawer, if it has one, waiting up to `within` for its thread to let go of its
/// device: true if it did, or there was none. It waits, so never on a window's thread while the
/// window is in use. The next model to be drawn starts another.
pub fn close_drawer(within: Duration) -> bool {
    let drawer = lock(&DRAWER).take();
    drawer.is_none_or(|drawer| drawer.close(within))
}

/// Begins closing the app's drawer on a thread of its own, as the app quits, and gives what says
/// when it's done. A program that ends while a graphics driver is letting go of a device can
/// crash in the driver, so [`wait_closed`] waits for it once the windows are gone.
pub fn begin_closing() -> futures::channel::oneshot::Receiver<()> {
    let (done, closed) = futures::channel::oneshot::channel();
    let Some(drawer) = lock(&DRAWER).take() else {
        done.send(()).ok();
        return closed;
    };
    let closing = std::thread::Builder::new().name("closing the model drawer".into()).spawn(move || {
        let closed = drawer.close(CLOSING_WITHIN);
        done.send(()).ok();
        closed
    });
    if let Ok(closing) = closing {
        *lock(&CLOSING) = Some(closing);
    }
    closed
}

/// Closes the app's drawer as the app quits, off the window's thread.
pub fn close_on_quit(cx: &mut App) {
    cx.on_app_quit(|_| {
        let closed = begin_closing();
        async move {
            closed.await.ok();
        }
    })
    .detach();
}

/// Waits for the drawer closed as the app quit to let go of its device: for once the windows
/// are gone, before the program ends. True if it did, or none was being closed.
pub fn wait_closed() -> bool {
    let closing = lock(&CLOSING).take();
    closing.is_none_or(|closing| closing.join().unwrap_or(false))
}

/// How many pictures of models have been asked of the drawer, stills and live ones, in all.
static DRAWS_ASKED: AtomicUsize = AtomicUsize::new(0);
/// How many models are on the drawer's card for this app, and how many have been put there.
static SCENES_HELD: AtomicUsize = AtomicUsize::new(0);
static SCENES_MADE: AtomicUsize = AtomicUsize::new(0);

/// How many pictures of models have been asked of the drawer, in all: for tests that nothing
/// draws for a model that isn't shown.
#[doc(hidden)]
pub fn draws_asked() -> usize {
    DRAWS_ASKED.load(Ordering::SeqCst)
}

/// How many models are on the graphics card for the app now: for tests that every one is let
/// go of.
#[doc(hidden)]
pub fn scenes_held() -> usize {
    SCENES_HELD.load(Ordering::SeqCst)
}

/// How many times a model has been put on the graphics card, in all: for tests of what's read
/// again.
#[doc(hidden)]
pub fn models_put_on_card() -> usize {
    SCENES_MADE.load(Ordering::SeqCst)
}

/// What a test has done each time a model is put on the card, as losing the card right after.
type AfterUpload = Box<dyn Fn(&Drawer) + Send>;
static AFTER_UPLOAD: Mutex<Option<AfterUpload>> = Mutex::new(None);

/// For tests: has `after` done with the drawer each time a model has just been put on its card,
/// or nothing with `None`.
#[doc(hidden)]
pub fn after_upload(after: Option<AfterUpload>) {
    *lock(&AFTER_UPLOAD) = after;
}

/// A model on a drawer's card, and the drawer it's on, counted while it's there.
#[doc(hidden)]
pub struct Uploaded {
    scene: Scene,
    drawer: Drawer,
}

impl Uploaded {
    fn new(drawer: Drawer, model: slides_model::Model) -> Uploaded {
        let scene = drawer.upload(model);
        SCENES_HELD.fetch_add(1, Ordering::SeqCst);
        SCENES_MADE.fetch_add(1, Ordering::SeqCst);
        if let Some(after) = &*lock(&AFTER_UPLOAD) {
            after(&drawer);
        }
        Uploaded { scene, drawer }
    }

    fn draw(&self, view: View, (width, height): (u32, u32)) -> Answer {
        DRAWS_ASKED.fetch_add(1, Ordering::SeqCst);
        self.drawer.draw(&self.scene, view, width, height)
    }
}

impl Drop for Uploaded {
    fn drop(&mut self) {
        SCENES_HELD.fetch_sub(1, Ordering::SeqCst);
    }
}

// A still.

/// One reader shared, so that a model can be read again from the start.
#[derive(Clone)]
struct Shared(Arc<Mutex<Box<dyn slides_engine::ReadSeek>>>);

impl Read for Shared {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        lock(&self.0).read(buf)
    }
}

impl Seek for Shared {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        lock(&self.0).seek(to)
    }
}

/// Draws the still of the model `reader` reads, seen from `view`, or framed whole with none,
/// `width` by `height`, with the app's drawer: what the deck is given to draw stills with. It
/// waits for the drawer, so it's for a background task. A model lost from the card before it's
/// drawn is read and put on again once; lost again, it can't be drawn.
pub fn draw_still(reader: Box<dyn slides_engine::ReadSeek>, view: Option<View>, width: u32, height: u32) -> Result<Picture, CantDraw> {
    let reader = Shared(Arc::new(Mutex::new(reader)));
    let mut again = false;
    loop {
        let model = slides_model::read(Box::new(reader.clone()))?;
        let view = view.unwrap_or_else(|| View::framing(&model.facts));
        let uploaded = Uploaded::new(drawer(), model);
        match uploaded.draw(view, (width, height)).recv() {
            Ok(picture) => return Ok(picture),
            Err(NotDrawn::Cant(why)) => return Err(why),
            Err(NotDrawn::UploadAgain) if !again => again = true,
            Err(NotDrawn::UploadAgain) => return Err(CantDraw::CardRefused),
            Err(NotDrawn::Replaced | NotDrawn::Closed) => return Err(CantDraw::NoAdapter),
        }
    }
}

// Turning.

/// What came of reading a model to turn it.
#[doc(hidden)]
pub enum Reading {
    /// It's on the card, and this is what it is.
    Read(Uploaded, ModelFacts),
    /// A save let go of its file, or was running: it's read again once that's done.
    LetGo,
    Cant(CantDraw),
}

impl Reading {
    /// Whether a save let go of its file, so that it's read again once that's done.
    pub fn was_let_go(&self) -> bool {
        matches!(self, Reading::LetGo)
    }
}

/// Reads the model `open` reads, through the talk's readers, and puts it on the drawer's card:
/// for a background task. Its file is let go of once it's read, and the model once it's on the
/// card.
pub(crate) fn read_model(open: LargeOpen) -> Reading {
    match open.open_unless_saving() {
        Ok(reader) => read_model_from(reader),
        Err(NotOpened::Saving) => Reading::LetGo,
        Err(NotOpened::Unreadable | NotOpened::NotUnpacked) => Reading::Cant(CantDraw::CouldNotRead),
    }
}

/// Reads the model `reader` reads, held among the talk's readers, and puts it on the drawer's
/// card. One that failed because a save let go of its file is read again after the save.
#[doc(hidden)]
pub fn read_model_from(reader: HeldReader) -> Reading {
    let watch = reader.watch();
    match slides_model::read(Box::new(reader)) {
        Ok(model) => {
            let facts = model.facts;
            Reading::Read(Uploaded::new(drawer(), model), facts)
        }
        // Only a save lets go of a file: one that failed otherwise can't be drawn.
        Err(_) if watch.is_let_go() => Reading::LetGo,
        Err(why) => Reading::Cant(why),
    }
}

enum State {
    /// Its file is being read on a background task.
    Reading,
    Ready(Uploaded),
    /// A save let go of its file: it's read again once that's done.
    LetGo,
    /// It can't be drawn here: its still shows.
    Cant(CantDraw),
}

/// Where a model's view goes at step `step` of its slide's show, `model` being one of its
/// models: its home until the click it appears on is past, and then a view a click.
pub fn aim_at(model: &slides_engine::ModelLayer, step: u32) -> Aim {
    let past = step.saturating_sub(model.click.unwrap_or(0)) as usize;
    match past.min(model.on.views.len()) {
        0 => Aim::Home,
        at => Aim::View(model.on.views[at - 1]),
    }
}

/// Where a turner's view is going in the show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aim {
    /// To the view it starts at.
    Home,
    /// To one of its views.
    View(View),
}

/// One model being turned, or shown live.
pub struct Turner {
    /// The place of the model's layer among the canvas's layers.
    pub(crate) layer: usize,
    pub(crate) on: ModelOn,
    /// What its file is read with, again after the card lost it.
    open: Option<LargeOpen>,
    state: State,
    /// The view it starts at, once it's known: the one written, or else the one that frames it.
    home: Option<View>,
    /// Where it's seen from, but for an orbit or a flight going on.
    base: Option<View>,
    /// Where the show has it go, once it's read if it isn't yet.
    aim: Aim,
    /// A flight from one view to another, and when it began, once it's drawn for.
    flight: Option<(View, View, Option<Instant>)>,
    /// Whether it flies from its home to where it's aimed once it's read.
    fly_when_read: bool,
    /// Whether it turns on its own, and since when it has from `base`.
    orbits: bool,
    orbit_began: Option<Instant>,
    /// Whether it's held by a drag, which stops it turning on its own meanwhile.
    held: bool,
    /// Where it was seen from when the window last drew it.
    seen: Option<View>,
    /// The picture asked for and not yet come, and what it's of.
    asked: Option<(Answer, View, (u32, u32))>,
    /// What the picture shown is of.
    drawn: Option<(View, (u32, u32))>,
    shown: Option<Shown>,
    /// How many times in a row the drawer said to put it on its card again.
    upload_again: u32,
    /// The size the drawer couldn't draw it at, which isn't asked for again.
    refused: Option<(u32, u32)>,
    /// Whether a picture is being turned with its box off the window's thread.
    turning: bool,
    turned_from: Arc<Mutex<Option<Pixmap>>>,
}

/// What a turner's advance came to.
#[derive(Default)]
pub(crate) struct Advanced {
    /// Whether a new picture was made to show.
    pub made: bool,
    /// The picture no longer shown, which the window must let go of.
    pub replaced: Option<Arc<RenderImage>>,
    /// A picture to turn with its box, off the window's thread, and then show.
    pub turning: Option<Turning>,
    /// Why it can't be drawn, if that's just been found.
    pub failed: Option<CantDraw>,
    /// Whether its file is to be read again, the card having lost it.
    pub read_again: Option<LargeOpen>,
}

impl Turner {
    /// A turner of `on`, the model of layer `layer`, being read with `open`: starting at its
    /// home view, and turning on its own if `orbits`.
    pub(crate) fn reading(layer: usize, on: ModelOn, open: LargeOpen, orbits: bool) -> Turner {
        Turner {
            layer,
            on,
            open: Some(open),
            state: State::Reading,
            home: None,
            base: None,
            aim: Aim::Home,
            flight: None,
            fly_when_read: false,
            orbits,
            orbit_began: None,
            held: false,
            seen: None,
            asked: None,
            drawn: None,
            shown: None,
            upload_again: 0,
            refused: None,
            turning: false,
            turned_from: Arc::default(),
        }
    }

    /// Starts it at `aim`, rather than flying there, once it's read.
    pub(crate) fn start_at(&mut self, aim: Aim) {
        self.aim = aim;
        self.fly_when_read = false;
    }

    /// Where the show has it go.
    pub(crate) fn aim(&self) -> Aim {
        self.aim
    }

    /// Takes in what came of reading it.
    pub(crate) fn read(&mut self, reading: Reading) -> Option<CantDraw> {
        match reading {
            Reading::Read(uploaded, facts) => {
                let home = self.on.view.unwrap_or_else(|| View::framing(&facts));
                self.home = Some(home);
                match (self.aim, self.base) {
                    // Read again after the card lost it: it goes on from where it was.
                    (_, Some(_)) => {}
                    // A view to fly to, asked for before it was read: it flies from its home.
                    (Aim::View(to), None) if self.fly_when_read => {
                        self.base = Some(home);
                        self.flight = Some((home, to, None));
                    }
                    (Aim::View(to), None) => self.base = Some(to),
                    (Aim::Home, None) => self.base = Some(home),
                }
                self.state = State::Ready(uploaded);
                // Drawn for the first time, or again on the card it's on now.
                self.drawn = None;
                None
            }
            Reading::LetGo => {
                self.state = State::LetGo;
                None
            }
            Reading::Cant(why) => {
                self.state = State::Cant(why);
                Some(why)
            }
        }
    }

    /// Reads it again, with `open`, as a save that let go of its file is done.
    pub(crate) fn reading_again(&mut self, open: LargeOpen) {
        self.open = Some(open);
        self.state = State::Reading;
        self.asked = None;
    }

    /// Whether a save let go of its file, so that it waits to read it again.
    pub(crate) fn was_let_go(&self) -> bool {
        matches!(self.state, State::LetGo)
    }

    /// Why it can't be drawn, if it can't.
    pub fn failed(&self) -> Option<CantDraw> {
        match self.state {
            State::Cant(why) => Some(why),
            _ => None,
        }
    }

    /// The picture shown now, if one is.
    pub(crate) fn shown(&self) -> Option<&Shown> {
        self.shown.as_ref()
    }

    /// Takes the picture shown away, for the window to let go of.
    pub(crate) fn take_shown(&mut self) -> Option<Arc<RenderImage>> {
        self.shown.take().map(|shown| shown.image)
    }

    /// Whether its view is moving on its own: flying, or turning on its own and not held.
    pub(crate) fn is_moving(&self) -> bool {
        self.flight.is_some() || (self.orbits && !self.held && self.base.is_some())
    }

    /// Whether it wants the window to draw again soon: its view moving, or a picture to come.
    pub(crate) fn wants_frames(&self) -> bool {
        matches!(self.state, State::Ready(_)) && (self.is_moving() || self.asked.is_some())
    }

    /// Whether a picture asked for of the drawer is still to come.
    pub(crate) fn waiting(&self) -> bool {
        self.asked.is_some()
    }

    /// Where it's seen from at `now`, once it's known.
    pub(crate) fn view_at(&mut self, now: Instant) -> Option<View> {
        let base = self.base?;
        if let Some((from, to, began)) = &mut self.flight {
            let began = *began.get_or_insert(now);
            let t = now.saturating_duration_since(began).as_secs_f32() / FLIGHT.as_secs_f32();
            if t < 1.0 {
                return Some(View::between(*from, *to, t));
            }
            let to = *to;
            self.flight = None;
            self.base = Some(to);
            self.orbit_began = Some(now);
            return Some(to);
        }
        if !self.orbits || self.held {
            return Some(base);
        }
        let began = *self.orbit_began.get_or_insert(now);
        let turned = now.saturating_duration_since(began).as_secs_f32() * ORBIT_DEGREES_PER_SECOND;
        Some(View { yaw: (base.yaw + turned).rem_euclid(360.0), ..base })
    }

    /// The view shown at `now`, held there: a flight or an orbit going on stops where it is.
    fn settle(&mut self, now: Instant) -> Option<View> {
        let view = self.view_at(now)?;
        self.flight = None;
        self.base = Some(view);
        self.orbit_began = Some(now);
        self.seen = None;
        Some(view)
    }

    /// Holds it, as a drag on it begins, or lets go of it as one ends: it doesn't turn on its
    /// own while it's held.
    pub(crate) fn hold(&mut self, held: bool, now: Instant) {
        self.settle(now);
        self.held = held;
    }

    /// Turns it as a drag of `dx` by `dy` pixels does: across turns it round, and up and down
    /// looks at it from higher or lower.
    pub(crate) fn turn_by(&mut self, dx: f32, dy: f32, now: Instant) {
        let Some(view) = self.settle(now) else { return };
        let pitch = (view.pitch + dy * DEGREES_PER_PIXEL).clamp(-MOST_PITCH, MOST_PITCH);
        self.base = Some(View { yaw: (view.yaw - dx * DEGREES_PER_PIXEL).rem_euclid(360.0), pitch, ..view });
    }

    /// Brings it nearer by `steps` of the wheel, or further with fewer than none.
    pub(crate) fn zoom_by(&mut self, steps: f32, now: Instant) {
        let Some(view) = self.settle(now) else { return };
        let distance = view.distance * ZOOM_PER_STEP.powf(steps);
        if distance.is_finite() && distance > 0.0 {
            self.base = Some(View { distance, ..view });
        }
    }

    /// Flies it to `aim` over [`FLIGHT`], from where it's seen now; before it's read, it flies
    /// from its home once it is.
    pub(crate) fn fly_to(&mut self, aim: Aim, now: Instant) {
        if aim == self.aim {
            return;
        }
        self.aim = aim;
        let Some(from) = self.view_at(now) else {
            // Not yet read: it flies from its home once it is.
            self.fly_when_read = matches!(aim, Aim::View(_));
            return;
        };
        let to = match aim {
            Aim::Home => self.home.unwrap_or(from),
            Aim::View(view) => view,
        };
        self.flight = Some((from, to, Some(now)));
    }

    /// Where it's seen from, as the window last drew it, or as a drag or the wheel has just
    /// left it: for writing it into the talk.
    pub fn view(&self) -> Option<View> {
        self.seen.or(self.base)
    }

    /// Takes in the picture asked for, if it's come, and asks for the next if its view or size
    /// has changed, `scale` being the scale the canvas's layers are drawn at: one picture at a
    /// time, and never at a size the drawer couldn't draw it at.
    pub(crate) fn advance(&mut self, now: Instant, scale: Option<f32>) -> Advanced {
        let mut advanced = Advanced::default();
        if let Some(answer) = self.asked.as_ref().and_then(|(answer, _, _)| answer.try_recv()) {
            let (_, view, size) = self.asked.take().expect("a picture was asked for");
            match answer {
                Ok(picture) => {
                    self.upload_again = 0;
                    self.drawn = Some((view, size));
                    if let Some(scale) = scale.filter(|_| self.on.rotate != 0.0) {
                        self.turning = true;
                        advanced.turning = Some(Turning { picture, rect: self.on.rect, rotate: self.on.rotate, scale, from: self.turned_from.clone() });
                    } else if let Some(shown) = show(picture) {
                        advanced.made = true;
                        advanced.replaced = self.shown.replace(shown).map(|shown| shown.image);
                    }
                }
                Err(NotDrawn::UploadAgain) if self.upload_again == 0 => {
                    // The card lost it: it's read again, once.
                    self.upload_again += 1;
                    self.state = State::Reading;
                    advanced.read_again = self.open.clone();
                }
                // Lost again with no picture between: it can't be drawn here.
                Err(NotDrawn::UploadAgain) => advanced.failed = Some(self.fail(CantDraw::CardRefused, &mut advanced.replaced)),
                Err(NotDrawn::Cant(why)) => {
                    // Not asked for at this size again.
                    self.refused = Some(size);
                    advanced.replaced = self.shown.take().map(|shown| shown.image);
                    advanced.failed = Some(why);
                }
                Err(NotDrawn::Replaced) => {}
                Err(NotDrawn::Closed) => advanced.failed = Some(self.fail(CantDraw::NoAdapter, &mut advanced.replaced)),
            }
        }
        let view = self.view_at(now);
        self.seen = view;
        if let (State::Ready(uploaded), None, Some(view), Some(scale), false) = (&self.state, &self.asked, view, scale, self.turning) {
            let size = picture_size(self.on.rect, scale);
            if self.drawn != Some((view, size)) && self.refused != Some(size) {
                self.asked = Some((uploaded.draw(view, size), view, size));
            }
        }
        advanced
    }

    /// Says it can't be drawn, for `why`: its still shows again.
    fn fail(&mut self, why: CantDraw, replaced: &mut Option<Arc<RenderImage>>) -> CantDraw {
        self.state = State::Cant(why);
        *replaced = self.shown.take().map(|shown| shown.image);
        why
    }

    /// Takes in a picture turned with its box, and gives the one it replaces, for the window to
    /// let go of.
    pub(crate) fn turned(&mut self, shown: Option<Shown>) -> Option<Arc<RenderImage>> {
        self.turning = false;
        if matches!(self.state, State::Cant(_)) {
            return shown.map(|shown| shown.image);
        }
        self.shown.replace(shown?).map(|shown| shown.image)
    }

    /// Is drawn at `on` from now on, as its layer is placed now.
    pub(crate) fn place(&mut self, on: ModelOn) {
        self.on = on;
    }
}

/// Whether `(x, y)`, in slide units, is on a box at `rect` turned by `rotate` degrees.
pub(crate) fn inside_turned(rect: Rect, rotate: f32, (x, y): (f32, f32)) -> bool {
    let (middle_x, middle_y) = (rect.x + rect.w / 2., rect.y + rect.h / 2.);
    let (sin, cos) = (-rotate).to_radians().sin_cos();
    let (dx, dy) = (x - middle_x, y - middle_y);
    let (along, down) = (dx * cos - dy * sin, dx * sin + dy * cos);
    along.abs() <= rect.w / 2. && down.abs() <= rect.h / 2.
}

/// The size in pixels a model placed at `rect` is drawn at, at `scale`: as large as its box
/// shows, [`MOST_SIDE`] a side at most.
pub(crate) fn picture_size(rect: Rect, scale: f32) -> (u32, u32) {
    let (w, h) = (rect.w * scale, rect.h * scale);
    let (w, h) = (if w.is_finite() { w.max(1.0) } else { 1.0 }, if h.is_finite() { h.max(1.0) } else { 1.0 });
    let shrink = (MOST_SIDE / w.max(h)).min(1.0);
    (((w * shrink).round() as u32).max(1), ((h * shrink).round() as u32).max(1))
}

/// A model's picture to be turned with its box, off the window's thread.
pub(crate) struct Turning {
    picture: Picture,
    rect: Rect,
    rotate: f32,
    scale: f32,
    from: Arc<Mutex<Option<Pixmap>>>,
}

impl Turning {
    /// The picture turned, in the window's image type: for a background task.
    pub(crate) fn turn(self) -> Option<Shown> {
        let Turning { picture, rect, rotate, scale, from } = self;
        let mut from = lock(&from);
        let layer = picture_layer(picture.width, picture.height, &picture.bgra, rect, rotate, scale, &mut from)?;
        let placed = Placed::Drawn { x: layer.x, y: layer.y, width: layer.pixmap.width(), height: layer.pixmap.height(), scale, for_rect: rect };
        let image = RenderImage::new(vec![to_bgra_frame(&layer.pixmap)]);
        Some(Shown { image: Arc::new(image), at: 0.0, placed })
    }
}

/// `picture` as the window shows it over its layer, stretched to the layer's rectangle.
fn show(picture: Picture) -> Option<Shown> {
    let Picture { width, height, bgra, .. } = picture;
    let image = RenderImage::new(vec![bgra_frame(width, height, bgra)?]);
    Some(Shown { image: Arc::new(image), at: 0.0, placed: Placed::Rect })
}
