//! The list of the talk's slides down the left of the window, each with its number and a
//! small picture of it. A slide is moved by dragging its row, and each row has a menu of what
//! can be done to its slide.

use std::collections::HashMap;
use std::future::Future;
use std::ops::Range;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, VirtualListScrollHandle, h_flex, v_flex,
    v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{Pixmap, to_bgra_frame};

use crate::document::Document;
use crate::slide_editor::SlideEditor;
use crate::workspace_view::{
    CopyToClipboard, CutToClipboard, DeleteSlide, DuplicateSlide, MoveSlideDown, MoveSlideTo,
    MoveSlideUp, NewSlide, PasteFromClipboard, SelectSlide, ToggleHidden,
};

pub const STRIP_ID: &str = "slide-strip";

/// The line between rows that shows where a dragged slide will land.
pub const DROP_LINE_ID: &str = "strip-drop-line";

/// The picture of a dragged slide, under the pointer.
pub const DRAG_PICTURE_ID: &str = "strip-drag-picture";

/// The key context of the list, where its own shortcuts apply.
pub const STRIP_CONTEXT: &str = "SlideStrip";

actions!(slide_strip, [CancelDrag]);

/// The list's own shortcuts. Call once, after `gpui_kit::init`.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", CancelDrag, Some(STRIP_CONTEXT))]);
}

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
/// gap and the thumbnail's frame. The number has room for three digits, and a fourth goes
/// into the padding rather than onto a second line.
const NUMBER_WIDTH: Pixels = px(24.);
const ROW_PADDING_X: Pixels = px(8.);
const ROW_PADDING_Y: Pixels = px(6.);
const GAP: Pixels = px(6.);
const FRAME: Pixels = px(2.);

/// How thick the line that shows where a dragged slide will land is.
const DROP_LINE: Pixels = px(3.);

/// How near the top or bottom of the list a dragged slide has to be for the list to scroll,
/// how far it scrolls at most at a time, and how often.
const SCROLL_EDGE: Pixels = px(36.);
const SCROLL_STEP: Pixels = px(14.);
const SCROLL_TICK: Duration = Duration::from_millis(16);

/// Thumbnails are drawn to a multiple of this many device pixels, so that making the list a
/// little wider or narrower doesn't draw them all again.
const WIDTH_STEP: u32 = 16;

/// The id of the row of slide `index`.
pub fn strip_row_id(index: usize) -> ElementId {
    ("strip-row", index).into()
}

/// The id of the number beside slide `index`'s picture.
pub fn strip_number_id(index: usize) -> ElementId {
    ("strip-number", index).into()
}

/// What a dragged row carries: which slide it is.
struct DraggedSlide {
    index: usize,
}

/// A slide being dragged: which it is, where the pointer is, and the gap between rows it would
/// land in, counted from the top, if it's over the list.
struct Drag {
    from: usize,
    /// The slide's version when the drag began, and how many slides there were. If either
    /// changes, the row may hold another slide than the one in the picture, so the drag ends.
    version: Option<u64>,
    count: usize,
    pointer: Point<Pixels>,
    gap: Option<usize>,
}

/// The row a slide from row `from` takes when it's let go in gap `gap`, or none if that's
/// where it is already.
fn landing(from: usize, gap: usize) -> Option<usize> {
    if gap == from || gap == from + 1 {
        None
    } else if gap > from {
        Some(gap - 1)
    } else {
        Some(gap)
    }
}

/// A row's picture, and what it's a picture of: the slide's version, at a width.
struct Thumbnail {
    version: u64,
    width: u32,
    image: Arc<RenderImage>,
}

pub struct SlideStrip {
    document: Option<Entity<Document>>,
    focus_handle: FocusHandle,
    scroll: VirtualListScrollHandle,
    /// The list's width, as last measured.
    width: Pixels,
    /// Whether the list has measured itself yet.
    measured: bool,
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
    /// Where the list is in the window, as last laid out.
    bounds: Bounds<Pixels>,
    drag: Option<Drag>,
    /// Whether the list is scrolling under a dragged slide.
    scrolling: bool,
    /// Whether a drag ended by the talk changing under it is still for the window to stop.
    drag_ended: bool,
    /// The rows the list last laid out. It also lays out a row by itself to measure it, before
    /// the rows on screen, so the last of a frame is the one that's on screen.
    laid_out: Option<Range<usize>>,
    /// What types on the slide, whose typing ends as the list takes the keyboard.
    editor: Option<WeakEntity<SlideEditor>>,
    _document_changed: Option<Subscription>,
}

