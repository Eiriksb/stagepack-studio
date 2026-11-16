//! Every icon the app's tools name is one the app carries. The icon set holds only the icons
//! listed for it, and a tool whose icon isn't there shows nothing at all, or its word alone,
//! which the headless tests can't see.

use std::fs;
use std::path::Path;

use gpui_kit::AssetSource;
use gpui_kit::assets::IconName;
use stagepack_studio::app_assets::AppAssets;

/// The icon names the app's sources use, as `IconName::Bold` or `Icons::Bold`.
fn icons_named_in(folder: &Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(folder).expect("the app's sources") {
        let path = entry.expect("a source file").path();
        if path.is_dir() {
            icons_named_in(&path, found);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("a readable source file");
        for prefix in ["IconName::", "Icons::"] {
            for (at, _) in text.match_indices(prefix) {
                let name: String = text[at + prefix.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    found.push((name, path.display().to_string()));
                }
            }
        }
    }
}

#[test]
fn every_icon_the_tools_name_is_carried() {
    let mut named = Vec::new();
    icons_named_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut named);
    assert!(named.len() > 20, "the tools' icons were found: {named:?}");
    let missing: Vec<_> = named
        .iter()
        .filter(|(name, _)| {
            let icon = IconName::ALL
                .iter()
                .find(|icon| format!("{icon:?}") == *name)
                .unwrap_or_else(|| panic!("{name} is an icon"));
            !matches!(AppAssets.load(&icon.path()), Ok(Some(_)))
        })
        .collect();
    assert!(missing.is_empty(), "icons the app names but doesn't carry: {missing:?}");
}
