//! The starter templates bundled in the app: small talks, their text and any pictures they
//! show, that a new presentation starts as. A template is data, never code: no component of
//! its own, no build, nothing compiled. Nothing here needs the window, so a template can be
//! read on any thread.

use std::sync::Arc;

use slides_engine::MemoryFiles;

pub use slides_engine::contrast;

/// A talk a new presentation can start as.
pub struct Template {
    /// What the app knows it by, which never changes.
    pub id: &'static str,
    /// What a person sees it called.
    pub name: &'static str,
    /// One line, read under its name, saying what it is.
    pub says: &'static str,
    /// The talk's text.
    pub slides: &'static str,
    /// Every file the text uses, by its path in the talk.
    pub files: &'static [(&'static str, &'static [u8])],
}

static TEMPLATES: [Template; 5] = [
    Template {
        id: "light",
        name: "Light",
        says: "White, with dark text",
        slides: include_str!("../templates/light/slides.md"),
        files: &[],
    },
    Template {
        id: "dark",
        name: "Dark",
        says: "Deep navy, with light text",
        slides: include_str!("../templates/dark/slides.md"),
        files: &[],
    },
    Template {
        id: "warm",
        name: "Warm",
        says: "Cream, with dark brown text",
        slides: include_str!("../templates/warm/slides.md"),
        files: &[],
    },
    Template {
        id: "ocean",
        name: "Ocean",
        says: "Deep blue, with white text and gold",
        slides: include_str!("../templates/ocean/slides.md"),
        files: &[],
    },
    Template {
        id: "tour",
        name: "Tour",
        says: "A short talk that shows what a slide can hold",
        slides: include_str!("../templates/tour/slides.md"),
        files: &[("assets/photo.png", include_bytes!("../templates/tour/assets/photo.png"))],
    },
];

/// Every template, in the order they're offered: Light, Dark, Warm, Ocean and Tour.
pub fn templates() -> &'static [Template] {
    &TEMPLATES
}

/// The template known as `id`, if there is one.
pub fn template(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.id == id)
}

/// The files of `template`, held in memory, as a talk made from it reads them.
pub fn memory_files(template: &Template) -> MemoryFiles {
    MemoryFiles::new(template.files.iter().map(|(path, bytes)| (path.to_string(), Arc::from(*bytes))).collect())
}
