//! The list of the talk's slides down the left of the window, each with its number and a
//! small picture of it.

use std::collections::HashMap;
use std::future::Future;
use std::ops::Range;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, VirtualListScrollHandle, h_flex, v_flex,
    v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{Pixmap, to_bgra_frame};

use crate::document::Document;
use crate::workspace_view::SelectSlide;

pub const STRIP_ID: &str = "slide-strip";

/// The key context of the list, where its own shortcuts apply.
pub const STRIP_CONTEXT: &str = "SlideStrip";

/// How wide the list starts, and how narrow and wide it can be made.
pub const STRIP_WIDTH: Pixels = px(220.);
pub const STRIP_MIN_WIDTH: Pixels = px(140.);
pub const STRIP_MAX_WIDTH: Pixels = px(400.);

/// How many thumbnails are drawn at a turn, before the window has its say again.
const THUMBNAILS_PER_TURN: usize = 2;

/// How far from the rows on screen a thumbnail is kept, in rows, so that scrolling back a
/// little shows it at once. Past that it's let go.
const KEPT_AROUND: usize = 24;

/// What's beside the thumbnail in a row: the padding either side, the slide's number, the
/// gap and the thumbnail's frame.
const NUMBER_WIDTH: Pixels = px(18.);
const ROW_PADDING_X: Pixels = px(8.);
const ROW_PADDING_Y: Pixels = px(6.);
const GAP: Pixels = px(6.);
const FRAME: Pixels = px(2.);

/// Thumbnails are drawn to a multiple of this many device pixels, so that making the list a
/// little wider or narrower doesn't draw them all again.
const WIDTH_STEP: u32 = 16;

/// The id of the row of slide `index`.
pub fn strip_row_id(index: usize) -> ElementId {
    ("strip-row", index).into()
}

struct Thumbnail {
    pixmap: Arc<Pixmap>,
    image: Arc<RenderImage>,
}

pub struct SlideStrip {
    document: Option<Entity<Document>>,
    focus_handle: FocusHandle,
    scroll: VirtualListScrollHandle,
    /// The list's width, as last measured.
    width: Pixels,
    scale_factor: f32,
    /// The rows' sizes, kept while the number of slides and their height stay as they are.
    row_sizes: Rc<Vec<Size<Pixels>>>,
    thumbnails: HashMap<usize, Thumbnail>,
    /// Images no row shows any more, for the next render to let go of.
    retired: Vec<Arc<RenderImage>>,
    /// The rows last put on screen.
    visible: Range<usize>,
    /// Whether thumbnails are being drawn.
    drawing: bool,
    /// How many thumbnails have been drawn, for tests.
    drawn: usize,
    /// The slide that was current at the last render, to bring a newly current one into view.
    shown_current: Option<usize>,
    _document_changed: Option<Subscription>,
}

