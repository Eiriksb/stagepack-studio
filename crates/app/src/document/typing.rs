//! Typing on a slide: where it starts, what it writes, and the colour and alignment given
//! to the text.

use super::*;

impl Document {
    /// Whether the thing selected takes typing: a shape, a box, or one of the talk's own
    /// components that takes content, with nothing in it, or with only text in it.
    pub fn selected_takes_text(&self) -> bool {
        let content = self.content_tags();
        let Some(selected) = &self.selected else {
            return false;
        };
        let Some(tag) = self.selected_tag() else {
            return false;
        };
        takes_text(tag, &content)
            && (tag.children.is_empty()
                || guarded(|| flows_in(self.talk(), selected.slide, &content))
                    .is_ok_and(|flows| flows.iter().any(|flow| flow.place == FlowPlace::Tag(tag.range.clone()))))
    }

    /// Starts typing in the thing selected, if it takes typing: at the end of its text, or, with
    /// none, in it empty, where nothing is written until something is typed. Says whether it did.
    pub fn begin_text_in_selected(&mut self) -> bool {
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        if !self.selected_takes_text() {
            return false;
        }
        let Some(Selected { slide, source }) = self.selected.clone() else {
            return false;
        };
        self.begin_text_in_tag(slide, source.start)
    }

    /// Starts typing in the shape, box or component taking content whose tag starts at byte
    /// `start` of slide `slide`: at the end of its text, or, with none, in it empty. Says whether
    /// it did.
    fn begin_text_in_tag(&mut self, slide: usize, start: usize) -> bool {
        let content = self.content_tags();
        if slide >= self.slide_count() {
            return false;
        }
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
            return false;
        };
        let key = FlowKey::Tag { start };
        if let Some(index) = key.find(&flows) {
            let end = TextEdit::new(flows[index].paras.clone()).select_all().selection.end();
            return self.begin_text(slide, index, end);
        }
        if !tag_starting_at(self.talk(), slide, start).is_some_and(|tag| waits_for_text(tag, &content)) {
            return false;
        }
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        self.go_to(slide);
        self.editing = Some(EditingText {
            session: self.new_session(),
            ..EditingText::in_empty_tag(slide, start)
        });
        self.keep_selection_with_typing();
        true
    }

    /// The number for a new time of typing.
    pub(super) fn new_session(&mut self) -> u64 {
        self.next_session += 1;
        self.next_session
    }

    /// Starts typing in flow `flow` of slide `slide`, an index into its `text_flows`, with the
    /// caret at `pos`, and makes `slide` the current slide. Text being typed elsewhere ends
    /// first, and `flow` is taken as it was before that. In the flow already being typed in,
    /// it only moves the caret. Says whether the slide has such a flow.
    pub fn begin_text(&mut self, slide: usize, flow: usize, pos: Pos) -> bool {
        let content = self.content_tags();
        if slide >= self.slide_count() {
            return false;
        }
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
            return false;
        };
        let Some(key) = FlowKey::of(&flows, flow) else {
            return false;
        };
        if self
            .editing
            .as_ref()
            .is_some_and(|editing| editing.slide == slide && editing.flow == key)
        {
            self.apply_text(|edit| edit.move_to(pos, false)).ok();
            return true;
        }
        let ended = match self.finish_text() {
            Ok((_, removed)) => removed,
            Err(refused) => {
                self.say(refused);
                None
            }
        };
        let flow = match ended {
            Some((on, removed)) if on == slide && removed == flow => return false,
            Some((on, removed)) if on == slide && removed < flow => flow - 1,
            _ => flow,
        };
        self.go_to(slide);
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
            return false;
        };
        let Some(key) = FlowKey::of(&flows, flow) else {
            return false;
        };
        let edit = TextEdit::new(flows[flow].paras.clone()).move_to(pos, false);
        self.editing = Some(EditingText {
            slide,
            flow: key,
            edit,
            run: None,
            session: self.new_session(),
            composing: None,
        });
        self.keep_selection_with_typing();
        true
    }

    /// Starts typing in region `region` of slide `slide`, which has nothing in it, and makes
    /// `slide` the current slide. Nothing is written until something is typed. A region with
    /// text in it is typed into at the end of its first run of text. Says whether the slide's
    /// layout has such a region to type in.
    pub fn begin_text_in_region(&mut self, slide: usize, region: &str) -> bool {
        let content = self.content_tags();
        if slide >= self.slide_count() {
            return false;
        }
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        self.go_to(slide);
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
            return false;
        };
        let place = slides_format::FlowPlace::Region(region.to_string());
        if let Some(index) = flows.iter().position(|flow| flow.place == place) {
            let end = TextEdit::new(flows[index].paras.clone()).select_all().selection.end();
            return self.begin_text(slide, index, end);
        }
        // A region of the layout with no text in it offers to add some.
        if self.text_layout(slide).placeholder(region).is_none() {
            return false;
        }
        self.editing = Some(EditingText {
            session: self.new_session(),
            ..EditingText::in_empty_region(slide, region)
        });
        self.keep_selection_with_typing();
        true
    }

    /// Makes `op` to the text being typed, writes what it gives to the talk, and returns the
    /// slides to show again. A run of typing in one flow is one step to undo, until the caret
    /// is moved, the text is styled, or [`Self::end_typing_run`] or anything else happens.
    ///
    /// What's typed always reads as the talk holds it: the flow its key finds holds exactly
    /// its paragraphs, or, before anything is written in an empty region, the region has no
    /// text and it holds one empty paragraph. If that doesn't hold before `op`, typing ends and
    /// it's refused, with nothing written. What's written must read back as the paragraphs `op`
    /// gave, with the slide's other text as it was. If it wouldn't, or it can't be written
    /// there, it's refused, and the talk and the text being typed stay as they were.
    pub fn apply_text(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
    ) -> Result<Vec<usize>, EditRefused> {
        self.write_text(op, None)
    }

    /// Makes `op`, a paste, to the text being typed, as [`Self::apply_text`] does, but as a step
    /// of its own to undo, named "Paste": the run of typing before it ends, and typing after it
    /// starts another.
    pub fn paste_text(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
    ) -> Result<Vec<usize>, EditRefused> {
        self.text_step(PASTE, op)
    }

    /// Deletes the selection of the text being typed, once it's been copied, as a step of its
    /// own to undo, named "Cut", as [`Self::paste_text`] is.
    pub fn cut_text(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.text_step(CUT, TextEdit::delete)
    }

    fn text_step(
        &mut self,
        label: &'static str,
        op: impl FnOnce(&TextEdit) -> TextEdit,
    ) -> Result<Vec<usize>, EditRefused> {
        self.end_typing_run();
        let made = self.write_text(op, Some(label));
        self.end_typing_run();
        made
    }

    /// [`Self::apply_text`], the step named `label` if it's given. A colour held for the
    /// selection is given to it first.
    fn write_text(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
        label: Option<&'static str>,
    ) -> Result<Vec<usize>, EditRefused> {
        let given = self.give_held_first();
        Ok(joined(given, self.write_now(op, label)?))
    }

    /// Keeps `color` to give the selection of the text being typed: the selection as it is,
    /// in the text as it is. It's given when [`Self::give_held_color`] is asked, or before
    /// anything else is done to the text or typing ends; by then the selection may have moved,
    /// and it's that selection that's coloured all the same. If the text itself has changed,
    /// it's dropped instead.
    pub fn hold_color(&mut self, color: String) {
        self.held_color = self.editing.as_ref().map(|editing| HeldColor {
            color,
            slide: editing.slide,
            flow: editing.flow.clone(),
            selection: editing.edit.selection,
            paras: editing.edit.paras.clone(),
        });
    }

    /// Gives the colour held for a selection to it, as one step, leaving the selection as it
    /// is now, and returns the slides to show again.
    pub fn give_held_color(&mut self) -> Result<Vec<usize>, EditRefused> {
        let Some(held) = self.held_color.take() else {
            return Ok(Vec::new());
        };
        let Some(editing) = &self.editing else {
            return Ok(Vec::new());
        };
        if editing.slide != held.slide || editing.flow != held.flow || editing.edit.paras != held.paras {
            // The text it was chosen for isn't there as it was.
            return Ok(Vec::new());
        }
        let (then, now) = (held.selection, editing.edit.selection);
        if then.is_caret() && then != now {
            // A colour for what was to be typed there, which wasn't.
            return Ok(Vec::new());
        }
        let color = held.color;
        self.write_now(
            |edit| {
                let colored = edit
                    .move_to(then.anchor, false)
                    .move_to(then.head, true)
                    .set_color(Some(color));
                if then == now {
                    colored
                } else {
                    colored.move_to(now.anchor, false).move_to(now.head, true)
                }
            },
            None,
        )
    }

    /// [`Self::give_held_color`], before something else is done: what it refuses is said.
    pub(super) fn give_held_first(&mut self) -> Vec<usize> {
        self.give_held_color().unwrap_or_else(|refused| {
            self.say(refused);
            Vec::new()
        })
    }

    /// [`Self::write_text`], with no colour held given first.
    fn write_now(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
        label: Option<&'static str>,
    ) -> Result<Vec<usize>, EditRefused> {
        let content = self.content_tags();
        // Anything done to the text drops what the system was composing: a commit types it.
        self.compose(None);
        let Some(editing) = self.editing.clone() else {
            return Ok(Vec::new());
        };
        let slide = editing.slide;
        let flows = (slide < self.slide_count())
            .then(|| guarded(|| flows_in(self.talk(), slide, &content)).ok())
            .flatten();
        let Some(flows) = flows.filter(|flows| reads_as_typed(self.talk(), &content, flows, &editing)) else {
            // The talk and what's typed have parted: nothing can be written safely.
            self.editing = None;
            self.held_color = None;
            return Err(EditRefused::CouldNotBeTyped);
        };
        let after = guarded(|| op(&editing.edit)).map_err(|_| EditRefused::CouldNotBeTyped)?;
        let kind = Kind::of(&editing.edit, &after);
        match kind {
            Kind::Nothing => return Ok(Vec::new()),
            Kind::Moved => {
                self.keep_typed(after, None);
                return Ok(Vec::new());
            }
            Kind::Typed | Kind::Formatted => {}
        }
        if let Some(place) = editing.flow.new_box() {
            // Typed or pasted, what first goes in a new box writes it, as "Insert text box".
            return self.write_new_box(&editing, place, after, kind);
        }
        let found = editing.flow.find(&flows);
        // An empty shape's, box's or component's first text, as a new box's, starts at its first
        // visible character: what has none writes nothing, and the thing stays as it was.
        let after = if found.is_none() && editing.flow.region().is_none() {
            let Some(paras) = from_first_visible(&after.paras) else {
                // A choice of kind of paragraph is kept for what's typed next; blank text isn't.
                let kept = if kind == Kind::Formatted { after } else { editing.edit.clone() };
                self.keep_typed(kept, editing.run);
                return Ok(Vec::new());
            };
            let end = TextEdit::new(paras.clone()).select_all().selection.end();
            TextEdit { typing_style: after.typing_style.clone(), ..TextEdit::new(paras).move_to(end, false) }
        } else {
            after
        };
        let change = match (found, editing.flow.region()) {
            (Some(index), _) => write_flow(self.talk(), slide, &flows[index], &after.paras, &content),
            // An empty region's first paragraph is written once it holds something.
            (None, Some(_)) if !holds_something(&after.paras) => {
                self.keep_typed(after, editing.run);
                return Ok(Vec::new());
            }
            (None, Some(region)) => write_into_region(self.talk(), slide, region, &after.paras),
            (None, None) => match editing.flow.tag_start() {
                Some(start) => guarded(|| write_into_tag(self.talk(), &content, slide, start, &after.paras))?,
                None => None,
            },
        };
        let Some(change) = change else {
            return Err(EditRefused::CouldNotBeTyped);
        };
        if change.is_empty() {
            self.keep_typed(after, editing.run);
            return Ok(Vec::new());
        }
        let typed = kind == Kind::Typed;
        let run = match editing.run {
            Some(run) if typed && self.history.is_open(run) => EditRun::Continue(run),
            _ => EditRun::New,
        };
        let expected = storable(&after.paras);
        // Writing into an empty region adds its text; anything else keeps the slide's count.
        let count = flows.len() + usize::from(found.is_none());
        let key = editing.flow.clone();
        let reads_back = |candidate: &Talk| {
            let flows = flows_in(candidate, slide, &content);
            flows.len() == count
                && key
                    .find(&flows)
                    .is_some_and(|index| flows[index].paras == expected)
        };
        let label = label.unwrap_or(if typed { TYPING } else { FORMAT_TEXT });
        let changed = self.make_text(label, slide, change, Join::Run(run), &reads_back)?;
        self.note_step(
            run,
            TextMark {
                slide,
                flow: editing.flow,
                before: editing.edit.selection,
                after: Some(after.selection),
                flow_after: None,
                session: editing.session,
            },
        );
        let run = if typed { self.history.open_run() } else { None };
        self.keep_typed(after, run);
        Ok(changed)
    }

    /// Aligns the text being typed `align`, and returns the slides to show again. It's the whole
    /// region the text is in that's aligned, or the whole box, as one step to undo, and typing
    /// goes on where it was: the paragraphs typed in are as they were, only where they're drawn
    /// changes. Aligning as it's set to be aligned already changes nothing and makes no step.
    pub fn align_text(&mut self, align: Align) -> Result<Vec<usize>, EditRefused> {
        self.compose(None);
        let given = self.give_held_first();
        let Some(editing) = self.editing.clone() else {
            return Ok(given);
        };
        let slide = editing.slide;
        if let FlowKey::NewBox { x, y, .. } = editing.flow {
            // Kept for the box, which is written with it.
            let align = match align {
                Align::Left => None,
                Align::Center => Some("center"),
                Align::Right => Some("right"),
            };
            if let Some(editing) = &mut self.editing {
                editing.flow = FlowKey::NewBox { x, y, align };
            }
            return Ok(given);
        }
        if slide >= self.slide_count() {
            return Ok(given);
        }
        let written = match align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        };
        let change = guarded(|| {
            let talk = self.talk();
            let place = typed_place(talk, &editing, &self.content_tags())?;
            if text_align(talk, slide, &place) == Some(align) {
                return Some(Change { edits: Vec::new() });
            }
            set_align(talk, slide, &place, written)
        })?
        .ok_or(EditRefused::CouldNotBeMade)?;
        if change.is_empty() {
            return Ok(given);
        }
        let depth = self.history.depth();
        // Not through `make`: the typing isn't finished first, as it goes on.
        let changed =
            self.try_make(ALIGN_TEXT, change, Holds::Slide(slide), slide, slide, Join::Alone, None)?;
        if self.history.depth() > depth {
            let selection = editing.edit.selection;
            self.history.mark_last(TextMark {
                slide,
                flow: editing.flow,
                before: selection,
                after: Some(selection),
                flow_after: None,
                session: editing.session,
            });
        }
        // What's typed next is a step of its own.
        self.end_typing_run();
        Ok(joined(given, changed))
    }

    /// How the text being typed is set to be aligned: its region's or its box's alignment, if
    /// any text is being typed.
    pub fn text_align(&self) -> Option<Align> {
        let editing = self.editing.as_ref()?;
        if editing.flow.new_box().is_some() {
            return Some(match editing.flow.new_box_align() {
                Some("center") => Align::Center,
                Some("right") => Align::Right,
                _ => Align::Left,
            });
        }
        let talk = self.talk();
        guarded(|| text_align(talk, editing.slide, &typed_place(talk, editing, &self.content_tags())?))
            .ok()
            .flatten()
    }

    /// Shows `composing` in place of the selection of the text being typed, as the system's
    /// input method composes it, or, with `None`, drops what it was composing. Nothing is
    /// written to the talk: committing it is typing it, through [`Self::apply_text`].
    pub fn compose(&mut self, composing: Option<Composing>) {
        if let Some(editing) = &mut self.editing {
            editing.composing = composing;
        }
    }

    /// Ends the run of typing, so that what's typed next is a step of its own, as after a pause.
    pub fn end_typing_run(&mut self) {
        if let Some(editing) = &mut self.editing {
            editing.run = None;
        }
    }

    /// The tags of the talk's components whose content is a box of text of its own as things
    /// stand: those whose build is loaded, works and says it takes content. Nothing is loaded
    /// to find out, so this is what the engine's text layout reads the slides with too.
    pub(super) fn content_tags(&self) -> Vec<String> {
        self.deck.components().iter().filter(|info| self.deck.takes_content(&info.tag)).map(|info| info.tag.clone()).collect()
    }

    /// The flows of slide `slide` of `talk`, as the slide's text layout counts them.
    pub fn text_flows(&self, talk: &Talk, slide: usize) -> Vec<TextFlow> {
        flows_in(talk, slide, &self.content_tags())
    }
}

