//! The one list of layouts.

use slides_format::{DEFAULT_LAYOUT, LAYOUT_NAMES, layout_names};

#[test]
fn the_default_layout_is_listed() {
    assert_eq!(layout_names(DEFAULT_LAYOUT).map(|layout| layout.name), Some(DEFAULT_LAYOUT));
    assert!(layout_names("nonsense").is_none());
}

#[test]
fn every_main_region_is_one_of_its_regions() {
    for layout in &LAYOUT_NAMES {
        assert!(layout.regions.contains(&layout.main), "{} has no region {}", layout.name, layout.main);
    }
}

#[test]
fn names_are_distinct() {
    for (at, layout) in LAYOUT_NAMES.iter().enumerate() {
        assert!(LAYOUT_NAMES[..at].iter().all(|other| other.name != layout.name), "{} is listed twice", layout.name);
        for (index, region) in layout.regions.iter().enumerate() {
            assert!(!layout.regions[..index].contains(region), "{} lists {region} twice", layout.name);
        }
    }
}
