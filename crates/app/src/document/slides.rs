//! The slide commands: adding, duplicating, deleting, moving and hiding slides, and their
//! notes.

use super::*;

/// The layout a slide has when its settings name none.
const DEFAULT_LAYOUT: &str = "content";

/// What Undo shows for giving a slide another layout.
pub const CHANGE_LAYOUT: &str = "Change layout";

impl Document {
    /// Gives the current slide the layout called `layout`, by the engine's name for it, as one
    /// step named "Change layout", and returns the slides to show again. Text keeps its place
    /// where the new layout has an area of the same name, and the rest shows in its main area.
    /// The layout the slide has already makes no step, and leaves the typing be. A name that
    /// isn't a layout's is refused.
    pub fn set_layout(&mut self, layout: &str) -> Result<Vec<usize>, EditRefused> {
        let layout = layout_by_name(layout).ok_or(EditRefused::CouldNotBeMade)?.name;
        if layout == self.layout_here() {
            return Ok(Vec::new());
        }
        let at = self.current;
        let change = self
            .build(|talk| slides_format::set_layout(talk, at, layout))?
            .ok_or(EditRefused::CouldNotBeMade)?;
        self.make(CHANGE_LAYOUT, change, Holds::Slide(at), at, at, None)
    }

    /// The engine's name for the layout of the current slide.
    pub fn layout_here(&self) -> &'static str {
        self.layout_of(self.current)
    }

    /// The engine's name for the layout of slide `slide`: the one its settings name, or the
    /// default with none. A name that isn't a layout's, which the slide is drawn with the
    /// default for, gives "", which is no layout's.
    pub fn layout_of(&self, slide: usize) -> &'static str {
        let named = self
            .talk()
            .slides
            .get(slide)
            .and_then(|slide| slide.settings.as_ref())
            .and_then(|settings| settings.get("layout"))
            .filter(|name| !name.is_empty())
            .unwrap_or(DEFAULT_LAYOUT);
        layout_by_name(named).map_or("", |layout| layout.name)
    }

    /// Adds a slide with the layout called `layout` after the current one, with nothing in
    /// it, so each of its places offers to be typed in, and makes it the current slide. A
    /// name that isn't a layout's is taken as the default one.
    pub fn add_slide(&mut self, layout: &str) -> Result<Vec<usize>, EditRefused> {
        let layout = layout_by_name(layout).map_or(DEFAULT_LAYOUT, |layout| layout.name);
        let text = if layout == DEFAULT_LAYOUT {
            "\n".to_string()
        } else {
            format!("layout: {layout}\n---\n\n")
        };
        let at = self.current;
        let change = self.build(|talk| slides_format::insert_slide(talk, Some(at), &text))?;
        self.make(ADD_SLIDE, change, Holds::Count(1), at, at + 1, None)
    }

    /// Puts a copy of the current slide straight after it, and makes the copy current.
    pub fn duplicate_current(&mut self) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let change = self.build(|talk| slides_format::duplicate_slide(talk, at))?;
        self.make(DUPLICATE_SLIDE, change, Holds::Count(1), at, at + 1, None)
    }

    /// Removes the current slide, unless it's the only one. The one after it becomes
    /// current, or the one before when it was last.
    pub fn delete_current(&mut self) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let Some(change) = self.build(|talk| slides_format::delete_slide(talk, at))? else {
            return Ok(Vec::new());
        };
        let after = at.min(self.slide_count() - 2);
        self.make(DELETE_SLIDE, change, Holds::Count(-1), at, after, None)
    }

    /// Takes the current slide out, as [`Self::delete_current`] does, named for Undo as a cut.
    pub fn cut_current(&mut self) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let Some(change) = self.build(|talk| slides_format::delete_slide(talk, at))? else {
            return Ok(Vec::new());
        };
        let after = at.min(self.slide_count() - 2);
        self.make(CUT_SLIDE, change, Holds::Count(-1), at, after, None)
    }

    /// Moves slide `from` so that `to` is its place afterwards, and makes it current.
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<Vec<usize>, EditRefused> {
        let change = self.build(|talk| slides_format::move_slide(talk, from, to))?;
        self.make(MOVE_SLIDE, change, Holds::Count(0), from, to, None)
    }

    /// Leaves the current slide out of shows, or puts it back in.
    pub fn set_current_hidden(&mut self, hidden: bool) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let change = self.build(|talk| slides_format::set_hidden(talk, at, hidden))?;
        let label = if hidden { HIDE_SLIDE } else { SHOW_SLIDE };
        self.edit_slide(label, at, change, EditRun::New)
    }

    /// Sets the notes of slide `index`, and makes it current. Notes typed into one slide
    /// with nothing else done in between are one step to undo.
    pub fn set_notes(&mut self, index: usize, notes: &str) -> Result<Vec<usize>, EditRefused> {
        let change = self.build(|talk| slides_format::set_notes(talk, index, notes))?;
        if change.is_empty() {
            // Nothing typed: the notes typing, wherever it is, goes on as it was.
            return Ok(Vec::new());
        }
        let run = self
            .typing_notes
            .filter(|&(slide, _)| slide == index)
            .map_or(EditRun::New, |(_, run)| EditRun::Continue(run));
        let changed = self.edit_slide(EDIT_NOTES, index, change, run)?;
        self.typing_notes = self.open_run().map(|run| (index, run));
        Ok(changed)
    }

    /// Whether `bytes` more can go on the slide whose text is being typed: a slide that holds
    /// more than [`MAX_SLIDE_BYTES`] can't be read. With no text being typed, there's room.
    pub fn has_room_for(&self, bytes: usize) -> bool {
        let slide = self.editing.as_ref().and_then(|editing| self.talk().slides.get(editing.slide));
        slide.is_none_or(|slide| slide.range.len().saturating_add(bytes) <= MAX_SLIDE_BYTES)
    }
}