impl SlideStrip {
    pub fn new(document: Option<Entity<Document>>, cx: &mut Context<Self>) -> Self {
        let document_changed = document
            .as_ref()
            .map(|document| cx.observe(document, |this, _, cx| this.document_changed(cx)));
        SlideStrip {
            document,
            focus_handle: cx.focus_handle(),
            scroll: VirtualListScrollHandle::new(),
            width: STRIP_WIDTH,
            measured: false,
            scale_factor: 1.0,
            row_sizes: Rc::new(Vec::new()),
            thumbnails: HashMap::new(),
            retired: Vec::new(),
            visible: 0..0,
            drawing: false,
            drawn: 0,
            shown_current: None,
            bounds: Bounds::default(),
            drag: None,
            scrolling: false,
            drag_ended: false,
            laid_out: None,
            editor: None,
            _document_changed: document_changed,
        }
    }

    /// Takes the keyboard for the slide in hand. Whatever calls this has let go of any thing
    /// selected on a slide: the list does on any press that reaches it, and a row does itself
    /// on a right press, which its menu keeps from the list.
    fn take_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
    }

    /// Has the typing that `editor` does on the slide end as the list takes the keyboard.
    pub fn end_typing_of(&mut self, editor: WeakEntity<SlideEditor>) {
        self.editor = Some(editor);
    }

    /// Lets go of the thing selected on a slide, and ends any typing on it as Escape does, as
    /// the list takes the keyboard: from here Delete and the clipboard act on whole slides, and
    /// a thing still shown as selected, a caret still drawn or a new text box still waiting for
    /// its first letter would say otherwise. Typing ends at once, before anything the press
    /// goes on to do, so that nothing resumes it and takes the keyboard back for the slide.
    pub fn let_go_of_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typing = self.document.as_ref().is_some_and(|document| document.read(cx).text().is_some());
        if let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade).filter(|_| typing) {
            editor.update(cx, |editor, cx| editor.stop_typing(window, cx));
        }
        if let Some(document) = &self.document {
            document.update(cx, |document, cx| {
                if document.selected().is_some() {
                    document.deselect();
                    cx.notify();
                }
            });
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// The picture row `index` shows: one of its slide as the slide is now, once drawn. A
    /// picture of what the slide was isn't shown, as it may be of another slide altogether.
    pub fn thumbnail(&self, index: usize, cx: &App) -> Option<Arc<RenderImage>> {
        let version = self.document.as_ref()?.read(cx).slide_version(index)?;
        self.thumbnails
            .get(&index)
            .filter(|thumbnail| thumbnail.version == version)
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

    fn measured(&mut self, bounds: Bounds<Pixels>, scale_factor: f32, cx: &mut Context<Self>) {
        self.bounds = bounds;
        let width = bounds.size.width;
        if !self.measured || width != self.width || scale_factor != self.scale_factor {
            self.measured = true;
            self.width = width;
            self.scale_factor = scale_factor;
            cx.notify();
        }
    }

    /// Takes in which rows are on screen, lets go of the pictures of rows far from them, and
    /// has the ones on screen that don't show their slide as it is now drawn.
    fn rows_shown(&mut self, rows: Range<usize>, cx: &mut Context<Self>) {
        self.visible = rows.clone();
        let Some(document) = self.document.clone() else {
            return;
        };
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
        // Until the list has measured itself, any size drawn at would be a guess.
        if self.measured && !self.drawing && !self.missing(cx).is_empty() {
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

    /// The rows on screen whose picture isn't of their slide as it is now, at the size shown.
    /// The list's own pictures are the record of what's drawn: the deck may let go of its
    /// copies to stay within its memory, and that's no reason to draw them again.
    fn missing(&self, cx: &App) -> Vec<usize> {
        let Some(document) = &self.document else {
            return Vec::new();
        };
        let document = document.read(cx);
        let width = self.drawn_width();
        let rows = self.visible.start.min(document.slide_count())
            ..self.visible.end.min(document.slide_count());
        rows.filter(|index| {
            let version = document.slide_version(*index);
            !self.thumbnails.get(index).is_some_and(|thumbnail| {
                Some(thumbnail.version) == version && thumbnail.width == width
            })
        })
        .collect()
    }

    /// Draws a few of the pictures the rows on screen are missing, and says whether any are
    /// still missing.
    fn draw_some(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(document) = self.document.clone() else {
            self.drawing = false;
            return false;
        };
        let width = self.drawn_width();
        let missing = self.missing(cx);
        for &index in missing.iter().take(THUMBNAILS_PER_TURN) {
            let drawn = document.update(cx, |document, _| {
                let version = document.slide_version(index)?;
                Some((version, document.thumbnail(index, width)?))
            });
            if let Some((version, pixmap)) = drawn {
                self.drawn += 1;
                self.show_thumbnail(index, version, width, pixmap);
            }
        }
        cx.notify();
        let more = missing.len() > THUMBNAILS_PER_TURN;
        if !more {
            self.drawing = false;
        }
        more
    }

    fn show_thumbnail(&mut self, index: usize, version: u64, width: u32, pixmap: Arc<Pixmap>) {
        let image = Arc::new(RenderImage::new(vec![to_bgra_frame(&pixmap)]));
        self.retire(index);
        self.thumbnails.insert(
            index,
            Thumbnail {
                version,
                width,
                image,
            },
        );
    }

    fn retire(&mut self, index: usize) {
        if let Some(old) = self.thumbnails.remove(&index) {
            self.retired.push(old.image);
        }
    }

    fn slide_count(&self, cx: &App) -> usize {
        self.document
            .as_ref()
            .map_or(0, |document| document.read(cx).slide_count())
    }

    /// The gap between rows nearest to `y`, in the window, counted from the top.
    fn gap_at(&self, y: Pixels, cx: &App) -> usize {
        let within = y.clamp(self.bounds.top(), self.bounds.bottom());
        let down = within - self.bounds.top() - self.scroll.offset().y;
        let gap = (down / self.row_height()).round().max(0.) as usize;
        gap.min(self.slide_count(cx))
    }

    /// How far the list scrolls at a time with a dragged slide at `pointer`: up when it's near
    /// the top, down when it's near the bottom, and not at all otherwise.
    fn scroll_speed(&self, pointer: Point<Pixels>) -> Pixels {
        let bounds = self.bounds;
        if pointer.x < bounds.left() || pointer.x > bounds.right() {
            return Pixels::ZERO;
        }
        let edge = SCROLL_EDGE.min(bounds.size.height / 4.);
        let speed = |into: Pixels| SCROLL_STEP * (into / edge).clamp(0., 1.);
        if pointer.y < bounds.top() + edge {
            -speed(bounds.top() + edge - pointer.y)
        } else if pointer.y > bounds.bottom() - edge {
            speed(pointer.y - (bounds.bottom() - edge))
        } else {
            Pixels::ZERO
        }
    }

    fn start_drag(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        // The list takes the keyboard, so that Escape reaches it.
        window.focus(&self.focus_handle, cx);
        let (version, count) = self.document.as_ref().map_or((None, 0), |document| {
            let document = document.read(cx);
            (document.slide_version(index), document.slide_count())
        });
        self.drag = Some(Drag {
            from: index,
            version,
            count,
            pointer: window.mouse_position(),
            gap: None,
        });
        cx.notify();
    }

    /// Follows a dragged slide to `pointer`, over the list at `bounds`.
    fn dragged_to(
        &mut self,
        pointer: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.bounds = bounds;
        let gap = bounds
            .contains(&pointer)
            .then(|| self.gap_at(pointer.y, cx));
        let Some(drag) = &mut self.drag else {
            return;
        };
        drag.pointer = pointer;
        if drag.gap != gap {
            drag.gap = gap;
            cx.notify();
        }
        if !self.scrolling && self.scroll_speed(pointer) != Pixels::ZERO {
            self.scrolling = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(SCROLL_TICK).await;
                    let more = this
                        .update(cx, |this, cx| this.scroll_under_drag(cx))
                        .unwrap_or(false);
                    if !more {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    /// Scrolls the list a little under a dragged slide near its top or bottom, and says
    /// whether it's to go on.
    fn scroll_under_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let speed = match &self.drag {
            Some(drag) if cx.has_active_drag() => self.scroll_speed(drag.pointer),
            _ => Pixels::ZERO,
        };
        if speed == Pixels::ZERO {
            self.scrolling = false;
            return false;
        }
        let content = self.row_height() * self.slide_count(cx) as f32;
        let furthest = (content - self.bounds.size.height).max(Pixels::ZERO);
        let offset = self.scroll.offset();
        let y = (offset.y - speed).clamp(-furthest, Pixels::ZERO);
        if y != offset.y {
            self.scroll.set_offset(point(offset.x, y));
            if let Some(drag) = &self.drag {
                let pointer = drag.pointer;
                let gap = self
                    .bounds
                    .contains(&pointer)
                    .then(|| self.gap_at(pointer.y, cx));
                if let Some(drag) = &mut self.drag {
                    drag.gap = gap;
                }
            }
            cx.notify();
        }
        true
    }

    /// Moves a slide dragged from row `from` to the gap the pointer is over.
    fn dropped(&mut self, from: usize, window: &mut Window, cx: &mut Context<Self>) {
        // A drag the talk changed under is over, whatever the window still carries.
        if self.drag.as_ref().is_none_or(|drag| drag.from != from) {
            return;
        }
        let gap = self.gap_at(window.mouse_position().y, cx);
        self.end_drag(cx);
        if let Some(to) = landing(from, gap) {
            window.dispatch_action(Box::new(MoveSlideTo { from, to }), cx);
        }
    }

    /// Ends a drag whose slide may no longer be in its row: one whose version, or the number
    /// of slides, has changed since it began, as by a reload or an undo.
    fn document_changed(&mut self, cx: &mut Context<Self>) {
        if let (Some(drag), Some(document)) = (&self.drag, &self.document) {
            let document = document.read(cx);
            if document.slide_version(drag.from) != drag.version
                || document.slide_count() != drag.count
            {
                self.drag = None;
                self.drag_ended = true;
            }
        }
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    /// Puts a dragged slide back where it was. Escape means something else elsewhere, so
    /// without a slide being dragged it's passed on.
    fn cancel_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drag.is_some() {
            cx.stop_active_drag(window);
            self.end_drag(cx);
        } else {
            cx.propagate();
        }
    }

    /// Where the line showing where a dragged slide will land goes, down from the top of the
    /// list, if it would move.
    fn drop_line(&self) -> Option<Pixels> {
        let drag = self.drag.as_ref()?;
        let gap = drag.gap?;
        landing(drag.from, gap)?;
        let y = self.row_height() * gap as f32 + self.scroll.offset().y - DROP_LINE / 2.;
        Some(y.clamp(
            Pixels::ZERO,
            (self.bounds.size.height - DROP_LINE).max(Pixels::ZERO),
        ))
    }

    fn rows(&mut self, rows: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // What's on screen is taken in once the frame is laid out: see `laid_out`.
        self.laid_out = Some(rows.clone());
        let Some(document) = self.document.as_ref().map(|document| document.read(cx)) else {
            return Vec::new();
        };
        let current = document.current();
        let count = document.slide_count();
        let theme = cx.theme();
        let (width, height) = (self.thumbnail_width(), self.thumbnail_height());
        let row_size = size(self.width, self.row_height());
        let dragged = self.drag.as_ref().map(|drag| drag.from);
        let strip = cx.entity().downgrade();
        let focus_handle = self.focus_handle.clone();
        rows.map(|index| {
            let selected = index == current;
            let hidden = document.is_hidden(index);
            let number = index + 1;
            let label: SharedString = if hidden {
                format!("Slide {number}, hidden").into()
            } else {
                format!("Slide {number}").into()
            };
            let image = self.thumbnail(index, cx);
            let picture = DragPicture {
                image: image.clone(),
                row: row_size,
                thumbnail: size(width, height),
                origin: Point::default(),
            };
            let strip = strip.clone();
            let focus_handle = focus_handle.clone();
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
                .when(dragged == Some(index), |row| row.opacity(0.4))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.take_keyboard(window, cx);
                    window.dispatch_action(Box::new(SelectSlide { index }), cx);
                }))
                // The menu acts on the slide in hand, so a right-click takes it in hand. The
                // menu keeps the press from reaching the list, so the row lets go of the thing
                // selected itself.
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        this.let_go_of_selection(window, cx);
                        this.take_keyboard(window, cx);
                        window.dispatch_action(Box::new(SelectSlide { index }), cx);
                    }),
                )
                .on_drag(DraggedSlide { index }, move |_, held_at, window, cx| {
                    strip
                        .update(cx, |strip, cx| strip.start_drag(index, window, cx))
                        .ok();
                    cx.new(|_| picture.held_at(held_at))
                })
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
                        .child(
                            div()
                                .id(strip_number_id(index))
                                .test_support()
                                .whitespace_nowrap()
                                .child(number.to_string()),
                        )
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
                .context_menu(move |menu, _, _| {
                    menu.action_context(focus_handle.clone())
                        .menu("New slide", Box::new(NewSlide))
                        .menu("Duplicate", Box::new(DuplicateSlide))
                        .menu_with_disabled("Delete", Box::new(DeleteSlide), count < 2)
                        .menu(
                            if hidden { "Show slide" } else { "Hide slide" },
                            Box::new(ToggleHidden),
                        )
                        .menu_with_disabled("Move up", Box::new(MoveSlideUp), index == 0)
                        .menu_with_disabled(
                            "Move down",
                            Box::new(MoveSlideDown),
                            index + 1 >= count,
                        )
                        .separator()
                        .menu_with_disabled("Cut", Box::new(CutToClipboard), count < 2)
                        .menu("Copy", Box::new(CopyToClipboard))
                        .menu("Paste", Box::new(PasteFromClipboard))
                })
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
        // A drag can end without the list hearing of it, as when it's let go elsewhere.
        if self.drag.is_some() && !cx.has_active_drag() {
            self.drag = None;
        }
        if std::mem::take(&mut self.drag_ended) {
            cx.stop_active_drag(window);
        }
        let drop_line = self.drop_line();
        let this = cx.entity().downgrade();
        v_flex()
            .id(STRIP_ID)
            .test_support()
            .key_context(STRIP_CONTEXT)
            .track_focus(&self.focus_handle)
            // Any press on the list gives it the keyboard, on a row, below the rows, or to drag.
            .on_any_mouse_down(cx.listener(|this, _, window, cx| this.let_go_of_selection(window, cx)))
            .relative()
            .overflow_hidden()
            .size_full()
            .bg(cx.theme().sidebar)
            .on_action(cx.listener(|this, _: &CancelDrag, window, cx| this.cancel_drag(window, cx)))
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<DraggedSlide>, _, cx| {
                    this.dragged_to(event.event.position, event.bounds, cx)
                }),
            )
            .on_drop(cx.listener(|this, dragged: &DraggedSlide, window, cx| {
                this.dropped(dragged.index, window, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_drag(cx)),
            )
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
                        // Laid out after the list, so the list's rows on screen are known.
                        this.update(cx, |this, cx| {
                            this.measured(bounds, scale_factor, cx);
                            if let Some(rows) = this.laid_out.take() {
                                this.rows_shown(rows, cx);
                            }
                        })
                        .ok();
                    },
                )
                .absolute()
                .size_full(),
            )
            .when_some(drop_line, |strip, y| {
                strip.child(
                    div()
                        .id(DROP_LINE_ID)
                        .test_support()
                        .absolute()
                        .left(ROW_PADDING_X)
                        .right(ROW_PADDING_X)
                        .top(y)
                        .h(DROP_LINE)
                        .rounded_full()
                        .bg(cx.theme().list_active_border),
                )
            })
    }
}