/// Whether `editing` reads as the talk holds it in `flows`, its slide's text: the flow its key
/// finds holds exactly its paragraphs, or, in an empty region before anything is written, the
/// region has no text and it holds one empty paragraph.
fn reads_as_typed(talk: &Talk, content: &[String], flows: &[slides_format::TextFlow], editing: &EditingText) -> bool {
    match (editing.flow.find(flows), editing.flow.region()) {
        (Some(index), _) => flows[index].paras == storable(&editing.edit.paras),
        (None, Some(region)) => {
            !region_has_text(flows, region) && !holds_something(&editing.edit.paras)
        }
        (None, None) if editing.flow.new_box().is_some() => !holds_something(&editing.edit.paras),
        (None, None) => {
            let start = editing.flow.tag_start();
            start.is_some_and(|start| tag_starting_at(talk, editing.slide, start).is_some_and(|tag| waits_for_text(tag, content)))
                && !holds_something(&editing.edit.paras)
        }
    }
}

/// The flows of slide `slide` of `talk`, with the tags in `content` taking content, as the
/// engine's text layout reads them: see [`Document::content_tags`].
pub(super) fn flows_in(talk: &Talk, slide: usize, content: &[String]) -> Vec<TextFlow> {
    text_flows_with(talk, slide, &|tag| content.iter().any(|name| name == tag))
}

