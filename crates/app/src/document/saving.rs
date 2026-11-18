//! Saving the talk, and reading its file again when it changes on disk.

use super::*;

/// What came of reading the talk's file again.
pub enum Reload {
    /// The talk took the new text, or a picture it shows was replaced. These are the
    /// slides to show again.
    Changed(Vec<usize>),
    /// The file says what it said before, and its pictures are as they were.
    Unchanged,
    /// The read gave nothing to use, so the talk stays as it was.
    KeptLastGood,
    /// The file changed while the talk has edits it doesn't, so nothing was taken: whoever is
    /// editing it is asked which to keep. [`Document::outside_change`] is what the file holds.
    Conflict,
    /// Reading what the file holds went wrong, so the talk stays as it was.
    Unreadable,
}

/// What the window says when a new read of the talk's file can't be taken in.
pub const UNREADABLE_FILE: &str =
    "The talk's file changed, but it couldn't be read, so the talk is shown as it was.";

impl Document {
    /// Whether this talk came from a packed file, rather than a folder.
    pub fn is_packed(&self) -> bool {
        matches!(self.place, TalkPlace::Packed(_))
    }

    pub fn place(&self) -> &TalkPlace {
        &self.place
    }

    /// Whether this talk has no place yet: a new presentation, not saved.
    pub fn is_untitled(&self) -> bool {
        matches!(self.place, TalkPlace::Untitled(_))
    }

    /// Numbers an untitled talk among those open: from 2 up, it's called *Untitled presentation
    /// 2* and so on, and 1 or less is the name alone.
    pub fn set_untitled_number(&mut self, number: usize) {
        if !self.is_untitled() {
            return;
        }
        self.place_name = match number {
            0 | 1 => self.place.name(),
            number => format!("{} {number}", self.place.name()),
        };
    }

    /// The number of an untitled talk among those open, as [`Self::set_untitled_number`] gave
    /// it, 1 for one called by the name alone. A talk with a place has none.
    pub fn untitled_number(&self) -> Option<usize> {
        if !self.is_untitled() {
            return None;
        }
        let name = self.place.name();
        match self.place_name.strip_prefix(name.as_str()) {
            Some("") => Some(1),
            Some(rest) => rest.strip_prefix(' ')?.parse().ok(),
            None => None,
        }
    }

    /// A save of the talk as it is now, where it's kept: for a folder, only its `slides.md` and
    /// the files added that it uses. An untitled talk is kept nowhere, so its job fails.
    pub fn save_job(&self) -> SaveJob {
        SaveJob::new(
            self.source().to_string(),
            self.on_disk.clone(),
            self.place.clone(),
            None,
            self.files_to_write(),
            self.index_for_save(),
        )
    }

    /// A save of the talk as it is now, somewhere new. For an untitled talk that's its first
    /// save, which writes it all: the text, and every file it uses.
    pub fn save_as_job(&self, target: SaveTarget) -> SaveJob {
        SaveJob::new(
            self.source().to_string(),
            self.on_disk.clone(),
            self.place.clone(),
            Some(target),
            self.files_to_write(),
            self.index_for_save(),
        )
    }

    /// The builds entered in the talk's index with the files added to it, of builds its text
    /// uses, for a save to enter in the index it writes.
    pub fn builds_to_enter(&self) -> Vec<BuildEntry> {
        let talk = self.talk();
        self.added
            .builds_entered()
            .into_iter()
            .filter(|entry| talk_uses(talk, &*self.files, &format!("build/{}.wasm", entry.file)))
            .collect()
    }

