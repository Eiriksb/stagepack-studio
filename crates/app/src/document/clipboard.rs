//! Copying and pasting slides and things, and the files they bring with them.

use super::*;
use crate::clipboard;

/// What a paste that brought files with it has to say of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Brought {
    /// Some of them couldn't be added, or the text couldn't be made to name them.
    pub left_out: bool,
    /// They were too big to come with it.
    pub too_large: bool,
    /// What to say of a component the talk has a different one of, by the same name.
    pub different: Option<String>,
    /// A pasted component had the name of one every talk has.
    pub built_in: bool,
    /// A component the slide shows came as a build without its source, so it couldn't be brought.
    pub component_left_out: bool,
    /// The stems of the builds that came without their sources.
    builds_alone: Vec<String>,
}

impl Brought {
    /// Marks a component left out if the current slide of `document`, as pasted, shows one missing
    /// that came only as a build.
    fn look_for_lone_builds(&mut self, document: &mut Document) {
        if self.builds_alone.is_empty() {
            return;
        }
        let notices = document.notices();
        let missing = clipboard::missing_components(&notices);
        self.component_left_out = missing.iter().any(|name| self.builds_alone.iter().any(|stem| stem.eq_ignore_ascii_case(name)));
    }

    /// The one thing to say of it, if any: a different component first, then one with a
    /// built-in's name, then files too big, then pictures left out, then a component left out.
    pub fn said(&self) -> Option<String> {
        let said = |is: bool, sentence: &str| is.then(|| sentence.to_string());
        self.different
            .clone()
            .or_else(|| said(self.built_in, clipboard::BUILT_IN_NAME))
            .or_else(|| said(self.too_large, clipboard::FILES_TOO_LARGE))
            .or_else(|| said(self.left_out, clipboard::MISSING_PICTURES))
            .or_else(|| said(self.component_left_out, clipboard::MISSING_COMPONENT))
    }
}

/// The files a paste carries, as a talk's files, to tell which of them its text uses.
struct CarriedFiles<'a>(&'a [PastedFile]);

impl CarriedFiles<'_> {
    fn find(&self, path: &str) -> Option<&PastedFile> {
        let path = slides_engine::normalise(path)?;
        self.0.iter().find(|file| file.path == path)
    }
}

impl TalkFiles for CarriedFiles<'_> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.find(path).map(|file| file.bytes.clone())
    }

    fn len(&self, path: &str) -> Option<u64> {
        self.find(path).map(|file| file.bytes.len() as u64)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        let prefix = format!("{folder}/");
        self.0.iter().filter(|file| file.path.starts_with(&prefix)).map(|file| file.path.clone()).collect()
    }
}

impl Document {
    /// The current slide's own text, to copy it: see [`slides_format::slide_text_for_copy`].
    /// Text being typed on it is in the talk already. `None` if working it out goes wrong.
    pub fn current_for_copy(&self) -> Option<String> {
        let at = self.current;
        worked_out(|| slides_format::slide_text_for_copy(self.talk(), at)).ok()
    }

