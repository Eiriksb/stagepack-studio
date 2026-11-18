//! The colour fields: a colour held while it's picked, and the colours offered.

use super::*;

impl Inspector {
    /// Keeps `color`, dragged in colour field `name`'s picker, for when the picker closes, or
    /// with `None` lets go of what's held. A drag is of the talk as it was when it began.
    pub(super) fn hold_color(&mut self, name: &str, color: Option<String>, cx: &App) {
        let text = self.text_hash(cx);
        if let Some(Field { widget: Widget::Color { held, .. }, .. }) = self.field_mut(name) {
            *held = color.map(|color| Held { text: held.as_ref().map_or(text, |held| held.text), color });
        }
    }

    /// Writes the colour held for colour field `name`, if there is one and nothing has changed
    /// the talk's text since the drag began.
    pub(super) fn give_held_color(&mut self, name: &str, cx: &mut Context<Self>) {
        let held = match self.field_mut(name) {
            Some(Field { widget: Widget::Color { held, .. }, .. }) => held.take(),
            _ => None,
        };
        if let Some(held) = held {
            if self.is_current(held.text, cx) {
                self.write_color(name, &held.color, cx);
            }
            cx.notify();
        }
    }

    /// A hash of the talk's text now. A build of its components or a save to another place
    /// leaves it as it is.
    fn text_hash(&self, cx: &App) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        if let Some(document) = &self.document {
            std::hash::Hash::hash(document.read(cx).source(), &mut hasher);
        }
        std::hash::Hasher::finish(&hasher)
    }

    /// Whether the talk's text is still what hashed to `text`.
    pub(super) fn is_current(&self, text: u64, cx: &App) -> bool {
        self.text_hash(cx) == text
    }

    /// Writes `color`, as `#rrggbb`, to colour field `name`, unless it's the colour the talk
    /// gives already, however that's written: a colour named by hand stays as it's written.
    pub(super) fn write_color(&mut self, name: &str, color: &str, cx: &mut Context<Self>) {
        let now = self.talk_attr(name, cx).as_deref().and_then(color_of).map(written_color);
        if now.as_deref() != Some(color) {
            self.write(name, Some(color), cx).ok();
        }
    }

    pub(super) fn featured_colors(&self, cx: &App) -> Vec<Hsla> {
        let Some(document) = &self.document else {
            return Vec::new();
        };
        let theme = talk_theme(document.read(cx).talk());
        [theme.text, theme.accent, theme.background]
            .into_iter()
            .map(|color| rgb(u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b)).into())
            .collect()
    }
}

/// A colour as the talk writes it, as the picker holds it, if the talk can read it.
pub(super) fn color_of(value: &str) -> Option<Hsla> {
    let color = toolkit::Color::parse(value)?;
    Some(rgb(u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b)).into())
}