    /// The builds a save enters in the talk's index, and the index with them entered in it, as a
    /// save anywhere but the talk's own folder writes it. It's the talk's own index as it's kept,
    /// every entry as it is, with an entry added for each component pasted whose source and build
    /// this save writes, as [`Self::builds_to_enter`] gives them, but for one the index has an
    /// entry for the same component or file: that's the talk's, and never replaced. An entry
    /// whose files this save doesn't write stays to be entered. `None` for the index if the
    /// talk's own can't be read, which a save leaves as it is.
    fn index_for_save(&self) -> (Vec<BuildEntry>, Option<Vec<u8>>) {
        let writing: BTreeSet<String> = self.files_to_write().into_iter().map(|(path, _)| path).collect();
        let own = &*self.own_files;
        let index = match (&self.index_kept_in_pack, own.len(INDEX_FILE)) {
            // What the last save wrote into a pack it couldn't read back: the pack's index now.
            (Some(kept), _) => serde_json::from_slice::<BuildIndex>(kept).ok(),
            (None, None) if own.read(INDEX_FILE).is_none() => Some(BuildIndex { format: 1, components: Vec::new() }),
            _ => added_files::build_index(own),
        };
        let builds: Vec<BuildEntry> = self
            .builds_to_enter()
            .into_iter()
            .filter(|entry| writing.contains(&format!("build/{}.wasm", entry.file)) && writing.contains(&format!("components/{}.rs", entry.file)))
            .filter(|entry| index.as_ref().is_none_or(|index| !index.components.iter().any(|kept| kept.tag == entry.tag || kept.file == entry.file)))
            .collect();
        if builds.is_empty() {
            return (builds, None);
        }
        let index = index.and_then(|mut index| {
            index.components.extend(builds.iter().cloned());
            index.format = index.format.max(1);
            serde_json::to_vec_pretty(&index).ok()
        });
        (builds, index)
    }

    /// Takes in a save that worked. The talk is saved if its text is still what was written,
    /// and after a Save As it's kept in the new place from now on.
    pub fn saved(&mut self, saved: Saved) {
        // Part of the talk now, wherever it's kept.
        for path in &saved.files_written {
            self.pending.remove(path);
        }
        // In the index where the talk is kept now, which is read as it is from here on.
        self.added.forget_builds(&saved.builds_written);
        // What the talk is read from now gives its index, unless the pack saved in was not read
        // back, when the next save starts from what was written in it.
        if saved.reopened.is_some() || saved.moved_to.is_some() {
            self.index_kept_in_pack = None;
        }
        if let Some(index) = saved.index_kept_in_pack {
            self.index_kept_in_pack = Some(index);
        }
        if let Some(files) = saved.reopened {
            self.own_files = files.clone();
            self.files = Arc::new(WithAdded::new(files, self.added.clone()));
            self.deck.set_files(Box::new(self.files.clone()));
            self.components_looked_at();
        }
        if let Some((place, files)) = saved.moved_to {
            self.place_name = place.name();
            self.place = place;
            // Those written are read from where they were written from now on. One the text
            // didn't use at this save wasn't written, and is kept as a file added, as Undo may
            // bring back the text that uses it, and the save after that writes it.
            for (path, bytes) in std::mem::take(&mut self.untitled_files) {
                if !saved.files_written.contains(&path) && self.added.add(&path, bytes) {
                    self.pending.insert(path);
                }
            }
            self.own_files = files.clone();
            self.files = Arc::new(WithAdded::new(files, self.added.clone()));
            self.deck.set_files(Box::new(self.files.clone()));
            self.components_looked_at();
        }
        // Read from where the talk is kept from here on, as its own files are, so that a change
        // to one there is seen as a change to any other is. One that isn't there to be read, as
        // in a pack that wasn't opened again, stays in memory.
        let written: Vec<String> = saved.files_written.iter().filter(|path| self.own_files.len(path).is_some()).cloned().collect();
        if self.added.release(&written) {
            self.deck.files_added();
        }
        if self.source() == saved.written {
            self.history.mark_saved();
        } else {
            // Edited while it saved: no step leads to what the file holds now.
            self.history.forget_saved();
        }
        self.on_disk = saved.written;
        self.outside = None;
    }

    /// What the talk's file held when the app last wrote or read it.
    pub fn stamp_on_disk(&self) -> &str {
        &self.on_disk
    }

    /// What the file holds, when it changed under unsaved edits and nobody has said yet whether
    /// to keep those or load it.
    pub fn outside_change(&self) -> Option<&str> {
        self.outside.as_deref()
    }