/// [`write_flow_with`], with the tags in `content` taking content.
pub(super) fn write_flow(talk: &Talk, slide: usize, flow: &TextFlow, paras: &[Para], content: &[String]) -> Option<Change> {
    write_flow_with(talk, slide, flow, paras, &|tag| content.iter().any(|name| name == tag))
}

/// Whether `tag` is one typing on it puts words in: a shape, a box, or one of the talk's own
/// components that draws what's written between its tags, whose tags are `content`.
fn takes_text(tag: &Tag, content: &[String]) -> bool {
    matches!(tag.name.as_str(), "Shape" | "Box") || content.contains(&tag.name)
}

/// Whether `tag` takes typing and has nothing in it yet.
pub(super) fn waits_for_text(tag: &Tag, content: &[String]) -> bool {
    takes_text(tag, content) && tag.children.is_empty()
}

/// The tag on slide `slide` of `talk`, at any depth, that starts at byte `start`.
pub(super) fn tag_starting_at(talk: &Talk, slide: usize, start: usize) -> Option<&Tag> {
    fn within(blocks: &[Block], start: usize) -> Option<&Tag> {
        blocks.iter().find_map(|block| match block {
            Block::Tag(tag) if tag.range.start == start => Some(tag),
            Block::Tag(tag) => within(&tag.children, start),
            Block::List { items, .. } => items.iter().find_map(|item| within(item, start)),
            _ => None,
        })
    }
    let slide = talk.slides.get(slide).filter(|slide| slide.error.is_none())?;
    within(&slide.blocks, start)
}

