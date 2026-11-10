//! Shows a slide's layers, each its own image in the window's sprite atlas.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::*;
use slides_engine::{Layer, to_bgra_frame};

/// The width of a 16:9 slide, in slide units.
const SLIDE_WIDTH: f32 = 1920.0;

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

#[derive(Default)]
pub struct SlideCanvas {
    sprites: Vec<Sprite>,
    /// Images no layer uses any more. Only a paint can release them, as it has the window.
    retired: Vec<Arc<RenderImage>>,
    scale: Option<f32>,
}

impl EventEmitter<ScaleChanged> for SlideCanvas {}

impl SlideCanvas {
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

    /// The scale to draw layers at: the slide's width in device pixels over 1920. `None`
    /// until the first paint has measured the slide.
    pub fn scale(&self) -> Option<f32> {
        self.scale
    }

    /// The image of each layer showing, first to last.
    pub fn images(&self) -> Vec<Arc<RenderImage>> {
        self.sprites
            .iter()
            .map(|sprite| sprite.image.clone())
            .collect()
    }

    fn measured(&mut self, scale: f32, cx: &mut Context<Self>) {
        let stale = self
            .scale
            .is_none_or(|drawn_at| (scale / drawn_at - 1.0).abs() > REDRAW_CHANGE);
        if stale {
            self.scale = Some(scale);
            cx.emit(ScaleChanged);
        }
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

impl Render for SlideCanvas {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sprites = self.sprites.clone();
        let retired = std::mem::take(&mut self.retired);
        let drawn_at = self.scale;
        let this = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |area, _, window, cx| {
                for image in retired {
                    window.drop_image(image).ok();
                }
                let scale_factor = window.scale_factor();
                let slide = letterbox(area, scale_factor);
                let scale = f32::from(slide.size.width) * scale_factor / SLIDE_WIDTH;
                if scale <= 0.0 {
                    return;
                }
                // From the device pixels the layers were drawn in to the window's own.
                let to_window = drawn_at.map_or(1.0, |drawn_at| scale / drawn_at) / scale_factor;
                for Sprite { layer, image } in sprites {
                    let place = Bounds {
                        origin: slide.origin
                            + point(
                                px(layer.x as f32 * to_window),
                                px(layer.y as f32 * to_window),
                            ),
                        size: size(
                            px(layer.pixmap.width() as f32 * to_window),
                            px(layer.pixmap.height() as f32 * to_window),
                        ),
                    };
                    // An image too large for the GPU is refused. The rest of the slide still shows.
                    window
                        .paint_image(slide, place, Corners::default(), image, 0, false)
                        .ok();
                }
                this.update(cx, |this, cx| this.measured(scale, cx)).ok();
            },
        )
        .absolute()
        .size_full()
    }
}
