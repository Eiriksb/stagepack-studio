//! The key an add-on package must be signed with: the public half of the owner's signing key,
//! made with `package addon-keygen`. The secret half is never in the repository.

// A shipped build checks every package against the owner's key, and nothing else.
#[cfg(all(feature = "shipped", feature = "test-hooks"))]
compile_error!("the add-on key's test hook must never be in a shipped build");

/// The ed25519 public key every add-on package is checked against.
pub const ADDON_PUBLIC_KEY: [u8; 32] = [76, 134, 209, 167, 8, 197, 62, 215, 99, 249, 1, 186, 79, 6, 200, 46, 96, 236, 187, 53, 222, 116, 147, 178, 148, 73, 54, 200, 35, 110, 254, 40];

/// The key a test put in place of [`ADDON_PUBLIC_KEY`], for the whole process. A release build
/// has no way to change the key.
#[cfg(feature = "test-hooks")]
static TEST_KEY: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);

/// The key add-on packages are checked against: [`ADDON_PUBLIC_KEY`], unless a test put
/// another in its place.
pub fn addon_key() -> [u8; 32] {
    #[cfg(feature = "test-hooks")]
    if let Some(key) = *TEST_KEY.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
        return key;
    }
    ADDON_PUBLIC_KEY
}

/// Puts `key` in place of [`ADDON_PUBLIC_KEY`] for every thread of this process, so that a
/// test can install packages it signed with a key of its own.
#[cfg(feature = "test-hooks")]
pub fn set_addon_key_for_test(key: [u8; 32]) {
    *TEST_KEY.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(key);
}