/// How big a dragged slide's picture is beside its row's, so that the line where it will land
/// shows past it.
const DRAG_PICTURE_SCALE: f32 = 0.7;

/// The picture of a dragged slide, a little smaller than its row's and see-through, held by
/// the same spot the pointer took hold of.
#[derive(Clone)]
struct DragPicture {
    image: Option<Arc<RenderImage>>,
    row: Size<Pixels>,
    thumbnail: Size<Pixels>,
    /// Where the picture goes in the row's bounds, which the window draws under the pointer.
    origin: Point<Pixels>,
}

impl DragPicture {
    /// The picture for a row taken hold of at `held_at`, from the row's top left.
    fn held_at(&self, held_at: Point<Pixels>) -> Self {
        let left = ROW_PADDING_X + NUMBER_WIDTH + GAP + FRAME;
        let top = ROW_PADDING_Y + FRAME;
        // The spot held, in the row's picture, kept under the pointer as the picture shrinks.
        let x = (held_at.x - left).clamp(Pixels::ZERO, self.thumbnail.width);
        let y = (held_at.y - top).clamp(Pixels::ZERO, self.thumbnail.height);
        DragPicture {
            origin: point(
                held_at.x - x * DRAG_PICTURE_SCALE,
                held_at.y - y * DRAG_PICTURE_SCALE,
            ),
            ..self.clone()
        }
    }
}

impl Render for DragPicture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div().relative().w(self.row.width).h(self.row.height).child(
            div()
                .id(DRAG_PICTURE_ID)
                .test_support()
                .absolute()
                .left(self.origin.x)
                .top(self.origin.y)
                .w(self.thumbnail.width * DRAG_PICTURE_SCALE)
                .h(self.thumbnail.height * DRAG_PICTURE_SCALE)
                .rounded(px(3.))
                .overflow_hidden()
                .border_1()
                .border_color(theme.list_active_border)
                .bg(theme.muted)
                .shadow_lg()
                .opacity(0.8)
                .when_some(self.image.clone(), |frame, image| {
                    frame.child(img(image).size_full())
                }),
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
