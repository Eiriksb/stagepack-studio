//! Putting new things on a slide: tags, shapes, text boxes, pictures and tables.

use super::*;

impl Document {
    /// Puts a `name` tag with `attrs`, and `body` inside it if it's given, on the current slide,
    /// after its content and before its notes, as one step named `label`, and selects it. It's
    /// given the size the engine gives a new one, and a place in the middle of the slide that
    /// nothing else on it has its corner at: see [`Self::insert_sized`].
    pub fn insert_tag(
        &mut self,
        label: &'static str,
        name: &str,
        attrs: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<Vec<usize>, EditRefused> {
        let deck = &mut self.deck;
        let size = guarded(|| deck.default_size(name, attrs))?;
        self.insert_sized(label, name, attrs, size, body)
    }

    /// [`Self::insert_tag`], `size` big: the tag's `x`, `y`, `w` and `h`, whole numbers, come after
    /// `attrs`, at the place [`insert::place_for`] gives among the corners of the things on the
    /// slide. Typing on the slide ends first. Undone, the tag goes and nothing is selected; made
    /// again, it's selected again.
    pub fn insert_sized(
        &mut self,
        label: &'static str,
        name: &str,
        attrs: &[(&str, &str)],
        size: (f32, f32),
        body: Option<&str>,
    ) -> Result<Vec<usize>, EditRefused> {
        // Where things are is worked out from the talk as tidied.
        let tidied = self.finish_before_edit()?;
        let slide = self.current;
        let layout = self.text_layout(slide);
        let taken: Vec<(f32, f32)> = layout.elements().iter().map(|element| (element.rect.x, element.rect.y)).collect();
        let (x, y) = insert::place_for(size, &taken);
        let place = [insert::whole(x), insert::whole(y), insert::whole(size.0.max(1.0)), insert::whole(size.1.max(1.0))];
        let mut all: Vec<(&str, &str)> = attrs.to_vec();
        all.extend(["x", "y", "w", "h"].into_iter().zip(place.iter().map(String::as_str)));
        let text = tag_text(&NewTag { name, attrs: &all, body }).ok_or(EditRefused::CouldNotBeMade)?;
        self.insert_text(label, &text, tidied)
    }

    /// Writes `text`, one block, at the end of the current slide's content, as one step named
    /// `label`, and selects it. `tidied` says whether finishing the typing first changed the
    /// talk. Undone, it goes and nothing is selected; made again, it's selected again.
    pub(super) fn insert_text(&mut self, label: &'static str, text: &str, tidied: bool) -> Result<Vec<usize>, EditRefused> {
        self.insert_texts(label, &[text.to_string()], tidied, None)
    }

    /// Writes `texts`, each one block, at the end of the current slide's content, in order, as
    /// one step named `label`, and selects them, as [`Self::insert_text`] does one. Undone,
    /// `before` is selected again.
    pub(super) fn insert_texts(
        &mut self,
        label: &'static str,
        texts: &[String],
        tidied: bool,
        before: Option<selection::Selection>,
    ) -> Result<Vec<usize>, EditRefused> {
        let slide = self.current;
        let (change, sources) = built(self.talk(), |talk| inserted(talk, slide, texts))?.ok_or(EditRefused::CouldNotBeMade)?;
        let depth = self.history.depth();
        let mut changed = self.make(label, change, Holds::Slide(slide), slide, slide, None)?;
        if tidied && !changed.contains(&slide) {
            changed.push(slide);
        }
        if self.history.depth() > depth
            && let Some(sources) = sources
            && self.pick(slide, sources.into_iter().map(Thing::Block).collect())
        {
            self.history.select_mark_last(SelectMark { before, after: self.selection.clone() });
        }
        Ok(changed)
    }

    /// Puts a shape of kind `kind`, one of [`insert::SHAPE_KINDS`], on the current slide, as
    /// [`Self::insert_sized`] does: filled with the theme's accent, or, for a line or an arrow,
    /// drawn in its text colour.
    pub fn insert_shape(&mut self, kind: &str) -> Result<Vec<usize>, EditRefused> {
        let theme = self.deck.theme();
        let attrs = insert::shape_attrs(kind, &theme);
        let attrs: Vec<(&str, &str)> = attrs.iter().map(|(name, value)| (*name, value.as_str())).collect();
        self.insert_sized(INSERT_SHAPE, "Shape", &attrs, insert::shape_size(kind), None)
    }

    /// Starts typing in a new text box on the current slide, which isn't written until something
    /// is typed in it: the first character typed writes it, with that character, as one step
    /// named "Insert text box". Till then it's nothing of the talk's: no change, no step, nothing
    /// to save. Anything that ends the typing drops it, writing nothing. It goes in the middle of
    /// the slide, stepped aside from the things on it, and from a new box already waiting there,
    /// which it takes the place of. Typing elsewhere ends first, and nothing stays selected.
    pub fn insert_text_box(&mut self) -> Result<Vec<usize>, EditRefused> {
        let waiting = self
            .new_box()
            .filter(|(slide, _)| *slide == self.current)
            .map(|(_, (x, y, _, _))| (x, y));
        let tidied = self.finish_before_edit()?;
        // Ending typing in a new box drops it; ending any other may tidy the talk.
        self.editing = None;
        self.deselect();
        let slide = self.current;
        let layout = self.text_layout(slide);
        let mut taken: Vec<(f32, f32)> = layout.elements().iter().map(|element| (element.rect.x, element.rect.y)).collect();
        taken.extend(waiting);
        let (x, y) = insert::place_for(TEXT_BOX_SIZE, &taken);
        self.editing = Some(EditingText {
            session: self.new_session(),
            ..EditingText::in_new_box(slide, x.max(0.0) as u32, y.max(0.0) as u32)
        });
        Ok(if tidied { vec![slide] } else { Vec::new() })
    }

    /// The new text box typing has begun in that isn't written yet, if there's one: its slide,
    /// and where it is, `(x, y, w, h)`, in slide units.
    pub fn new_box(&self) -> Option<(usize, (f32, f32, f32, f32))> {
        let editing = self.editing.as_ref()?;
        let (x, y) = editing.flow.new_box()?;
        Some((editing.slide, (x as f32, y as f32, TEXT_BOX_SIZE.0, TEXT_BOX_SIZE.1)))
    }

    /// Writes the new text box `editing` is typing in, at (`x`, `y`), holding `after`'s
    /// paragraphs from their first visible character on, as one step of its own named "Insert
    /// text box", and goes on typing in it, at the end of what's written. Input with no visible
    /// character in it, as Enter, a space or a paste of blank lines, writes nothing, and the box
    /// stays as it was, waiting.
    pub(super) fn write_new_box(&mut self, editing: &EditingText, (x, y): (u32, u32), after: TextEdit, kind: Kind) -> Result<Vec<usize>, EditRefused> {
        let content = self.content_tags();
        let Some(paras) = from_first_visible(&after.paras) else {
            // A choice of kind of paragraph is kept for what's typed next; blank text isn't.
            let kept = if kind == Kind::Formatted { after } else { editing.edit.clone() };
            self.keep_typed(kept, None);
            return Ok(Vec::new());
        };
        let end = TextEdit::new(paras.clone()).select_all().selection.end();
        let after = TextEdit { typing_style: after.typing_style.clone(), ..TextEdit::new(paras).move_to(end, false) };
        let slide = editing.slide;
        let align = editing.flow.new_box_align();
        let (change, start) = guarded(|| new_box_change(self.talk(), slide, (x, y), align, &after.paras, &content))?
            .ok_or(EditRefused::CouldNotBeTyped)?;
        let count = guarded(|| flows_in(self.talk(), slide, &content).len())? + 1;
        let expected = storable(&after.paras);
        let key = FlowKey::Tag { start };
        let reads_back = |candidate: &Talk| {
            let flows = flows_in(candidate, slide, &content);
            flows.len() == count && key.find(&flows).is_some_and(|index| flows[index].paras == expected)
        };
        let depth = self.history.depth();
        let changed = self
            .try_make(INSERT_TEXT_BOX, change, Holds::Slide(slide), slide, slide, Join::Alone, Some(&reads_back))
            .map_err(|_| EditRefused::CouldNotBeTyped)?;
        if self.history.depth() > depth {
            // Undone, nothing's left to type in; made again, typing goes on after what was typed.
            self.history.mark_last(TextMark {
                slide,
                flow: key.clone(),
                before: Selection::caret(Pos::default()),
                after: Some(after.selection),
                flow_after: None,
                session: editing.session,
            });
        }
        if let Some(editing) = &mut self.editing {
            editing.flow = key;
        }
        self.keep_typed(after, None);
        Ok(changed)
    }

    /// Adds `bytes`, a picture, to the talk as a file named as like `suggested` as is safe, and
    /// puts it on the current slide as [`Self::insert_tag`] does: its own size, made smaller to fit
    /// in half the slide. A picture the talk has already is that one.
    pub fn insert_picture(&mut self, suggested: &str, bytes: Vec<u8>) -> Result<(), InsertFailed> {
        let path = self.add_file(FileKind::Picture, suggested, bytes).map_err(InsertFailed::File)?;
        self.insert_tag(INSERT_PICTURE, "Picture", &[("src", &path)], None).map_err(InsertFailed::Edit)?;
        Ok(())
    }

    /// Puts a table of `columns` columns and `rows` rows under its heading row, every cell empty,
    /// at the end of the current slide's content, as one step named "Insert table", and starts
    /// typing in its first heading cell. Typing on the slide ends first, and nothing stays
    /// selected. Undone, the table goes; made again, it's selected.
    pub fn insert_table(&mut self, columns: usize, rows: usize) -> Result<Vec<usize>, EditRefused> {
        let text = new_table(columns, rows).ok_or(EditRefused::CouldNotBeMade)?;
        let tidied = self.finish_before_edit()?;
        let slide = self.current;
        let change = built(self.talk(), |talk| insert_block(talk, slide, &text))?.ok_or(EditRefused::CouldNotBeMade)?;
        let before = tables_on(self.talk(), slide);
        let moved = change.clone();
        self.deselect();
        let depth = self.history.depth();
        let mut changed = self.make(INSERT_TABLE, change, Holds::Slide(slide), slide, slide, None)?;
        if tidied && !changed.contains(&slide) {
            changed.push(slide);
        }
        // The new table is the one on the slide that isn't one of those it had, moved along.
        let kept: Vec<Range<usize>> = before.iter().filter_map(|table| selection::moved(table, &moved)).collect();
        let new = tables_on(self.talk(), slide).into_iter().find(|table| !kept.contains(table));
        if self.history.depth() > depth
            && let Some(table) = new
        {
            self.history.select_mark_last(SelectMark::blocks(slide, None, Some(table.clone())));
            self.land_in_cell(slide, table.start, (0, 0), false);
        }
        Ok(changed)
    }
}

/// The change that writes `texts`, each one block, at the end of slide `slide`'s content, in
/// order, as [`insert_block`] writes one, and where each is once it's written, in the talk's
/// line endings, if that can be found. A single block gets the change [`insert_block`] gives.
fn inserted(talk: &Talk, slide: usize, texts: &[String]) -> Option<(Change, Option<Vec<Range<usize>>>)> {
    inserted_at_once(talk, slide, texts).or_else(|| inserted_in_turn(talk, slide, texts))
}

/// [`inserted`], each block written in turn on the talk as the ones before leave it.
fn inserted_in_turn(talk: &Talk, slide: usize, texts: &[String]) -> Option<(Change, Option<Vec<Range<usize>>>)> {
    let mut now = talk.clone();
    let mut first = None;
    let mut sources = Vec::new();
    let mut found = true;
    for text in texts {
        let change = insert_block(&now, slide, text)?;
        let source = change.edits.first().and_then(|edit| {
            let written = if edit.text.contains("\r\n") { text.replace('\n', "\r\n") } else { text.clone() };
            let at = edit.range.start + edit.text.find(&written)?;
            Some(at..at + written.len())
        });
        // Each written goes after those before it, which stay where they are.
        if made_in_turn(&mut now, &change, &mut sources).is_none() {
            found = false;
            sources.clear();
        }
        match source {
            Some(source) if found => sources.push(source),
            _ => found = false,
        }
        first.get_or_insert(change);
    }
    let change = if texts.len() == 1 { first? } else { one_edit(&talk.source, &now.source) };
    Some((change, found.then_some(sources)))
}

/// [`inserted`] for several blocks: the first written by [`insert_block`], and the others in the
/// same edit after it, each after a blank line, as it would write each after the one before.
/// Only where the slide then ends with exactly those blocks, where they were written: one
/// reading of the talk, however many there are. `None` otherwise, for them to be written in turn.
fn inserted_at_once(talk: &Talk, slide: usize, texts: &[String]) -> Option<(Change, Option<Vec<Range<usize>>>)> {
    if texts.len() < 2 {
        return None;
    }
    let first = insert_block(talk, slide, &texts[0])?;
    let [edit] = first.edits.as_slice() else {
        return None;
    };
    let eol = if edit.text.contains("\r\n") { "\r\n" } else { "\n" };
    let written: Vec<String> = texts.iter().map(|text| text.replace('\n', eol)).collect();
    let at = edit.text.find(&written[0])?;
    let mut text = edit.text[..at].to_string();
    let mut sources = Vec::with_capacity(texts.len());
    for (index, block) in written.iter().enumerate() {
        if index > 0 {
            text.push_str(eol);
            text.push_str(eol);
        }
        let start = edit.range.start + text.len();
        sources.push(start..start + block.len());
        text.push_str(block);
    }
    text.push_str(&edit.text[at + written[0].len()..]);
    let change = Change::single(SourceEdit { range: edit.range.clone(), text });
    let after = reparse(talk, change.apply(&talk.source));
    let blocks = &after.slides.get(slide)?.blocks;
    let ends: Vec<Range<usize>> = blocks.iter().rev().take(texts.len()).rev().map(|block| block.range().clone()).collect();
    let before = talk.slides.get(slide)?.blocks.len();
    let exact = after.slides.len() == talk.slides.len() && blocks.len() == before + texts.len() && ends == sources;
    exact.then_some((change, Some(sources)))
}
