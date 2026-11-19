//! Shows a slide's layers, each its own image in the window's sprite atlas.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use slides_engine::{Layer, ModelLayer, ModelOn, Rect, VideoLayer, VideoOn, to_bgra_frame};
use slides_model::{CantDraw, View};

use crate::document::LargeOpen;
use crate::model_view::{self, Aim, Turner};
use crate::present::{LayerLook, SlideLook};
use crate::video_player::{Placed, Player, Shown, Turning, discard, open_and_play};

/// The width of a 16:9 slide, in slide units.
pub use slides_engine::SLIDE_WIDTH;

/// How far the slide can grow or shrink before its layers are drawn again. Below this they
/// are stretched to fit, so a resize doesn't draw the slide again for every pixel.
const REDRAW_CHANGE: f32 = 0.01;

/// Sent when the slide has changed size enough that its layers should be drawn again, at
/// [`SlideCanvas::scale`].
pub struct ScaleChanged;

/// Sent when a save has let go of the file of a video playing here: whatever plays it opens it
/// again, with [`SlideCanvas::open_videos_again`], once the save is done.
pub struct VideoLetGo;

/// Sent when a video playing here couldn't go on, saying why.
pub struct VideoFailed(pub slides_video::CantPlay);

/// Sent when a video has been opened to play: one a pack holds compressed is unpacked by then,
/// and its still can be made.
pub struct VideoOpened;

/// Sent when a model turned here can't be drawn, saying why.
pub struct ModelFailed(pub CantDraw);

/// Sent when a save has let go of the file of a model being read here, or the graphics card lost
/// it: whatever turns it reads it again, from where the talk is now, with
/// [`SlideCanvas::read_models_again`], once any save is done.
pub struct ModelLetGo;

#[derive(Clone)]
struct Sprite {
    layer: Arc<Layer>,
    image: Arc<RenderImage>,
}

/// A layer to paint, how far below its place it is, whether it's painted itself, and the
/// pictures of videos and models over it, each with where its layer is now.
type RunSprite = (Sprite, f32, bool, Vec<(Shown, Rect)>);

/// Showing layers next to each other that share an opacity. Each sprite comes with how far
/// below its place it is, in slide units, and the pictures of the videos playing over it.
struct Run {
    opacity: f32,
    sprites: Vec<RunSprite>,
}

#[derive(Default)]
pub struct SlideCanvas {
    sprites: Vec<Sprite>,
    looks: Vec<LayerLook>,
    slide_look: SlideLook,
    /// Images no layer uses any more. Only a paint can release them, as it has the window.
    retired: Vec<Arc<RenderImage>>,
    scale: Option<f32>,
    /// Whether a line is drawn around the slide, as the editor does on its light desk.
    framed: bool,
    /// Where that line was last drawn.
    edge: Option<Bounds<Pixels>>,
    /// Where the slide was last drawn, and the area it was drawn in.
    placed: Option<(Bounds<Pixels>, Bounds<Pixels>)>,
    /// The videos playing over its layers, each with a number of its own.
    players: Vec<(u64, Player)>,
    players_made: u64,
    /// Pictures of videos no longer shown, which the next paint releases.
    retired_frames: Vec<Arc<RenderImage>>,
    /// How many pictures of videos have been made to show, and released, in all: released as
    /// the window lets go of them, as it paints.
    frames_made: usize,
    frames_released: Rc<Cell<usize>>,
    /// How many pictures of videos were passed over, late, in all.
    frames_skipped: usize,
    /// How long taking the videos' pictures last took the window's thread.
    last_advance: std::time::Duration,
    /// When the counts were last written out, and what they were then: see [`COUNTS_TO_STDERR`].
    counted: Option<(std::time::Instant, usize, usize, usize)>,
    /// The models drawn live over their layers, each with a number of its own.
    turners: Vec<(u64, Turner)>,
    turners_made: u64,
    /// Live pictures of models no longer shown, which the next paint releases.
    retired_models: Vec<Arc<RenderImage>>,
    /// How many live pictures of models have been made to show, and released, in all.
    models_made: usize,
    models_released: Rc<Cell<usize>>,
    /// When the models' counts were last written out, and what they were then.
    models_counted: Option<(std::time::Instant, usize, usize)>,
}

/// Set, to anything, to have a window playing a video write how many of its pictures were
/// shown, skipped and released each second to the standard error, and one turning a 3D model how
/// many of its pictures were drawn and shown, and released: for checking by hand how smoothly a
/// video plays or a model turns on a computer.
pub const COUNTS_TO_STDERR: &str = "STUDIO_VIDEO_COUNTS";

