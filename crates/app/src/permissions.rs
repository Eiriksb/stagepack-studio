//! Which folders the user has said can always be built. The app keeps this itself, in its own
//! data folder, never in a talk: a talk is untrusted, and can't give itself permission.

use std::collections::BTreeSet;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use slides_engine::write_atomically;

/// The file's name in the app's data folder.
const FILE_NAME: &str = "build-permissions.json";
/// The app's own folder in the user's data folder.
const APP_FOLDER: &str = "stagepack-studio";

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    always_build: BTreeSet<PathBuf>,
}

/// The folders whose components are built whenever they change.
pub struct BuildPermissions {
    file: PathBuf,
    /// Each folder by its canonical path, so however it's written it's the same folder.
    always_build: BTreeSet<PathBuf>,
}

impl BuildPermissions {
    /// The permissions kept in `file`. A file that is missing or can't be read counts as no
    /// permissions at all.
    pub fn load(file: PathBuf) -> BuildPermissions {
        BuildPermissions {
            always_build: read(&file).always_build,
            file,
        }
    }

    /// Where the app keeps its permissions: in its own data folder, if the system has one.
    pub fn default_file() -> Option<PathBuf> {
        Some(BuildPermissions::app_folder()?.join(FILE_NAME))
    }

    /// The app's own data folder, if the system has one.
    pub fn app_folder() -> Option<PathBuf> {
        Some(dirs::data_dir()?.join(APP_FOLDER))
    }

    /// Whether the components in `folder` are built whenever they change.
    pub fn always(&self, folder: &Path) -> bool {
        folder
            .canonicalize()
            .is_ok_and(|folder| self.always_build.contains(&folder))
    }

    /// Says from now on that the components in `folder` are built whenever they change, and
    /// writes that down.
    pub fn set_always(&mut self, folder: &Path) -> io::Result<()> {
        let folder = folder.canonicalize()?;
        // Another window may have added a folder since this one read the file.
        let mut stored = read(&self.file);
        stored
            .always_build
            .extend(self.always_build.iter().cloned());
        stored.always_build.insert(folder);
        let json = serde_json::to_vec_pretty(&stored).map_err(io::Error::other)?;
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomically(&self.file, |file| file.write_all(&json))?;
        self.always_build = stored.always_build;
        Ok(())
    }
}

fn read(file: &Path) -> Stored {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
