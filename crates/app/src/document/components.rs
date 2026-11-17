//! The talk's own components, and the palette that offers them.

use super::*;

impl Document {
    /// The talk's own components, and whether each is built.
    pub fn components(&self) -> &[ComponentInfo] {
        self.deck.components()
    }

    /// What's wrong with the talk's own components, as found when they were last looked at.
    pub fn component_problems(&self) -> &[Diagnostic] {
        self.deck.component_problems()
    }

    /// Looks at the talk's components again, after a build or a change to one of them.
    pub fn reload_components(&mut self) -> Reload {
        let changed = self.deck.refresh_components();
        if changed.is_empty() {
            Reload::Unchanged
        } else {
            self.components_looked_at();
            Reload::Changed(changed)
        }
    }

    /// What the component palette offers: a chart first, then the talk's own components in name
    /// order, each with how built it is. The tags with tools of their own aren't in it. It's
    /// worked out as the talk's components change, not each time it's asked for.
    pub fn palette(&mut self) -> Vec<PaletteEntry> {
        self.palette.clone()
    }

    /// Lists the palette again from the talk's components as they are now: which there are and
    /// how built each is. Nothing is loaded: what each takes is known only once its build is.
    pub(super) fn components_looked_at(&mut self) {
        let deck = &self.deck;
        let listed = guarded(|| {
            let own: Vec<String> = deck.components().iter().map(|info| info.tag.clone()).collect();
            let theirs = deck.available_components().into_iter().filter(|tag| own.contains(tag) && !LISTED_BUILT_INS.contains(&tag.as_str()));
            LISTED_BUILT_INS.iter().map(|tag| tag.to_string()).chain(theirs).collect::<Vec<_>>()
        });
        let entries: Vec<PaletteEntry> = listed.unwrap_or_default().iter().map(|tag| self.palette_entry(tag)).collect();
        self.palette = entries;
    }

    /// The palette's entry for `tag`, as far as is known without loading anything.
    fn palette_entry(&self, tag: &str) -> PaletteEntry {
        let state = self.deck.components().iter().find(|info| info.tag == tag).map(|info| info.state);
        let (takes_content, loading) = match self.deck.loaded_schema(tag) {
            SchemaLoad::Loaded(schema) => (schema.children, false),
            SchemaLoad::Failed => (false, false),
            _ => (false, state != Some(ComponentState::NeedsBuilding) && !self.palette_given_up.contains(tag)),
        };
        PaletteEntry { tag: tag.to_string(), label: palette::label(tag), ours: state.is_some(), state, takes_content, loading }
    }

    /// Starts an opening of the palette: each of its components whose build isn't loaded yet is
    /// to be, by [`Self::palette_step`], with a budget of its own.
    pub fn begin_palette(&mut self) {
        self.palette_given_up.clear();
        self.components_looked_at();
    }

    /// Loads the build of the first of the palette's components still loading, and fills in its
    /// entry: one module at a time, which the sandbox holds to a quarter of a second of running.
    /// Says whether any is still loading.
    pub fn palette_step(&mut self) -> bool {
        let Some(at) = self.palette.iter().position(|entry| entry.loading) else {
            return false;
        };
        let tag = self.palette[at].tag.clone();
        let deck = &mut self.deck;
        let started = Instant::now();
        let _ = guarded(|| deck.component_schema(&tag));
        self.palette_longest_step = self.palette_longest_step.max(started.elapsed());
        if let SchemaLoad::NotLoaded = self.deck.loaded_schema(&tag) {
            // Nothing could be loaded for it: it isn't tried again this opening.
            self.palette_given_up.insert(tag.clone());
        }
        self.palette[at] = self.palette_entry(&tag);
        self.palette.iter().any(|entry| entry.loading)
    }

    /// Stops loading for this opening of the palette: what isn't loaded yet is listed as it is,
    /// to be loaded if it's chosen.
    pub fn give_up_palette(&mut self) {
        let loading: Vec<String> = self.palette.iter().filter(|entry| entry.loading).map(|entry| entry.tag.clone()).collect();
        self.palette_given_up.extend(loading);
        self.components_looked_at();
    }

    /// The longest one [`Self::palette_step`] has taken, for tests.
    #[doc(hidden)]
    pub fn palette_longest_step(&self) -> Duration {
        self.palette_longest_step
    }

    /// How many of the talk's modules have been loaded, or tried, for tests.
    #[doc(hidden)]
    pub fn modules_loaded(&self) -> usize {
        self.deck.modules_loaded()
    }

    /// Puts the component `tag` from the palette on the current slide, [`COMPONENT_SIZE`] big,
    /// as one step named "Insert component", and selects it: written with what
    /// [`palette::default_attrs`] gives, and, if it takes content, an empty body. One the
    /// palette doesn't offer, or one with no build, puts nothing there. One whose build can't
    /// be loaded is refused. Nothing is built.
    pub fn insert_component(&mut self, tag: &str) -> Result<Vec<usize>, EditRefused> {
        let listed = self.palette.iter().any(|entry| entry.tag == tag && entry.state != Some(ComponentState::NeedsBuilding));
        if !listed {
            return Ok(Vec::new());
        }
        let deck = &mut self.deck;
        let schema = guarded(|| deck.component_schema(tag))?;
        if let Some(at) = self.palette.iter().position(|entry| entry.tag == tag) {
            self.palette[at] = self.palette_entry(tag);
        }
        let schema = schema.ok_or(EditRefused::ComponentNotLoaded)?;
        let attrs = palette::default_attrs(&schema, &self.deck.theme());
        let attrs: Vec<(&str, &str)> = attrs.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        let body = schema.children.then_some("");
        self.insert_sized(INSERT_COMPONENT, tag, &attrs, COMPONENT_SIZE, body)
    }
}