/// `paras` from their first visible character on ([`is_visible_char`]): the paragraphs before
/// it, and what's before it in its own, left out. `None` if there's none.
pub(super) fn from_first_visible(paras: &[Para]) -> Option<Vec<Para>> {
    let visible = |para: &Para| para.spans.iter().any(|span| span.text.chars().any(is_visible_char));
    let first = paras.iter().position(visible)?;
    let mut out = paras[first..].to_vec();
    let spans = &mut out[0].spans;
    while let Some(span) = spans.first_mut() {
        let rest = span.text.trim_start_matches(|c: char| !is_visible_char(c));
        if rest.is_empty() {
            spans.remove(0);
        } else {
            span.text = rest.to_string();
            break;
        }
    }
    Some(out)
}

/// The change that writes a new text box, at (`x`, `y`) and [`TEXT_BOX_SIZE`], holding `paras`,
/// at the end of slide `slide`'s content, as one edit, and where its tag starts once it's made:
/// the box is written holding one empty paragraph, and that then written as `paras`. `None` if
/// it can't be.
pub(super) fn new_box_change(talk: &Talk, slide: usize, (x, y): (u32, u32), align: Option<&str>, paras: &[Para], content: &[String]) -> Option<(Change, usize)> {
    let place = [x.to_string(), y.to_string(), insert::whole(TEXT_BOX_SIZE.0), insert::whole(TEXT_BOX_SIZE.1)];
    let mut attrs: Vec<(&str, &str)> = ["x", "y", "w", "h"].into_iter().zip(place.iter().map(String::as_str)).collect();
    attrs.extend(align.map(|align| ("align", align)));
    let text = tag_text(&NewTag { name: "Box", attrs: &attrs, body: Some("&nbsp;") })?;
    let opened = insert_block(talk, slide, &text)?;
    let edit = opened.edits.first()?;
    let written = if edit.text.contains("\r\n") { text.replace('\n', "\r\n") } else { text.clone() };
    let start = edit.range.start + edit.text.find(&written)?;
    let held = reparse(talk, opened.apply(&talk.source));
    let flows = flows_in(&held, slide, content);
    let index = FlowKey::Tag { start }.find(&flows)?;
    let filled = write_flow(&held, slide, &flows[index], paras, content)?;
    Some((one_edit(&talk.source, &filled.apply(&held.source)), start))
}