    /// Loads `text`, what the file holds after a change from outside, in place of the edits
    /// made here. It's one step, which brings the edits back when it's undone. If reading it
    /// goes wrong, it's refused, and the talk and the question about the change stay as they were.
    pub fn take_theirs(&mut self, text: String) -> Result<(), EditRefused> {
        if text.is_empty() {
            self.outside = None;
            return Ok(());
        }
        self.replace_all(LOAD_CHANGED_FILE, text.clone())?;
        self.outside = None;
        self.on_disk = text;
        // What the file holds is what's here now.
        self.history.mark_saved();
        Ok(())
    }

    /// Keeps the edits made here over the file's change, which isn't asked about again. The
    /// file holds neither, so the talk has unsaved changes until it's saved.
    pub fn keep_mine(&mut self) {
        if let Some(theirs) = self.outside.take() {
            self.on_disk = theirs;
            self.history.forget_saved();
        }
    }

    /// What an export needs to know about the talk as it is now.
    pub fn export_input(&self) -> ExportInput {
        ExportInput {
            source: self.source().to_string(),
            files: self.files.clone(),
            talk_fonts: self.talk_fonts.clone(),
        }
    }

    /// Takes a new read of the talk's file: its text, or `None` if it couldn't be read. An
    /// empty read is an editor caught halfway through a save, not an empty talk.
    ///
    /// A file that holds what the app last wrote or read brings nothing new. A change made
    /// while the talk has unsaved edits isn't taken, but asked about. If reading what it holds
    /// goes wrong, the talk stays as it was.
    pub fn reload(&mut self, contents: Option<String>) -> Reload {
        let Some(source) = contents.filter(|source| !source.is_empty()) else {
            return Reload::KeptLastGood;
        };
        let holds_mine = source == self.source();
        if holds_mine || source == self.on_disk {
            let Ok(talk) = read_after(self.talk(), self.source().to_string()) else {
                return Reload::Unreadable;
            };
            return self.take_same(source, holds_mine, talk);
        }
        if self.has_unsaved_changes() {
            self.outside = Some(source);
            return Reload::Conflict;
        }
        let Ok(talk) = read_after(self.talk(), source.clone()) else {
            return Reload::Unreadable;
        };
        self.take(source, talk)
    }

    /// Takes a read of the file that holds what's here, `holds_mine`, or what the app last
    /// wrote or read. `talk` is what's here, read again.
    fn take_same(&mut self, source: String, holds_mine: bool, talk: Talk) -> Reload {
        if holds_mine {
            // The file holds what's here, however it came to: nothing is unsaved.
            self.history.mark_saved();
        }
        self.on_disk = source;
        // The app's own save, or changed back: a picture the talk shows may still have been
        // replaced.
        self.outside = None;
        let changed = self.deck.update_parsed(talk);
        if changed.is_empty() {
            Reload::Unchanged
        } else {
            Reload::Changed(changed)
        }
    }

    /// Takes `source`, what the file holds, as the talk's text. `talk` is what it reads as.
    fn take(&mut self, source: String, talk: Talk) -> Reload {
        self.on_disk = source.clone();
        let same_text = source == self.source();
        let old = (!same_text).then(|| self.source().to_string());
        // Even the same text goes through: a picture it shows may have been replaced.
        let mut changed = self.deck.update_parsed(talk);
        if let Some(old) = old {
            self.follow(&one_edit(&old, self.deck.source()));
        }
        if !same_text {
            // Its steps are ranges of the text it had, which no longer fit.
            self.history = History::new();
            self.typing_notes = None;
            // Nor does the text being typed: what was typed is in the talk, or the file.
            self.editing = None;
            self.held_color = None;
        }
        if same_text && changed.is_empty() {
            return Reload::Unchanged;
        }
        let last = self.slide_count() - 1;
        if self.current > last {
            self.current = last;
            // Its text may be as it was, but this is now the slide to show.
            if !changed.contains(&last) {
                changed.push(last);
            }
        }
        Reload::Changed(changed)
    }
}
