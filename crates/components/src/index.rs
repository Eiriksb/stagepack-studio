//! What a talk's `build` folder says it holds: which component each module is, and which
//! source it was built from.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Where a talk keeps its index of builds.
pub const INDEX_FILE: &str = "build/index.json";

/// The longest a component's file name can be, without its `.rs`.
const MAX_STEM: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildIndex {
    pub format: u32,
    pub components: Vec<BuildEntry>,
}

/// One build: the component `tag`, in `build/<file>.wasm`, built from a source with this hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildEntry {
    pub tag: String,
    pub file: String,
    pub source_hash: String,
}

impl BuildIndex {
    /// Enters `entry`, in place of any entry for the same component or the same file.
    pub fn enter(&mut self, entry: BuildEntry) {
        self.components.retain(|kept| kept.tag != entry.tag && kept.file != entry.file);
        self.components.push(entry);
    }
}

/// The SHA-256 of `bytes`, in lowercase hex.
pub fn source_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether a component's file name, without its `.rs`, can be used as it is in a path:
/// letters, digits, `_` and `-` only, 1 to 64 of them.
pub fn safe_stem(stem: &str) -> bool {
    (1..=MAX_STEM).contains(&stem.len()) && stem.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_is_sha256_in_lowercase_hex() {
        assert_eq!(source_hash(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_safe_stem_is_short_and_plain() {
        for stem in ["score", "Score_2", "bar-chart", &"a".repeat(64)] {
            assert!(safe_stem(stem), "{stem}");
        }
        for stem in ["", "a b", "a.b", "../a", "a/b", "é", &"a".repeat(65)] {
            assert!(!safe_stem(stem), "{stem}");
        }
    }
}