/// The change that puts `paras` in the shape, box or component taking content, one of `content`,
/// with nothing in it whose tag starts at byte `start` of slide `slide`: the tag given a body of
/// one empty paragraph, and that written as `paras`, as one edit inside the tag. `None` if it
/// can't be.
fn write_into_tag(talk: &Talk, content: &[String], slide: usize, start: usize, paras: &[Para]) -> Option<Change> {
    let tag = tag_starting_at(talk, slide, start).filter(|tag| waits_for_text(tag, content))?;
    let opened = set_tag_body(talk, slide, &tag.range, "&nbsp;")?;
    let held = reparse(talk, opened.apply(&talk.source));
    let flows = flows_in(&held, slide, content);
    let index = FlowKey::Tag { start }.find(&flows)?;
    let written = write_flow(&held, slide, &flows[index], paras, content)?;
    let text = written.apply(&held.source);
    Some(one_edit(&talk.source, &text))
}

/// A colour chosen for a selection of text being typed, and the text it was chosen in.
#[derive(Clone, Debug)]
pub(super) struct HeldColor {
    color: String,
    slide: usize,
    flow: FlowKey,
    selection: Selection,
    paras: Vec<Para>,
}

/// The slides `a` and `b` say to show again, each once.
pub(super) fn joined(mut a: Vec<usize>, b: Vec<usize>) -> Vec<usize> {
    for slide in b {
        if !a.contains(&slide) {
            a.push(slide);
        }
    }
    a
}

/// Where `editing`'s text is on its slide: its region, or the box it's in.
fn typed_place(talk: &Talk, editing: &EditingText, content: &[String]) -> Option<FlowPlace> {
    match &editing.flow {
        FlowKey::Region { name, .. } => Some(FlowPlace::Region(name.clone())),
        FlowKey::Cell { .. } => {
            let flows = flows_in(talk, editing.slide, content);
            editing.flow.find(&flows).map(|index| flows[index].place.clone())
        }
        FlowKey::Tag { start } => {
            tag_starting_at(talk, editing.slide, *start).map(|tag| FlowPlace::Tag(tag.range.clone()))
        }
        FlowKey::NewBox { .. } => None,
    }
}