fn counts_wanted() -> bool {
    static WANTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *WANTED.get_or_init(|| std::env::var_os(COUNTS_TO_STDERR).is_some())
}

impl EventEmitter<ScaleChanged> for SlideCanvas {}
impl EventEmitter<VideoLetGo> for SlideCanvas {}
impl EventEmitter<VideoOpened> for SlideCanvas {}
impl EventEmitter<VideoFailed> for SlideCanvas {}
impl EventEmitter<ModelFailed> for SlideCanvas {}
impl EventEmitter<ModelLetGo> for SlideCanvas {}

impl SlideCanvas {
    /// A canvas with a line around its slide, for the editor. A show's has none.
    pub fn framed() -> Self {
        SlideCanvas {
            framed: true,
            ..SlideCanvas::default()
        }
    }

    /// Where the line around the slide was last drawn, just outside it, if it has one.
    pub fn edge(&self) -> Option<Bounds<Pixels>> {
        self.edge
    }

    /// Where the slide was last drawn in the window.
    pub fn slide_bounds(&self) -> Option<Bounds<Pixels>> {
        self.placed.map(|(slide, _)| slide)
    }

    /// Where the area the slide sits in, its bars beside it included, was last drawn.
    pub fn area(&self) -> Option<Bounds<Pixels>> {
        self.placed.map(|(_, area)| area)
    }

    /// The layers to show, first to last, drawn at [`Self::scale`]. A layer that was
    /// already showing keeps its image.
    pub fn set_layers(&mut self, layers: Vec<Arc<Layer>>, cx: &mut Context<Self>) {
        // Every old layer is still alive here, so no two pointers can be mistaken for each other.
        let mut unused: HashMap<*const Layer, Arc<RenderImage>> = self
            .sprites
            .drain(..)
            .map(|sprite| (Arc::as_ptr(&sprite.layer), sprite.image))
            .collect();
        // A slide can show one layer twice, and that's one image.
        let mut used: HashMap<*const Layer, Arc<RenderImage>> = HashMap::new();
        self.sprites = layers
            .into_iter()
            .map(|layer| {
                let key = Arc::as_ptr(&layer);
                let image = used
                    .entry(key)
                    .or_insert_with(|| {
                        unused.remove(&key).unwrap_or_else(|| {
                            Arc::new(RenderImage::new(vec![to_bgra_frame(&layer.pixmap)]))
                        })
                    })
                    .clone();
                Sprite { layer, image }
            })
            .collect();
        self.retired.extend(unused.into_values());
        cx.notify();
    }

    /// How each layer looks, first to last. A layer the list doesn't reach is fully visible.
    /// A hidden layer paints nothing, but keeps its image, so that it can come back at once.
    /// The looks outlast [`Self::set_layers`], so a redraw at a new size brings nothing back.
    pub fn set_looks(&mut self, looks: Vec<LayerLook>, cx: &mut Context<Self>) {
        self.looks = looks;
        cx.notify();
    }

    /// How each layer looks, as last set.
    pub fn looks(&self) -> &[LayerLook] {
        &self.looks
    }

    /// How the whole slide looks.
    pub fn set_slide_look(&mut self, look: SlideLook, cx: &mut Context<Self>) {
        self.slide_look = look;
        cx.notify();
    }

    /// How the whole slide looks, as last set.
    pub fn slide_look(&self) -> SlideLook {
        self.slide_look
    }

    /// The scale to draw layers at: the slide's width in device pixels over 1920. `None`
    /// until the first paint has measured the slide.
    pub fn scale(&self) -> Option<f32> {
        self.scale
    }

    /// Plays the video `on` over layer `layer`, from its start, reading it with `open`.
    pub fn play(&mut self, layer: usize, on: VideoOn, open: LargeOpen, cx: &mut Context<Self>) {
        let player = Player::opening(layer, on, 0.0, cx.background_executor().clone());
        self.start(player, open, cx);
    }