    /// Adds a slide holding `text`, a slide's own text as [`Self::current_for_copy`] gives it,
    /// perhaps from another talk, after the current one, and makes it current. It's refused,
    /// with nothing changed, unless it makes exactly one slide there, every other slide keeping
    /// its text: a text that would split into several slides, or take in the slide after it,
    /// isn't pasted.
    ///
    /// Whatever `text` is, it's first made a slide's own text by [`crate::clipboard::one_slide`],
    /// with none of the talk's settings, so a pasted slide never sets them; text that doesn't
    /// read as one slide is refused.
    ///
    /// An empty talk, whose text is nothing but whitespace and perhaps a byte order mark, has one
    /// slide with nothing on it: the pasted slide takes its place as the talk's only slide, after
    /// the byte order mark, with the talk's line endings, and a break line before it if it starts
    /// with settings of its own, which only then are read as its settings block. It's refused
    /// unless that makes one slide with none of the talk's settings.
    pub fn paste_slide(&mut self, text: &str) -> Result<Vec<usize>, EditRefused> {
        let Some(slide) = guarded(|| crate::clipboard::one_slide(text))? else {
            return Err(EditRefused::WouldChangeTheSlides);
        };
        let text = slide.text;
        let source = self.source();
        let bom = if source.starts_with('\u{feff}') { "\u{feff}" } else { "" };
        if self.slide_count() == 1 && source[bom.len()..].trim().is_empty() {
            let eol = if source.contains("\r\n") { "\r\n" } else { "\n" };
            let first = guarded(|| if parse_talk(&text).slides.len() == 1 { text.clone() } else { format!("---\n{text}") })?;
            let new = format!("{bom}{}", first.replace('\n', eol));
            let sets_nothing = guarded(|| {
                let talk = parse_talk(&new);
                talk.slides.len() == 1 && talk_settings(&talk).is_empty()
            })?;
            if !sets_nothing {
                return Err(EditRefused::WouldChangeTheSlides);
            }
            let change = Change::single(SourceEdit { range: 0..source.len(), text: new });
            return self.make(PASTE_SLIDE, change, Holds::Count(0), 0, 0, None);
        }
        let at = self.current;
        let change = self.build(|talk| slides_format::insert_slide(talk, Some(at), &text))?;
        self.make(PASTE_SLIDE, change, Holds::Inserted(at + 1), at, at + 1, None)
    }

    /// Pastes the slide `text` as [`Self::paste_slide`] does, with `files`, the files it uses
    /// from the talk it was copied from: they're added first, as [`Self::bring_files`] says, and
    /// each path in its text that had to change is made to name the file as added. A slide that
    /// isn't one is refused before anything is added.
    pub fn paste_slide_with_files(&mut self, text: &str, files: PastedFiles) -> Result<(Vec<usize>, Brought), EditRefused> {
        if guarded(|| crate::clipboard::one_slide(text))?.is_none() {
            return Err(EditRefused::WouldChangeTheSlides);
        }
        let (text, mut brought) = self.bring_files_for(text, files);
        let changed = self.paste_slide(&text)?;
        brought.look_for_lone_builds(self);
        Ok((changed, brought))
    }

    /// Adds those of `files` that `text` uses, as [`Self::bring_files`] says, and gives `text`
    /// naming them as added. If it can't be made to name them exactly, it's given as it is, and
    /// that's left out.
    fn bring_files_for(&mut self, text: &str, files: PastedFiles) -> (String, Brought) {
        let (renames, mut brought) = self.bring_files(text, files);
        if renames.is_empty() {
            return (text.to_string(), brought);
        }
        match guarded(|| added_files::rename_uses(text, &renames)).ok().flatten() {
            Some(renamed) => (renamed, brought),
            None => {
                brought.left_out = true;
                (text.to_string(), brought)
            }
        }
    }

