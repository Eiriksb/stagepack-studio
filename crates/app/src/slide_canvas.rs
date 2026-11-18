//! Shows a slide's layers, each its own image in the window's sprite atlas.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use slides_engine::{Layer, VideoLayer, VideoOn, to_bgra_frame};

use crate::document::LargeOpen;
use crate::present::{LayerLook, SlideLook};
use crate::video_player::{Placed, Player, Shown, open_and_play};

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

#[derive(Clone)]
struct Sprite {
    layer: Arc<Layer>,
    image: Arc<RenderImage>,
}

/// Showing layers next to each other that share an opacity. Each sprite comes with how far
/// below its place it is, in slide units, and the pictures of the videos playing over it.
struct Run {
    opacity: f32,
    sprites: Vec<(Sprite, f32, Vec<Shown>)>,
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
    /// How many pictures of videos have been made to show, and released, in all.
    frames_made: usize,
    frames_released: usize,
}

impl EventEmitter<ScaleChanged> for SlideCanvas {}
impl EventEmitter<VideoLetGo> for SlideCanvas {}

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
        self.start(Player::opening(layer, on, 0.0), open, cx);
    }

    /// Starts `player`, opening its file with `open` on a background task.
    fn start(&mut self, player: Player, open: LargeOpen, cx: &mut Context<Self>) {
        self.players_made += 1;
        let id = self.players_made;
        let (from, looped) = (player.resume_from(), player.on.looped);
        self.players.push((id, player));
        cx.spawn(async move |canvas, cx| {
            let opened = cx
                .background_spawn(async move { open_and_play(open, from, looped) })
                .await;
            canvas
                .update(cx, |canvas, cx| {
                    // One stopped meanwhile ends its thread as what came is dropped.
                    if let Some((_, player)) = canvas.players.iter_mut().find(|(at, _)| *at == id) {
                        player.opened(opened);
                        cx.notify();
                    }
                })
                .ok();
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

    /// Stops the video playing over layer `layer`, if one is.
    pub fn stop_video(&mut self, layer: usize, cx: &mut Context<Self>) {
        let (gone, kept) = std::mem::take(&mut self.players).into_iter().partition(|(_, player)| player.layer == layer);
        self.players = kept;
        let gone: Vec<(u64, Player)> = gone;
        if gone.is_empty() {
            return;
        }
        for (_, mut player) in gone {
            self.retired_frames.extend(player.take_shown());
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

    /// Plays `wanted` and nothing else, reading each with what `open` gives for its file: one
    /// already playing goes on, unless `afresh`, when every video starts again from its start.
    pub fn show_videos(&mut self, wanted: &[VideoLayer], afresh: bool, mut open: impl FnMut(&str) -> Option<LargeOpen>, cx: &mut Context<Self>) {
        if afresh {
            self.stop_videos(cx);
        }
        self.place_videos(wanted, cx);
        for video in wanted {
            let playing = self.players.iter().any(|(_, player)| player.layer == video.layer && player.on.src == video.on.src);
            if !playing && let Some(opened) = open(&video.on.src) {
                self.play(video.layer, video.on.clone(), opened, cx);
            }
        }
    }

    /// Opens again, with what `open` gives for its file, each video whose file a save let go of,
    /// and plays it on from where it had got to.
    pub fn open_videos_again(&mut self, mut open: impl FnMut(&str) -> Option<LargeOpen>, cx: &mut Context<Self>) {
        let waiting: Vec<u64> = self.players.iter().filter(|(_, player)| player.was_let_go()).map(|(id, _)| *id).collect();
        for id in waiting {
            let Some(at) = self.players.iter().position(|(at, _)| *at == id) else { continue };
            let (_, mut player) = self.players.remove(at);
            match open(&player.on.src) {
                Some(opened) => {
                    player.reopening();
                    self.start(player, opened, cx);
                }
                None => self.retired_frames.extend(player.take_shown()),
            }
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
        (self.frames_made, self.frames_released)
    }

    /// Takes the pictures due now from the videos playing, and asks the window for another frame
    /// while any plays.
    fn advance_videos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.players.is_empty() {
            return;
        }
        let now = cx.background_executor().now();
        let (mut playing, mut let_go) = (false, false);
        for (_, player) in &mut self.players {
            let waiting = player.was_let_go();
            let advanced = player.advance(now, self.scale);
            self.frames_made += usize::from(advanced.made);
            self.retired_frames.extend(advanced.replaced);
            playing |= player.wants_frames();
            let_go |= !waiting && player.was_let_go();
        }
        if playing {
            window.request_animation_frame();
        }
        if let_go {
            cx.emit(VideoLetGo);
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
            let over = self
                .players
                .iter()
                .filter(|(_, player)| player.layer == index)
                .filter_map(|(_, player)| player.shown().cloned())
                .collect();
            let sprite = (sprite.clone(), look.offset_y, over);
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
    sprites: Vec<(Sprite, f32, Vec<Shown>)>,
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
    for (Sprite { layer, image }, offset_y, over) in sprites {
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
        window
            .paint_image(slide, place, Corners::default(), image, 0, false)
            .ok();
        // A video playing over the layer, where the layer is and moving as it does.
        for shown in over {
            let place = match shown.placed {
                Placed::Rect(rect) => Bounds {
                    origin: slide.origin
                        + point(
                            px(rect.x * unit + offset_x * width),
                            px((rect.y + offset_y) * unit),
                        ),
                    size: size(px(rect.w * unit), px(rect.h * unit)),
                },
                Placed::Drawn { x, y, width: drawn_width, height: drawn_height, scale: drawn } => {
                    let to_window = scale / drawn / scale_factor;
                    Bounds {
                        origin: slide.origin
                            + point(
                                px(x as f32 * to_window + offset_x * width),
                                px(y as f32 * to_window + offset_y * unit),
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
        let mut retired = std::mem::take(&mut self.retired);
        let frames = std::mem::take(&mut self.retired_frames);
        self.frames_released += frames.len();
        retired.extend(frames);
        let edge_color = self.framed.then(|| cx.theme().border);
        let this = cx.entity().downgrade();
        // A child of its own, as it has to happen even with no layer showing.
        let housekeeping = canvas(
            |_, _, _| {},
            move |area, _, window, cx| {
                for image in retired {
                    window.drop_image(image).ok();
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
