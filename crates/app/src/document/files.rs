//! The files added to the talk, and bringing them back with recovered changes.

use super::*;

impl Document {
    /// Whether the talk has a file at `path`, relative to it, as a picture's is written.
    pub fn has_file(&self, path: &str) -> bool {
        self.files.len(path).is_some()
    }

    /// What lists the talk's pictures, by their paths, in name order, as its files are now:
    /// those a video's still can be. It reads the talk's folder, so it's for another thread.
    pub fn picture_files(&self) -> impl FnOnce() -> Vec<String> + Send + 'static {
        let files = self.files.clone();
        move || {
            let pictures = FileKind::Picture.extensions();
            files
                .names_in(FileKind::Picture.folder())
                .into_iter()
                .filter(|path| {
                    let extension = path.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase());
                    extension.is_some_and(|extension| pictures.contains(&extension.as_str()))
                })
                .collect()
        }
    }

    /// Adds `bytes` to the talk as a file of `kind`, named as like `suggested` as is safe, and
    /// returns its path in the talk, as the text names it. A file the talk has with the same
    /// bytes, saved or added, is that file. A name it has already is numbered, but a component's
    /// never is, as its name is the component's. Nothing is written until a save, which writes
    /// it if the text uses it then; till then it's kept with the talk's unsaved changes, and a
    /// slide that shows it shows it at once.
    pub fn add_file(&mut self, kind: FileKind, suggested: &str, bytes: Vec<u8>) -> Result<String, NotAdded> {
        self.add_file_avoiding(kind, suggested, bytes, &BTreeSet::new())
    }

    /// [`Self::add_file`], never giving a new file a path in `avoid`, in lower case.
    pub(super) fn add_file_avoiding(&mut self, kind: FileKind, suggested: &str, bytes: Vec<u8>, avoid: &BTreeSet<String>) -> Result<String, NotAdded> {
        let taken = |path: &str| self.on_disk(path) || avoid.contains(&path.to_ascii_lowercase());
        let (path, bytes) = match added_files::place(&*self.files, kind, suggested, bytes, taken)? {
            Placed::There(path) => return Ok(path),
            Placed::New(path, bytes) => (path, bytes),
        };
        let added = self.added.add(&path, Arc::from(bytes));
        debug_assert!(added, "a name that's free");
        if !added {
            return Err(NotAdded::NameTaken);
        }
        self.pending.insert(path.clone());
        // A slide may name it already, and a component is found by its files.
        if kind == FileKind::ComponentSource || kind == FileKind::ComponentBuild || talk_uses(self.talk(), &*self.files, &path) {
            self.deck.files_added();
            self.components_looked_at();
        }
        Ok(path)
    }

    /// The bytes of the file at `path`, outside the talk, to add to it with [`Self::add_file`].
    /// A folder, a link to anything but a file, and a file over the limit are refused.
    pub fn read_outside_file(path: &Path) -> Result<Vec<u8>, NotAdded> {
        added_files::read_outside(path)
    }

    /// The files added to the talk that its text uses and no save has written yet, held in
    /// memory: those but the large files, which lie on disk.
    pub fn added_files(&self) -> Vec<(String, Arc<[u8]>)> {
        let talk = self.talk();
        self.pending
            .iter()
            .filter(|path| talk_uses(talk, &*self.files, path))
            .filter_map(|path| Some((path.clone(), self.added.get(path)?)))
            .collect()
    }

    /// The files a save writes besides the text: those added to the talk that its text uses and
    /// no save has written yet, and for an untitled talk the files it was made with that its
    /// text uses. A file its text no longer uses is never written. A large file, such as a
    /// video, is given by where it lies, never its bytes.
    pub fn files_to_write(&self) -> Vec<(String, AddedFile)> {
        let talk = self.talk();
        let mut files: Vec<(String, AddedFile)> = self
            .untitled_files
            .iter()
            .filter(|(path, _)| talk_uses(talk, &*self.files, path))
            .map(|(path, bytes)| (path.clone(), AddedFile::Bytes(bytes.clone())))
            .collect();
        files.extend(
            self.pending
                .iter()
                .filter(|path| talk_uses(talk, &*self.files, path))
                .filter_map(|path| Some((path.clone(), self.added.file(path)?))),
        );
        files
    }

    /// Whether the talk holds the source of any component of its own, wherever it came from: its
    /// own files, a paste, or a copy brought back, used by its text or not.
    pub fn holds_component_source(&self) -> bool {
        !self.files.names_in(FileKind::ComponentSource.folder()).is_empty()
    }

    /// The files added to the talk that no save has written, used or not, held in memory: those
    /// but the large files, which lie on disk.
    pub fn unsaved_files(&self) -> Vec<(String, Arc<[u8]>)> {
        self.pending.iter().filter_map(|path| Some((path.clone(), self.added.get(path)?))).collect()
    }

    /// The files added to the talk that no save has written, used or not, for its working copy:
    /// held, or, for a large file, by where it lies.
    pub fn unsaved_added(&self) -> Vec<(String, AddedFile)> {
        self.pending.iter().filter_map(|path| Some((path.clone(), self.added.file(path)?))).collect()
    }

    /// Changes each time a file is added to the talk.
    pub fn files_added_stamp(&self) -> u64 {
        self.added.stamp()
    }

    /// Takes `text`, the working copy of edits that weren't saved, as [`Self::recover`] does,
    /// with `files`, the files added to the talk that were kept with it, as added again. One the
    /// talk has already is left as it is, and one that isn't a name the app gives is left out.
    ///
    /// One whose name the talk now gives a different file comes back under another name, or is
    /// the file the talk has with the same bytes, and every use of it in `text`, a picture's
    /// source or a tag's attribute, is made to name that instead before it's taken: the slides
    /// show what they showed. If one of those uses can't be rewritten exactly, or the file can't
    /// be brought back at all, `text` is taken as it is, and the window says so, once.
    pub fn recover_with_files(&mut self, text: String, files: Vec<(String, Arc<[u8]>)>) -> Result<Vec<usize>, EditRefused> {
        self.recover_with_builds(text, files, Vec::new())
    }

    /// Takes `text` and `files` back as [`Self::recover_with_files`] does, with `builds`, the
    /// builds a paste had entered in the talk's index, kept with them. Each is entered again only
    /// as a paste would enter it: its source and build are files brought back here, the tag is
    /// the one the app finds in that source, it isn't a built-in's, and the talk's index has no
    /// entry for that component or file. The hash it records is compared, by the engine, with
    /// the source's, as for any build. Any other is let go, and that component needs building.
    pub fn recover_with_builds(
        &mut self,
        text: String,
        files: Vec<(String, Arc<[u8]>)>,
        builds: Vec<BuildEntry>,
    ) -> Result<Vec<usize>, EditRefused> {
        let changed = self.recover_files_and_text(text, files)?;
        let mut entered = false;
        for entry in builds {
            let (source, build) = (format!("components/{}.rs", entry.file), format!("build/{}.wasm", entry.file));
            let hash_like = entry.source_hash.len() == 64 && entry.source_hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            let brought_back = slides_components::safe_stem(&entry.file) && self.pending.contains(&source) && self.pending.contains(&build);
            let tag = brought_back.then(|| self.added.get(&source)).flatten().and_then(|bytes| {
                std::str::from_utf8(&bytes).ok().and_then(slides_components::component_tag)
            });
            let indexed = added_files::build_index(&*self.files)
                .is_some_and(|index| index.components.iter().any(|kept| kept.tag == entry.tag || kept.file == entry.file));
            if hash_like && tag.as_deref() == Some(entry.tag.as_str()) && !self.deck.is_built_in(&entry.tag) && !indexed {
                self.added.enter_build(entry);
                entered = true;
            }
        }
        if entered {
            self.deck.files_added();
            self.components_looked_at();
        }
        Ok(changed)
    }

    fn recover_files_and_text(&mut self, text: String, files: Vec<(String, Arc<[u8]>)>) -> Result<Vec<usize>, EditRefused> {
        let (renames, mut clashed) = self.bring_back_files(files);
        let text = if renames.is_empty() {
            text
        } else {
            match guarded(|| added_files::rename_uses(&text, &renames)).ok().flatten() {
                Some(renamed) => renamed,
                None => {
                    clashed = true;
                    text
                }
            }
        };
        if clashed {
            self.message = Some(added_files::KEPT_FILE_CLASH.to_string());
        }
        self.recover(text)
    }

    /// Adds `files` back as [`Self::recover_with_files`] says, and gives the new path of each
    /// that couldn't keep its own, `(old, new)`, and whether any couldn't be brought back.
    fn bring_back_files(&mut self, files: Vec<(String, Arc<[u8]>)>) -> (Vec<(String, String)>, bool) {
        let mut renames = Vec::new();
        let mut left_out = false;
        let mut any = false;
        for (path, bytes) in files {
            let Some(kind) = added_files::kind_of_path(&path) else {
                continue;
            };
            let same = self.files.len(&path) == Some(bytes.len() as u64) && self.files.read(&path).is_some_and(|held| *held == *bytes);
            if same {
                continue;
            }
            let free = self.files.len(&path).is_none() && !self.on_disk(&path);
            let new = if free {
                path
            } else {
                let (_, name) = path.split_once('/').expect("a path from kind_of_path");
                match added_files::place(&*self.files, kind, name, bytes.to_vec(), |path| self.on_disk(path)) {
                    Ok(Placed::New(new, _)) => {
                        renames.push((path, new.clone()));
                        new
                    }
                    // The same bytes under another name already.
                    Ok(Placed::There(there)) => {
                        renames.push((path, there));
                        continue;
                    }
                    Err(_) => {
                        left_out = true;
                        continue;
                    }
                }
            };
            if self.added.add(&new, bytes) {
                self.pending.insert(new);
                any = true;
            }
        }
        if any {
            self.deck.files_added();
            self.components_looked_at();
        }
        (renames, left_out)
    }

    /// Whether anything is at `path` in a folder talk's folder, a link or a folder included,
    /// which the talk's files don't list.
    pub(super) fn on_disk(&self, path: &str) -> bool {
        match &self.place {
            TalkPlace::Folder(folder) => std::fs::symlink_metadata(folder.join(path)).is_ok(),
            TalkPlace::Packed(_) | TalkPlace::Untitled(_) => false,
        }
    }
}
