use std::path::PathBuf;

/// Compares `pixmap` with `tests/golden/<name>.png`. `UPDATE_GOLDEN=1` rewrites the
/// file; a mismatch writes `target/golden-actual/<name>.png` and panics with its path.
pub fn assert_golden(name: &str, pixmap: &tiny_skia::Pixmap) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let golden = root.join("tests/golden").join(format!("{name}.png"));
    let actual = pixmap.encode_png().expect("encode png");
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &actual).unwrap();
        return;
    }
    let expected = std::fs::read(&golden)
        .unwrap_or_else(|_| panic!("missing golden {}; run with UPDATE_GOLDEN=1", golden.display()));
    if expected != actual {
        let out = root.join("../../target/golden-actual");
        std::fs::create_dir_all(&out).unwrap();
        let out = out.join(format!("{name}.png"));
        std::fs::write(&out, &actual).unwrap();
        panic!("golden {name} differs; actual written to {}", out.display());
    }
}