    /// Starts `player`, opening its file with `open` on a background task. What comes of a
    /// player stopped meanwhile, or of the canvas gone, ends off the window's thread.
    fn start(&mut self, player: Player, open: LargeOpen, cx: &mut Context<Self>) {
        self.players_made += 1;
        let id = self.players_made;
        let (from, looped) = (player.resume_from(), player.on.looped);
        self.players.push((id, player));
        let background = cx.background_executor().clone();
        cx.spawn(async move |canvas, cx| {
            let opened = cx
                .background_spawn(async move { open_and_play(open, from, looped) })
                .await;
            let mut opened = Some(opened);
            canvas
                .update(cx, |canvas, cx| {
                    let Some((_, player)) = canvas.players.iter_mut().find(|(at, _)| *at == id) else { return };
                    let Some(opened) = opened.take() else { return };
                    let worked = opened.is_ok();
                    if player.opened(opened) {
                        cx.emit(VideoLetGo);
                    }
                    if worked {
                        cx.emit(VideoOpened);
                    }
                    if let Some(why) = player.failed() {
                        cx.emit(VideoFailed(why));
                    }
                    cx.notify();
                })
                .ok();
            if let Some(opened) = opened {
                discard(opened, &background);
            }
        })
        .detach();
        cx.notify();
    }

    /// Stops every video playing, which shows its still again.
    pub fn stop_videos(&mut self, cx: &mut Context<Self>) {
        if self.players.is_empty() {
            return;
        }
        for (_, mut player) in std::mem::take(&mut self.players) {
            self.retired_frames.extend(player.take_shown());
        }
        cx.notify();
    }

    /// Ends the decoding of every video, each keeping the picture it shows, as on a slide left
    /// that's still seen as the next comes in.
    pub fn hold_videos(&mut self, cx: &mut Context<Self>) {
        for (_, player) in &mut self.players {
            player.hold();
        }
        cx.notify();
    }

    /// The layers a video is playing over, or being opened to play over.
    pub fn playing_videos(&self) -> Vec<usize> {
        self.players.iter().filter(|(_, player)| player.is_playing()).map(|(_, player)| player.layer).collect()
    }

    /// Whether any video is held here, playing or not.
    pub fn has_videos(&self) -> bool {
        !self.players.is_empty()
    }

    /// Follows the videos among the layers as they're placed now: a video still over its layer
    /// plays where the layer is, and one whose layer plays something else, or nothing, stops.
    pub fn place_videos(&mut self, videos: &[VideoLayer], cx: &mut Context<Self>) {
        let mut gone = Vec::new();
        for (id, player) in &mut self.players {
            match videos.iter().find(|video| video.layer == player.layer && video.on.src == player.on.src) {
                Some(video) if video.on != player.on => {
                    player.place(video.on.clone());
                    cx.notify();
                }
                Some(_) => {}
                None => gone.push(*id),
            }
        }
        self.players.retain_mut(|(id, player)| {
            let keep = !gone.contains(id);
            if !keep {
                self.retired_frames.extend(player.take_shown());
            }
            keep
        });
        if !gone.is_empty() {
            cx.notify();
        }
    }

