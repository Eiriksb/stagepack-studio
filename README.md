# Stagepack Studio

A native desktop editor for Slidev `.sptx` talks. "Stagepack Studio" is a working name.

This is early work: only the bridge protocol crate (`crates/protocol`) exists so far. It is checked against the golden fixtures of the Studio add-on; set `STUDIO_SRC` (or fill in `dev.env`) to point at that checkout before running `cargo test`.

The design and the implementation plans live under `docs/superpowers/`.