impl SlideStrip {
    pub fn new(document: Option<Entity<Document>>, cx: &mut Context<Self>) -> Self {
        let document_changed = document
            .as_ref()
            .map(|document| cx.observe(document, |_, _, cx| cx.notify()));
        SlideStrip {
            document,
            focus_handle: cx.focus_handle(),
            scroll: VirtualListScrollHandle::new(),
            width: STRIP_WIDTH,
            scale_factor: 1.0,
            row_sizes: Rc::new(Vec::new()),
            thumbnails: HashMap::new(),
            retired: Vec::new(),
            visible: 0..0,
            drawing: false,
            drawn: 0,
            shown_current: None,
            _document_changed: document_changed,
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// The image row `index` shows, once its thumbnail is drawn.
    pub fn thumbnail(&self, index: usize) -> Option<Arc<RenderImage>> {
        self.thumbnails
            .get(&index)
            .map(|thumbnail| thumbnail.image.clone())
    }

    /// How many thumbnails the list has had drawn.
    pub fn thumbnails_drawn(&self) -> usize {
        self.drawn
    }

    /// How wide a thumbnail shows, in the window's pixels.
    fn thumbnail_width(&self) -> Pixels {
        (self.width - ROW_PADDING_X * 2. - NUMBER_WIDTH - GAP - FRAME * 2.).max(px(16.))
    }

    fn thumbnail_height(&self) -> Pixels {
        self.thumbnail_width() * (9. / 16.)
    }

    fn row_height(&self) -> Pixels {
        (self.thumbnail_height() + FRAME * 2. + ROW_PADDING_Y * 2.).round()
    }

    /// How wide a thumbnail is drawn, in device pixels.
    fn drawn_width(&self) -> u32 {
        let device = (f32::from(self.thumbnail_width()) * self.scale_factor).ceil() as u32;
        device.div_ceil(WIDTH_STEP) * WIDTH_STEP
    }

    fn measured(&mut self, width: Pixels, scale_factor: f32, cx: &mut Context<Self>) {
        if width != self.width || scale_factor != self.scale_factor {
            self.width = width;
            self.scale_factor = scale_factor;
            cx.notify();
        }
    }

    /// Takes in what the rows on screen show, and has what's missing drawn.
    fn rows_shown(&mut self, rows: Range<usize>, cx: &mut Context<Self>) {
        self.visible = rows.clone();
        let Some(document) = self.document.clone() else {
            return;
        };
        let width = self.drawn_width();
        let count = document.read(cx).slide_count();
        let keep = rows.start.saturating_sub(KEPT_AROUND)..(rows.end + KEPT_AROUND).min(count);
        let gone: Vec<usize> = self
            .thumbnails
            .keys()
            .copied()
            .filter(|index| !keep.contains(index))
            .collect();
        let retiring = !gone.is_empty();
        for index in gone {
            self.retire(index);
        }
        if retiring {
            // The next render lets go of their images.
            cx.notify();
        }
        let mut missing = false;
        for index in rows {
            match document.read(cx).drawn_thumbnail(index, width) {
                Some(pixmap) => {
                    let current = self
                        .thumbnails
                        .get(&index)
                        .is_some_and(|thumbnail| Arc::ptr_eq(&thumbnail.pixmap, &pixmap));
                    if !current {
                        // Drawn already, for another row of the list or before it scrolled away.
                        self.show_thumbnail(index, pixmap);
                    }
                }
                None => missing = true,
            }
        }
        if missing && !self.drawing {
            self.drawing = true;
            cx.spawn(async move |this, cx| {
                loop {
                    YieldNow(false).await;
                    let more = this
                        .update(cx, |this, cx| this.draw_some(cx))
                        .unwrap_or(false);
                    if !more {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    /// Draws a few of the thumbnails the rows on screen are missing, and says whether any are
    /// still missing.
    fn draw_some(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(document) = self.document.clone() else {
            self.drawing = false;
            return false;
        };
        let width = self.drawn_width();
        let count = document.read(cx).slide_count();
        let rows = self.visible.start.min(count)..self.visible.end.min(count);
        let missing: Vec<usize> = rows
            .filter(|&index| document.read(cx).drawn_thumbnail(index, width).is_none())
            .collect();
        for &index in missing.iter().take(THUMBNAILS_PER_TURN) {
            let drawn = document.update(cx, |document, _| document.thumbnail(index, width));
            if let Some(pixmap) = drawn {
                self.drawn += 1;
                self.show_thumbnail(index, pixmap);
            }
        }
        cx.notify();
        let more = missing.len() > THUMBNAILS_PER_TURN;
        if !more {
            self.drawing = false;
        }
        more
    }

    fn show_thumbnail(&mut self, index: usize, pixmap: Arc<Pixmap>) {
        let image = Arc::new(RenderImage::new(vec![to_bgra_frame(&pixmap)]));
        self.retire(index);
        self.thumbnails.insert(index, Thumbnail { pixmap, image });
    }

    fn retire(&mut self, index: usize) {
        if let Some(old) = self.thumbnails.remove(&index) {
            self.retired.push(old.image);
        }
    }

    fn rows(&mut self, rows: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.rows_shown(rows.clone(), cx);
        let Some(document) = self.document.as_ref().map(|document| document.read(cx)) else {
            return Vec::new();
        };
        let current = document.current();
        let theme = cx.theme();
        let (width, height) = (self.thumbnail_width(), self.thumbnail_height());
        rows.map(|index| {
            let selected = index == current;
            let hidden = document.is_hidden(index);
            let number = index + 1;
            let label: SharedString = if hidden {
                format!("Slide {number}, hidden").into()
            } else {
                format!("Slide {number}").into()
            };
            let image = self
                .thumbnails
                .get(&index)
                .map(|thumbnail| thumbnail.image.clone());
            h_flex()
                .id(strip_row_id(index))
                .test_support()
                .aria_label(label)
                .aria_selected(selected)
                .w_full()
                .h(self.row_height())
                .px(ROW_PADDING_X)
                .py(ROW_PADDING_Y)
                .gap(GAP)
                .items_start()
                .cursor_pointer()
                .when(selected, |row| row.bg(theme.list_active))
                .when(!selected, |row| row.hover(|row| row.bg(theme.list_hover)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.focus_handle, cx);
                    window.dispatch_action(Box::new(SelectSlide { index }), cx);
                }))
                .child(
                    v_flex()
                        .w(NUMBER_WIDTH)
                        .flex_none()
                        .items_end()
                        .gap_1()
                        .text_xs()
                        .text_color(if selected {
                            theme.foreground
                        } else {
                            theme.muted_foreground
                        })
                        .child(number.to_string())
                        .when(hidden, |column| {
                            column.child(
                                Icon::new(IconName::EyeOff)
                                    .xsmall()
                                    .text_color(theme.muted_foreground),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .w(width + FRAME * 2.)
                        .h(height + FRAME * 2.)
                        .p(FRAME - px(1.))
                        .rounded(px(5.))
                        .border(FRAME - px(1.))
                        .border_color(if selected {
                            theme.list_active_border
                        } else {
                            theme.transparent
                        })
                        .child(
                            div()
                                .size_full()
                                .rounded(px(3.))
                                .overflow_hidden()
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.muted)
                                .when(hidden, |frame| frame.opacity(0.45))
                                .when_some(image, |frame, image| {
                                    frame.child(img(image).size_full())
                                }),
                        ),
                )
                .into_any_element()
        })
        .collect()
    }
}

impl Render for SlideStrip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.retired.drain(..) {
            window.drop_image(image).ok();
        }
        let count = self
            .document
            .as_ref()
            .map_or(0, |document| document.read(cx).slide_count());
        let current = self
            .document
            .as_ref()
            .map(|document| document.read(cx).current());
        if current != self.shown_current {
            self.shown_current = current;
            if let Some(current) = current {
                self.scroll.scroll_to_item(current, ScrollStrategy::Nearest);
            }
        }
        // A slide that's gone has nothing to show.
        let gone: Vec<usize> = self
            .thumbnails
            .keys()
            .copied()
            .filter(|index| *index >= count)
            .collect();
        for index in gone {
            self.retire(index);
        }
        let row = size(self.width, self.row_height());
        if self.row_sizes.len() != count || self.row_sizes.first().is_some_and(|size| *size != row)
        {
            self.row_sizes = Rc::new(vec![row; count]);
        }
        let this = cx.entity().downgrade();
        v_flex()
            .id(STRIP_ID)
            .test_support()
            .key_context(STRIP_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                v_virtual_list(
                    cx.entity(),
                    "strip-rows",
                    self.row_sizes.clone(),
                    |this, rows, _, cx| this.rows(rows, cx),
                )
                .track_scroll(&self.scroll)
                .size_full(),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let scale_factor = window.scale_factor();
                        this.update(cx, |this, cx| {
                            this.measured(bounds.size.width, scale_factor, cx)
                        })
                        .ok();
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

/// Lets the window have its turn before the task goes on.
struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