    /// Adds the files of `files`, pasted from another talk with `text`, that `text` uses, as
    /// [`talk_uses`] finds them among the files pasted: a picture or data file a picture or a
    /// tag's attribute names, and a component's source and build when a tag in `text` is the
    /// component the app finds in that source. Which are used is decided before anything is
    /// added, and the rest are dropped: nothing unused is added, named, kept or offered. Each is
    /// added as [`Self::add_file`] adds a dropped one, and the paths that had to change are given,
    /// `(as it was, as added)`, with what there is to say of them. One that can't be added is
    /// left out. A new file never takes the name of a file `text` names that wasn't pasted, so a
    /// name it had to be given is never one the text already meant for another.
    ///
    /// A component's source and build are added only if the talk has no component of that name,
    /// and it isn't the name of one every talk has. If it has one with the same source, that
    /// one is used as it is; with different source, nothing of the pasted one is added, and
    /// that's said. A build is added only beside its source, only to a free name, and only if the
    /// talk's build index has no entry for that component or file: an entry there is never
    /// replaced. It's entered in the index, kept with the files added until a save, named for
    /// the component the app finds in that source, and recorded as built from it only if the copy
    /// says it was built from exactly those bytes, by their hash as the app works it out; if it
    /// says another, that's recorded, and the engine shows it out of date. Nothing is built, and
    /// no component's source or build is ever replaced.
    ///
    /// The hash a copy says a build was built from can't be checked without building it: a
    /// clipboard that says the right one has its module shown as up to date for the pasted
    /// source, exactly as a talk's own index does when the talk is opened. The module runs
    /// sandboxed, as any component does, and is used only if it says it is that component.
    fn bring_files(&mut self, text: &str, files: PastedFiles) -> (Vec<(String, String)>, Brought) {
        // Only what's used is spoken of: a file that came and failed its checks is missing from the
        // slide, if the slide uses it, which is said once it's pasted.
        let mut brought = Brought { too_large: files.too_large, ..Brought::default() };
        let stem_of = |path: &str| path.split_once('/').and_then(|(_, name)| name.rsplit_once('.')).map(|(stem, _)| stem.to_string());
        // A build carried without its source was the copied talk's, but can't be paired here:
        // said once pasted, if the slide shows that component missing.
        let sources: BTreeSet<String> =
            files.files.iter().filter(|file| file.kind == FileKind::ComponentSource).filter_map(|file| stem_of(&file.path)).collect();
        brought.builds_alone = files
            .files
            .iter()
            .filter(|file| file.kind == FileKind::ComponentBuild)
            .filter_map(|file| stem_of(&file.path))
            .filter(|stem| !sources.contains(stem))
            .collect();
        let Ok(pasted) = guarded(|| parse_talk(text)) else {
            brought.left_out = true;
            return (Vec::new(), brought);
        };
        let carried = CarriedFiles(&files.files);
        let used: Vec<bool> = files.files.iter().map(|file| guarded(|| talk_uses(&pasted, &carried, &file.path)).unwrap_or(false)).collect();
        let files: Vec<PastedFile> = files.files.into_iter().zip(used).filter_map(|(file, used)| used.then_some(file)).collect();
        // What `text` names that wasn't pasted, which nothing added may come to be.
        let avoid: BTreeSet<String> = added_files::written_names(&pasted)
            .into_iter()
            .filter_map(slides_engine::normalise)
            .filter(|name| !files.iter().any(|file| file.path == *name))
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let mut renames = Vec::new();
        let (components, others): (Vec<PastedFile>, Vec<PastedFile>) = files.into_iter().partition(|file| file.kind.is_component());
        for file in others {
            let name = file.path.split_once('/').map_or(file.path.as_str(), |(_, name)| name).to_string();
            match self.add_file_avoiding(file.kind, &name, file.bytes, &avoid) {
                Ok(path) if path != file.path => renames.push((file.path, path)),
                Ok(_) => {}
                Err(_) => brought.left_out = true,
            }
        }
        let (sources, mut builds): (Vec<PastedFile>, Vec<PastedFile>) =
            components.into_iter().partition(|file| file.kind == FileKind::ComponentSource);
        for source in sources {
            let Some(stem) = stem_of(&source.path) else { continue };
            let build = builds.iter().position(|build| stem_of(&build.path).as_deref() == Some(stem.as_str())).map(|at| builds.swap_remove(at));
            let Some(tag) = std::str::from_utf8(&source.bytes).ok().and_then(slides_components::component_tag) else {
                brought.left_out = true;
                continue;
            };
            if self.deck.is_built_in(&tag) {
                brought.built_in = true;
                continue;
            }
            if let Some(info) = self.components().iter().find(|info| info.tag == tag) {
                if !self.files.read(&info.file).is_some_and(|held| held == source.bytes) {
                    brought.different = Some(clipboard::different_component(&palette::label(&tag)));
                }
                continue;
            }
            let hash = source_hash(&source.bytes);
            if self.add_file(FileKind::ComponentSource, &format!("{stem}.rs"), source.bytes).is_err() {
                brought.left_out = true;
                continue;
            }
            // Without its build, it needs building, as any source does.
            let Some(build) = build else { continue };
            let build_path = format!("build/{stem}.wasm");
            let indexed = added_files::build_index(&*self.files)
                .is_some_and(|index| index.components.iter().any(|entry| entry.tag == tag || entry.file == stem));
            // A build or an entry already there is the talk's: the pasted one needs building.
            if indexed || self.files.len(&build_path).is_some() || self.on_disk(&build_path) {
                continue;
            }
            let built_from = build.built_from.clone();
            if self.add_file(FileKind::ComponentBuild, &format!("{stem}.wasm"), build.bytes).is_err() {
                brought.left_out = true;
                continue;
            }
            let source_hash = match built_from {
                Some(said) if said == hash => hash,
                Some(other) => other,
                // Built from what, nobody said: it needs building.
                None => continue,
            };
            self.added.enter_build(BuildEntry { tag, file: stem, source_hash });
            self.deck.files_added();
            self.components_looked_at();
        }
        (renames, brought)
    }

