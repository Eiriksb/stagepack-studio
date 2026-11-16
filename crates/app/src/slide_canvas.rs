//! Shows a slide's layers, each its own image in the window's sprite atlas.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use slides_engine::{Layer, to_bgra_frame};

use crate::present::{LayerLook, SlideLook};

/// The width of a 16:9 slide, in slide units.
pub const SLIDE_WIDTH: f32 = 1920.0;

/// How far the slide can grow or shrink before its layers are drawn again. Below this they
/// are stretched to fit, so a resize doesn't draw the slide again for every pixel.
const REDRAW_CHANGE: f32 = 0.01;

/// Sent when the slide has changed size enough that its layers should be drawn again, at
/// [`SlideCanvas::scale`].
pub struct ScaleChanged;

#[derive(Clone)]
struct Sprite {
    layer: Arc<Layer>,
    image: Arc<RenderImage>,
}

/// Showing layers next to each other that share an opacity. Each sprite comes with how far
/// below its place it is, in slide units.
struct Run {
    opacity: f32,
    sprites: Vec<(Sprite, f32)>,
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
}

impl EventEmitter<ScaleChanged> for SlideCanvas {}

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
            let sprite = (sprite.clone(), look.offset_y);
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
    sprites: Vec<(Sprite, f32)>,
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
    for (Sprite { layer, image }, offset_y) in sprites {
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
    }
}

impl Render for SlideCanvas {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let retired = std::mem::take(&mut self.retired);
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
