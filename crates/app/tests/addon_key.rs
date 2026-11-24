//! The add-on key: the owner's, until a test puts its own in place. One test, as the key a
//! test puts in place is the whole process's.

use stagepack_studio::addons::key::{addon_key, set_addon_key_for_test, ADDON_PUBLIC_KEY};

#[test]
fn the_key_is_the_owners_until_a_test_sets_its_own() {
    assert_eq!(addon_key(), ADDON_PUBLIC_KEY);
    set_addon_key_for_test([9; 32]);
    assert_eq!(addon_key(), [9; 32]);
}