    /// The files of the talk's that `text`, a slide's own text or a thing's, uses, as
    /// [`talk_uses`] finds them, for a copy to carry: a picture or data file in `assets` a tag
    /// or picture names, and for a component of the talk's own, its source and build. A build
    /// says which source it was built from, as the talk's index records. Files are read only
    /// while there's room for them in a copy; past that, the last one read is more than a copy
    /// carries, and it carries none.
    pub fn files_for_copy(&self, text: &str) -> Vec<CopiedFile> {
        let Ok(talk) = worked_out(|| parse_talk(text)) else {
            return Vec::new();
        };
        let files = &*self.files;
        let index = added_files::build_index(files);
        let mut copied: Vec<CopiedFile> = Vec::new();
        let mut total = 0usize;
        for folder in ["assets", "components", "build"] {
            for path in files.names_in(folder) {
                if copied.len() > MAX_CLIPBOARD_FILES || total > MAX_CLIPBOARD_FILES_BYTES {
                    return copied;
                }
                let copyable = crate::clipboard::pasted_kind(&path).is_some()
                    && files.len(&path).is_some_and(|len| len <= MAX_ADDED_BYTES as u64)
                    && guarded(|| talk_uses(&talk, files, &path)).unwrap_or(false);
                let Some(bytes) = copyable.then(|| files.read(&path)).flatten() else {
                    continue;
                };
                let stem = path.strip_prefix("build/").and_then(|name| name.strip_suffix(".wasm"));
                let built_from = stem.and_then(|stem| {
                    let entry = index.as_ref()?.components.iter().find(|entry| entry.file == stem)?;
                    Some(entry.source_hash.clone())
                });
                total = total.saturating_add(bytes.len());
                copied.push(CopiedFile { path, bytes: bytes.into(), built_from });
            }
        }
        copied
    }

    /// The text of the things selected, as a copy of them gives it, in the order they're written
    /// with a blank line between, and the files they use, as [`Self::files_for_copy`] gives them.
    /// Refused with nothing selected, or only a layout's areas.
    pub fn copy_selected(&self) -> Result<(String, Vec<CopiedFile>), EditRefused> {
        let (slide, blocks) = self.selected_blocks()?.ok_or(EditRefused::CouldNotBeMade)?;
        let texts = worked_out(|| blocks.iter().map(|block| block_text(self.talk(), slide, block)).collect::<Option<Vec<String>>>())?
            .ok_or(EditRefused::CouldNotBeMade)?;
        let text = texts.join("\n\n");
        let files = self.files_for_copy(&text);
        Ok((text, files))
    }

    /// Puts the things `markdown` holds, perhaps from another talk, on the current slide, with
    /// `files`, the files they use: see [`Self::put_thing`].
    pub fn paste_element(&mut self, markdown: &str, files: PastedFiles) -> Result<(Vec<usize>, Brought), EditRefused> {
        self.put_thing(PASTE, markdown, files)
    }

    /// Puts a copy of the things selected on the slide they're on, as a paste of them would, as
    /// one step named "Duplicate", and selects the copies. The clipboard isn't touched. Refused
    /// when only a layout's areas are selected.
    pub fn duplicate_selected(&mut self) -> Result<Vec<usize>, EditRefused> {
        // Typing is finished first, which can move the things; the slide is drawn again if it did.
        let tidied = self.finish_before_edit()?;
        let Some((slide, blocks)) = self.selected_blocks()? else {
            return Ok(if tidied { vec![self.current] } else { Vec::new() });
        };
        let texts = built(self.talk(), |talk| blocks.iter().map(|block| block_text(talk, slide, block)).collect::<Option<Vec<String>>>())?
            .ok_or(EditRefused::CouldNotBeMade)?;
        self.go_to(slide);
        let (mut changed, _) = self.put_thing(DUPLICATE, &texts.join("\n\n"), PastedFiles::default())?;
        if tidied && !changed.contains(&slide) {
            changed.push(slide);
        }
        Ok(changed)
    }