    /// The videos of `wanted` that [`Self::show_videos`] would open: those not playing here, or
    /// all of them `afresh`.
    pub fn videos_to_open<'a>(&self, wanted: &'a [VideoLayer], afresh: bool) -> Vec<&'a VideoLayer> {
        wanted
            .iter()
            .filter(|video| afresh || !self.players.iter().any(|(_, player)| player.layer == video.layer && player.on.src == video.on.src))
            .collect()
    }

    /// Plays `wanted` and nothing else, reading each with what `open` gives for its file: one
    /// already playing goes on, unless `afresh`, when every video starts again from its start.
    pub fn show_videos(&mut self, wanted: &[VideoLayer], afresh: bool, mut open: impl FnMut(&str) -> Option<LargeOpen>, cx: &mut Context<Self>) {
        if afresh {
            self.stop_videos(cx);
        }
        self.place_videos(wanted, cx);
        for video in self.videos_to_open(wanted, false).into_iter().cloned().collect::<Vec<_>>() {
            if let Some(opened) = open(&video.on.src) {
                self.play(video.layer, video.on, opened, cx);
            }
        }
    }

    /// Opens again, with what `open` gives for its file, each video whose file a save let go of,
    /// and plays it on from where it had got to. One `open` gives nothing for waits on.
    pub fn open_videos_again(&mut self, mut open: impl FnMut(&str) -> Option<LargeOpen>, cx: &mut Context<Self>) {
        let waiting: Vec<u64> = self.players.iter().filter(|(_, player)| player.was_let_go()).map(|(id, _)| *id).collect();
        for id in waiting {
            let Some(at) = self.players.iter().position(|(at, _)| *at == id) else { continue };
            // One that can't be opened now waits on.
            let Some(opened) = open(&self.players[at].1.on.src) else { continue };
            let (_, mut player) = self.players.remove(at);
            player.reopening();
            self.start(player, opened, cx);
        }
    }

    /// The files of the videos whose files a save let go of, waiting to be opened again.
    pub fn videos_let_go(&self) -> Vec<String> {
        self.players.iter().filter(|(_, player)| player.was_let_go()).map(|(_, player)| player.on.src.clone()).collect()
    }

    /// Why each video held here stopped, if it couldn't go on, in order.
    pub fn video_failures(&self) -> Vec<Option<slides_video::CantPlay>> {
        self.players.iter().map(|(_, player)| player.failed()).collect()
    }

    /// The picture each video held here shows now, in the order they were started.
    #[doc(hidden)]
    pub fn video_frames(&self) -> Vec<Option<Arc<RenderImage>>> {
        self.players.iter().map(|(_, player)| player.shown().map(|shown| shown.image.clone())).collect()
    }

    /// When in its video each picture shown is from, in seconds, in the same order.
    #[doc(hidden)]
    pub fn video_frame_times(&self) -> Vec<Option<f64>> {
        self.players.iter().map(|(_, player)| player.shown().map(|shown| shown.at)).collect()
    }

    /// The names of the threads decoding the videos held here.
    #[doc(hidden)]
    pub fn video_threads(&self) -> Vec<String> {
        self.players.iter().filter_map(|(_, player)| player.thread().map(str::to_string)).collect()
    }

    /// How many pictures each video held here has decoded ahead.
    #[doc(hidden)]
    pub fn videos_queued(&self) -> Vec<usize> {
        self.players.iter().map(|(_, player)| player.queued()).collect()
    }

    /// How many pictures of videos have been made to show, and how many released, in all.
    #[doc(hidden)]
    pub fn video_images(&self) -> (usize, usize) {
        (self.frames_made, self.frames_released.get())
    }

    /// How many pictures of videos have been passed over, late, for a newer one, in all.
    #[doc(hidden)]
    pub fn video_pictures_skipped(&self) -> usize {
        self.frames_skipped
    }

    /// How long taking the videos' pictures took the window's thread when it last drew.
    #[doc(hidden)]
    pub fn last_video_advance(&self) -> std::time::Duration {
        self.last_advance
    }

    /// Takes the pictures due now from the videos playing, and asks the window for another frame
    /// while any plays. A turned video's picture is turned on a background task.
    fn advance_videos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.players.is_empty() {
            // The next to play is counted from its start.
            self.counted = None;
            return;
        }
        let began = std::time::Instant::now();
        let now = cx.background_executor().now();
        let (mut playing, mut let_go) = (false, false);
        let mut turnings: Vec<(u64, Turning)> = Vec::new();
        let mut failed = Vec::new();
        for (id, player) in &mut self.players {
            let (waiting, failing) = (player.was_let_go(), player.failed().is_some());
            let advanced = player.advance(now, self.scale);
            self.frames_made += usize::from(advanced.made);
            self.frames_skipped += advanced.skipped;
            self.retired_frames.extend(advanced.replaced);
            turnings.extend(advanced.turning.map(|turning| (*id, turning)));
            playing |= player.wants_frames();
            let_go |= !waiting && player.was_let_go();
            failed.extend(player.failed().filter(|_| !failing));
        }
        for why in failed {
            cx.emit(VideoFailed(why));
        }
        for (id, turning) in turnings {
            cx.spawn(async move |canvas, cx| {
                let shown = cx.background_spawn(async move { turning.turn() }).await;
                canvas
                    .update(cx, |canvas, cx| {
                        let made = shown.is_some();
                        // One stopped meanwhile never shows it.
                        if let Some((_, player)) = canvas.players.iter_mut().find(|(at, _)| *at == id) {
                            canvas.frames_made += usize::from(made);
                            canvas.retired_frames.extend(player.turned(shown));
                        }
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
        }
        if playing {
            window.request_animation_frame();
        }
        if let_go {
            cx.emit(VideoLetGo);
        }
        if counts_wanted() {
            self.write_counts(began);
        }
        self.last_advance = began.elapsed();
    }

    /// Writes how many pictures were shown, skipped and released since last written, once a
    /// second at most, as [`COUNTS_TO_STDERR`] asks.
    fn write_counts(&mut self, now: std::time::Instant) {
        let (made, skipped, released) = (self.frames_made, self.frames_skipped, self.frames_released.get());
        let Some((then, made_then, skipped_then, released_then)) = self.counted else {
            self.counted = Some((now, made, skipped, released));
            return;
        };
        let seconds = now.duration_since(then).as_secs_f64();
        if seconds < 1.0 {
            return;
        }
        eprintln!(
            "video pictures in {seconds:.2} s: {} shown, {} skipped, {} released",
            made - made_then,
            skipped - skipped_then,
            released - released_then
        );
        self.counted = Some((now, made, skipped, released));
    }

    /// Draws the model `on`, of layer `layer`, live over its layer, reading it with `open`, from
    /// its home view, or from `aim`; turning on its own if `orbits`.
    pub fn turn_model(&mut self, layer: usize, on: ModelOn, open: LargeOpen, orbits: bool, aim: Aim, cx: &mut Context<Self>) {
        let mut turner = Turner::reading(layer, on, orbits);
        turner.start_at(aim);
        self.turners_made += 1;
        let id = self.turners_made;
        self.turners.push((id, turner));
        self.read_model(id, open, cx);
    }

    /// Reads the model of turner `id` with `open` on a background task, and puts it on the
    /// card. What comes of one stopped meanwhile, or of the canvas gone, is let go of.
    fn read_model(&mut self, id: u64, open: LargeOpen, cx: &mut Context<Self>) {
        cx.spawn(async move |canvas, cx| {
            let reading = cx.background_spawn(async move { model_view::read_model(open) }).await;
            let mut reading = Some(reading);
            canvas
                .update(cx, |canvas, cx| {
                    let Some((_, turner)) = canvas.turners.iter_mut().find(|(at, _)| *at == id) else { return };
                    let Some(reading) = reading.take() else { return };
                    if let Some(why) = turner.read(reading) {
                        cx.emit(ModelFailed(why));
                    }
                    if turner.was_let_go() {
                        cx.emit(ModelLetGo);
                    }
                    cx.notify();
                })
                .ok();
            // A model no one turns any more leaves the card here.
            drop(reading);
        })
        .detach();
        cx.notify();
    }

    /// Stops drawing every model live: each shows its still again.
    pub fn stop_models(&mut self, cx: &mut Context<Self>) {
        if self.turners.is_empty() {
            return;
        }
        for (_, mut turner) in std::mem::take(&mut self.turners) {
            self.retired_models.extend(turner.take_shown());
        }
        cx.notify();
    }

    /// Follows the models among the layers as they're placed now: one still over its layer is
    /// drawn where the layer is, and one whose layer shows something else, or nothing, stops.
    pub fn place_models(&mut self, models: &[ModelLayer], cx: &mut Context<Self>) {
        let mut gone = Vec::new();
        for (id, turner) in &mut self.turners {
            match models.iter().find(|model| model.layer == turner.layer && model.on.src == turner.on.src) {
                Some(model) if model.on != turner.on => {
                    turner.place(model.on.clone());
                    cx.notify();
                }
                Some(_) => {}
                None => gone.push(*id),
            }
        }
        self.turners.retain_mut(|(id, turner)| {
            let keep = !gone.contains(id);
            if !keep {
                self.retired_models.extend(turner.take_shown());
            }
            keep
        });
        if !gone.is_empty() {
            cx.notify();
        }
    }

    /// The layers whose models are drawn live here.
    pub fn models_turned(&self) -> Vec<usize> {
        self.turners.iter().map(|(_, turner)| turner.layer).collect()
    }

    fn turner(&mut self, layer: usize) -> Option<&mut Turner> {
        self.turners.iter_mut().find(|(_, turner)| turner.layer == layer).map(|(_, turner)| turner)
    }

    /// Turns the model of layer `layer`, if it's drawn live, as a drag of `dx` by `dy` pixels does.
    pub fn turn_by(&mut self, layer: usize, dx: f32, dy: f32, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        if let Some(turner) = self.turner(layer) {
            turner.turn_by(dx, dy, now);
            cx.notify();
        }
    }

    /// Holds the model of layer `layer`, as a drag on it begins, or lets go of it.
    pub fn hold_model(&mut self, layer: usize, held: bool, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        if let Some(turner) = self.turner(layer) {
            turner.hold(held, now);
            cx.notify();
        }
    }

    /// Brings the model of layer `layer` nearer by `steps` of the wheel.
    pub fn zoom_by(&mut self, layer: usize, steps: f32, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        if let Some(turner) = self.turner(layer) {
            turner.zoom_by(steps, now);
            cx.notify();
        }
    }

    /// Flies the model of layer `layer` to `aim`, over a second.
    pub fn fly_model(&mut self, layer: usize, aim: Aim, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        if let Some(turner) = self.turner(layer) {
            turner.fly_to(aim, now);
            cx.notify();
        }
    }

    /// Where the model of layer `layer` is seen from now, if it's drawn live and read.
    pub fn model_view(&self, layer: usize) -> Option<View> {
        self.turners.iter().find(|(_, turner)| turner.layer == layer)?.1.view()
    }

    /// Where the show has the model of layer `layer` go, if it's drawn live.
    pub fn model_aim(&self, layer: usize) -> Option<Aim> {
        Some(self.turners.iter().find(|(_, turner)| turner.layer == layer)?.1.aim())
    }

    /// The files of the models waiting to be read again, as a save let go of them or the card
    /// lost them.
    pub fn models_let_go(&self) -> Vec<String> {
        self.turners.iter().filter(|(_, turner)| turner.was_let_go()).map(|(_, turner)| turner.on.src.clone()).collect()
    }

    /// Reads again, with what `open` gives for its file, each model whose file a save let go of,
    /// or that the card lost.
    /// One `open` gives nothing for waits on.
    pub fn read_models_again(&mut self, mut open: impl FnMut(&str) -> Option<LargeOpen>, cx: &mut Context<Self>) {
        let waiting: Vec<u64> = self.turners.iter().filter(|(_, turner)| turner.was_let_go()).map(|(id, _)| *id).collect();
        for id in waiting {
            let Some((_, turner)) = self.turners.iter_mut().find(|(at, _)| *at == id) else { continue };
            let Some(opened) = open(&turner.on.src) else { continue };
            turner.reading_again();
            self.read_model(id, opened, cx);
        }
    }

    /// Why each model drawn live here can't be drawn, if it can't, in order.
    pub fn model_failures(&self) -> Vec<Option<CantDraw>> {
        self.turners.iter().map(|(_, turner)| turner.failed()).collect()
    }

    /// The live picture each model drawn here shows now, in the order they were started.
    #[doc(hidden)]
    pub fn model_pictures(&self) -> Vec<Option<Arc<RenderImage>>> {
        self.turners.iter().map(|(_, turner)| turner.shown().map(|shown| shown.image.clone())).collect()
    }

    /// Whether any model drawn here waits for a picture from the drawer.
    #[doc(hidden)]
    pub fn models_waiting(&self) -> bool {
        self.turners.iter().any(|(_, turner)| turner.waiting())
    }

    /// How many live pictures of models have been made to show, and how many released, in all.
    #[doc(hidden)]
    pub fn model_images(&self) -> (usize, usize) {
        (self.models_made, self.models_released.get())
    }

    /// Takes in the pictures of models that have come, asks for the next of each whose view has
    /// changed, and asks the window for another frame while any view moves or a picture is to
    /// come. A turned model's picture is turned on a background task.
    fn advance_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.turners.is_empty() {
            // The next to turn is counted from its start.
            self.models_counted = None;
            return;
        }
        if counts_wanted()
            && let Some(line) = model_counts(&mut self.models_counted, std::time::Instant::now(), self.models_made, self.models_released.get())
        {
            eprintln!("{line}");
        }
        let now = cx.background_executor().now();
        let mut wanted = false;
        let mut turnings = Vec::new();
        let mut let_go = false;
        for (id, turner) in &mut self.turners {
            let advanced = turner.advance(now, self.scale);
            self.models_made += usize::from(advanced.made);
            self.retired_models.extend(advanced.replaced);
            turnings.extend(advanced.turning.map(|turning| (*id, turning)));
            let_go |= advanced.read_again;
            if let Some(why) = advanced.failed {
                cx.emit(ModelFailed(why));
            }
            wanted |= turner.wants_frames();
        }
        if let_go {
            cx.emit(ModelLetGo);
        }
        for (id, turning) in turnings {
            cx.spawn(async move |canvas, cx| {
                let shown = cx.background_spawn(async move { turning.turn() }).await;
                canvas
                    .update(cx, |canvas, cx| {
                        let made = shown.is_some();
                        if let Some((_, turner)) = canvas.turners.iter_mut().find(|(at, _)| *at == id) {
                            canvas.models_made += usize::from(made);
                            canvas.retired_models.extend(turner.turned(shown));
                        }
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
        }
        if wanted {
            window.request_animation_frame();
        }
    }

    /// The image of every layer, first to last, hidden ones included.
    pub fn images(&self) -> Vec<Arc<RenderImage>> {
        self.sprites
            .iter()
            .map(|sprite| sprite.image.clone())
            .collect()
    }

    fn measured(
        &mut self,
        area: Bounds<Pixels>,
        slide: Bounds<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        self.placed = Some((slide, area));
        if self.framed {
            self.edge = Some(slide.dilate(px(1.)));
        }
        let stale = self
            .scale
            .is_none_or(|drawn_at| (scale / drawn_at - 1.0).abs() > REDRAW_CHANGE);
        if stale {
            self.scale = Some(scale);
            cx.emit(ScaleChanged);
        }
    }

    /// The layers to paint, first to last.
    fn runs(&self) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        for (index, sprite) in self.sprites.iter().enumerate() {
            let look = self.looks.get(index).copied().unwrap_or_default();
            if !look.visible {
                continue;
            }
            let over: Vec<(Shown, Rect)> = self
                .players
                .iter()
                .filter(|(_, player)| player.layer == index)
                .filter_map(|(_, player)| Some((player.shown()?.clone(), player.on.rect)))
                .collect();
            let model: Vec<(Shown, Rect)> = self
                .turners
                .iter()
                .filter(|(_, turner)| turner.layer == index)
                .filter_map(|(_, turner)| Some((turner.shown()?.clone(), turner.on.rect)))
                .collect();
            // A model drawn live shows in place of its still, which would show through around it.
            let still = model.is_empty();
            let sprite = (sprite.clone(), look.offset_y, still, over.into_iter().chain(model).collect());
            match runs.last_mut() {
                Some(run) if run.opacity == look.opacity => run.sprites.push(sprite),
                _ => runs.push(Run {
                    opacity: look.opacity,
                    sprites: vec![sprite],
                }),
            }
        }
        runs
    }
}

/// The largest 16:9 rectangle centred in `area`, on whole device pixels so that a layer
/// drawn for this width lands exactly on the pixels of the screen.
pub fn letterbox(area: Bounds<Pixels>, scale_factor: f32) -> Bounds<Pixels> {
    let (area_width, area_height) = (
        f32::from(area.size.width) * scale_factor,
        f32::from(area.size.height) * scale_factor,
    );
    let width = area_width.min(area_height * 16.0 / 9.0).floor();
    let height = (width * 9.0 / 16.0).floor();
    let left = (f32::from(area.origin.x) * scale_factor + (area_width - width) / 2.0).floor();
    let top = (f32::from(area.origin.y) * scale_factor + (area_height - height) / 2.0).floor();
    Bounds {
        origin: point(px(left / scale_factor), px(top / scale_factor)),
        size: size(px(width / scale_factor), px(height / scale_factor)),
    }
}

/// Where the slide sits in `area`, and the scale its layers belong at there: its width in
/// device pixels over 1920. `None` if there's no room for it.
fn fit(area: Bounds<Pixels>, scale_factor: f32) -> Option<(Bounds<Pixels>, f32)> {
    let slide = letterbox(area, scale_factor);
    let scale = f32::from(slide.size.width) * scale_factor / SLIDE_WIDTH;
    (scale > 0.0).then_some((slide, scale))
}

fn paint_sprites(
    sprites: Vec<RunSprite>,
    area: Bounds<Pixels>,
    drawn_at: Option<f32>,
    offset_x: f32,
    window: &mut Window,
) {
    let scale_factor = window.scale_factor();
    let Some((slide, scale)) = fit(area, scale_factor) else {
        return;
    };
    // From the device pixels the layers were drawn in to the window's own.
    let to_window = drawn_at.map_or(1.0, |drawn_at| scale / drawn_at) / scale_factor;
    let width = f32::from(slide.size.width);
    let unit = width / SLIDE_WIDTH;
    for (Sprite { layer, image }, offset_y, painted, over) in sprites {
        let place = Bounds {
            origin: slide.origin
                + point(
                    px(layer.x as f32 * to_window + offset_x * width),
                    px(layer.y as f32 * to_window + offset_y * width / SLIDE_WIDTH),
                ),
            size: size(
                px(layer.pixmap.width() as f32 * to_window),
                px(layer.pixmap.height() as f32 * to_window),
            ),
        };
        // Clipped to the slide, so that a layer on the move stays off the bars beside it.
        // An image too large for the GPU is refused. The rest of the slide still shows.
        if painted {
            window
                .paint_image(slide, place, Corners::default(), image, 0, false)
                .ok();
        }
        // A video playing over the layer, where the layer is and moving as it does.
        for (shown, rect) in over {
            let place = match shown.placed {
                Placed::Rect => Bounds {
                    origin: slide.origin
                        + point(
                            px(rect.x * unit + offset_x * width),
                            px((rect.y + offset_y) * unit),
                        ),
                    size: size(px(rect.w * unit), px(rect.h * unit)),
                },
                Placed::Drawn { x, y, width: drawn_width, height: drawn_height, scale: drawn, for_rect } => {
                    let to_window = scale / drawn / scale_factor;
                    // Moved since it was drawn, as its box was.
                    let (moved_x, moved_y) = ((rect.x - for_rect.x) * unit, (rect.y - for_rect.y) * unit);
                    Bounds {
                        origin: slide.origin
                            + point(
                                px(x as f32 * to_window + moved_x + offset_x * width),
                                px(y as f32 * to_window + moved_y + offset_y * unit),
                            ),
                        size: size(px(drawn_width as f32 * to_window), px(drawn_height as f32 * to_window)),
                    }
                }
            };
            window
                .paint_image(slide, place, Corners::default(), shown.image, 0, false)
                .ok();
        }
    }
}

impl Render for SlideCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.advance_videos(window, cx);
        self.advance_models(window, cx);
        let retired = std::mem::take(&mut self.retired);
        let frames = std::mem::take(&mut self.retired_frames);
        let released = self.frames_released.clone();
        let model_pictures = std::mem::take(&mut self.retired_models);
        let models_released = self.models_released.clone();
        let edge_color = self.framed.then(|| cx.theme().border);
        let this = cx.entity().downgrade();
        // A child of its own, as it has to happen even with no layer showing.
        let housekeeping = canvas(
            |_, _, _| {},
            move |area, _, window, cx| {
                for image in retired {
                    window.drop_image(image).ok();
                }
                for image in frames {
                    window.drop_image(image).ok();
                    released.set(released.get() + 1);
                }
                for image in model_pictures {
                    window.drop_image(image).ok();
                    models_released.set(models_released.get() + 1);
                }
                if let Some((slide, scale)) = fit(area, window.scale_factor()) {
                    if let Some(color) = edge_color {
                        window.paint_quad(outline(slide.dilate(px(1.)), color, BorderStyle::Solid));
                    }
                    this.update(cx, |this, cx| this.measured(area, slide, scale, cx))
                        .ok();
                }
            },
        )
        .absolute()
        .size_full();

        let drawn_at = self.scale;
        let SlideLook { opacity, offset_x } = self.slide_look;
        // An image takes its opacity from the element around it, and that's what the runs are for.
        let runs = self.runs().into_iter().map(|run| {
            div().absolute().size_full().opacity(run.opacity).child(
                canvas(
                    |_, _, _| {},
                    move |area, _, window, _| {
                        paint_sprites(run.sprites, area, drawn_at, offset_x, window)
                    },
                )
                .size_full(),
            )
        });
        div()
            .absolute()
            .size_full()
            .opacity(opacity)
            .child(housekeeping)
            .children(runs)
    }
}

/// What a window turning models writes of their live pictures, drawn and shown, and released,
/// `made` and `released` in all now: nothing the first time, which `counted` begins counting
/// from, and then once a second at most, counting afresh from each time it's written.
fn model_counts(counted: &mut Option<(std::time::Instant, usize, usize)>, now: std::time::Instant, made: usize, released: usize) -> Option<String> {
    let Some((then, made_then, released_then)) = *counted else {
        *counted = Some((now, made, released));
        return None;
    };
    let seconds = now.duration_since(then).as_secs_f64();
    if seconds < 1.0 {
        return None;
    }
    *counted = Some((now, made, released));
    Some(format!("model pictures in {seconds:.2} s: {} drawn and shown, {} released", made - made_then, released - released_then))
}

#[cfg(test)]
mod tests {
    use super::model_counts;
    use std::time::{Duration, Instant};

    #[test]
    fn model_counts_are_written_once_a_second_at_most() {
        let mut counted = None;
        let start = Instant::now();
        // The first time a model turns, counting begins.
        assert_eq!(model_counts(&mut counted, start, 4, 2), None);
        assert_eq!(model_counts(&mut counted, start + Duration::from_millis(500), 8, 6), None);
        assert_eq!(
            model_counts(&mut counted, start + Duration::from_millis(1100), 12, 11).as_deref(),
            Some("model pictures in 1.10 s: 8 drawn and shown, 9 released")
        );
        // Counted afresh from then.
        assert_eq!(model_counts(&mut counted, start + Duration::from_millis(1900), 20, 19), None);
        assert_eq!(
            model_counts(&mut counted, start + Duration::from_millis(2200), 20, 20).as_deref(),
            Some("model pictures in 1.10 s: 8 drawn and shown, 9 released")
        );
    }
}