    /// Puts the things `markdown` holds on the current slide, as one step named `label`, and
    /// selects them; undone, what was selected before is selected again. They must be things,
    /// as [`clipboard::some_things`] says, or it's refused, with nothing added. Typing ends
    /// first, and a new text box waiting for its first letter is dropped. `files` are added
    /// first, as [`Self::bring_files`] says, and each path in the things that had to change is
    /// made to name the file as added. Where a thing already has its corner at exactly one of
    /// their `x` and `y`, they're all moved 40 down and right together, as often as it takes.
    /// They're written with [`insert_block`], after the slide's content, in order.
    fn put_thing(&mut self, label: &'static str, markdown: &str, files: PastedFiles) -> Result<(Vec<usize>, Brought), EditRefused> {
        let things = guarded(|| clipboard::some_things(markdown))?.ok_or(EditRefused::NotOneThing)?;
        let tidied = self.finish_before_edit()?;
        // What's selected once the typing is tidied, which an undo selects again.
        let before = self.selection.clone();
        self.editing = None;
        let (text, brought) = self.bring_files_for(&things.join("\n\n"), files);
        // Each thing is a block of its own, as it was before its files were named again; if
        // naming them split or joined them, they'd name files other than those added.
        let renamed = guarded(|| clipboard::some_things(&text))?.filter(|renamed| renamed.len() == things.len());
        let things = renamed.ok_or(EditRefused::CouldNotBeMade)?;
        let texts = self.stepped_aside(&things);
        let changed = self.insert_texts(label, &texts, tidied, before)?;
        let mut brought = brought;
        brought.look_for_lone_builds(self);
        Ok((changed, brought))
    }

    /// `texts`, each one tag, moved aside from the corners of the things on the current slide,
    /// all by the same amount, as [`insert::step_aside_together`] works it out from those with a
    /// place written as numbers, so that things put together keep how they lay. Each is as it
    /// is if it has no place written as numbers, or nothing is in the way.
    fn stepped_aside(&mut self, texts: &[String]) -> Vec<String> {
        let layout = self.text_layout(self.current);
        let taken: Vec<(f32, f32)> = layout.elements().iter().map(|element| (element.rect.x, element.rect.y)).collect();
        let first_tag = |talk: &Talk| match talk.slides.first()?.blocks.first()? {
            Block::Tag(tag) => Some(tag.clone()),
            _ => None,
        };
        let place_of = |text: &str| {
            guarded(|| {
                let talk = parse_talk(text);
                let tag = first_tag(&talk)?;
                let place = |name: &str| tag.attrs.iter().find(|attr| attr.name == name)?.value.trim().parse::<f32>().ok().filter(|at| at.is_finite());
                Some((place("x")?, place("y")?))
            })
            .ok()
            .flatten()
        };
        let places: Vec<Option<(f32, f32)>> = texts.iter().map(|text| place_of(text)).collect();
        let placed: Vec<(f32, f32)> = places.iter().flatten().copied().collect();
        let step = insert::step_aside_together(&placed, &taken);
        texts
            .iter()
            .zip(places)
            .map(|(text, place)| {
                let Some((x, y)) = place.filter(|_| step != 0.0) else {
                    return text.clone();
                };
                let moved = guarded(|| {
                    let talk = parse_talk(text);
                    let tag = first_tag(&talk)?;
                    let text = set_tag_attr(&talk, 0, &tag.range, "x", Some(&insert::whole(x + step)))?.apply(text);
                    let talk = parse_talk(&text);
                    let tag = first_tag(&talk)?;
                    Some(set_tag_attr(&talk, 0, &tag.range, "y", Some(&insert::whole(y + step)))?.apply(&text))
                });
                moved.ok().flatten().unwrap_or_else(|| text.clone())
            })
            .collect()
    }
}
