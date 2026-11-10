# Native Rust spikes: decisions made while building

The record of calls made during the spikes without asking the owner. Each says what it costs if it's wrong.

## Rulings

- **No linked worktree.** Worked on `spike/native-rust` in the main checkout, as the plan says, because a worktree would rebuild a 31 GB cache. Cost if wrong: the owner's checkout stays on the spike branch until they switch back.
- **Layers as separate GPUI images.** The plan's one-bitmap-per-frame approach missed every frame-time limit, so `--draw sprites` was added and meets them all. Cost if wrong: real 4K screens or dragging to rotate could still need the wgpu compositor.
- **`to_bgra_frame` demultiplies.** GPUI's shader multiplies by alpha, so premultiplied pixels gave dark edges. Cost if wrong: none seen.
- **Updated the design's Drawing section** to say layers are separate images, and corrected its claim about opacity. Cost if wrong: one paragraph.
- **The bundle also carries the wasm target's `.rmeta` files** (77 MB). Rust 1.98 keeps the standard library's metadata there. Cost if wrong: size only.
- **The bundle also carries `libserde_derive`.** The `serde` library refers to it. Cost if wrong: 13 MB, since three variants are copied.
- **Linking happens in a private scratch folder.** LLD reads `%` in its output path as a placeholder. Cost if wrong: none.
- **Wasmtime gets its `anyhow` feature,** so its errors convert as the plan's interface needs. Cost if wrong: none.
- **Added a scripted run with real key events on a nested display,** beyond the plan, so the owner's check starts from something proven. Cost if wrong: none.
- **The spike branch isn't merged or put through the usual finish step,** because the plan says it's never merged. Cost if wrong: the branch is still there.
- **The reviewer's "ledger records the wrong line" point was downgraded to minor.** It affects only a working file, and the reviewer confirmed all 22 tests pass. Cost if wrong: none.
- **The single-line paragraph limit was upgraded and written into the results,** because "only the edited paragraph changes" overclaimed without it. Cost if wrong: none.
- **Moved the memory measure to GPU memory** after a deliberate-leak control showed process memory can't see the leak. Cost if wrong: none.
- **Plan A brings `docs/` over from the spike branch first,** added to the roadmap. Cost if wrong: one extra step.

## Set aside by the reviewer, and why that stands

All of these belong to throwaway code or to later plans, so none changes a spike's answer.

- The source check for `include!`, `env!`, `#[path]` and `mod x;`, and Wasmtime's memory and time limits: product requirements for plan C. Cost if wrong: plan C misses them, but the design states them.
- A bundle on other Linux distributions or glibc versions: the clean check reused this machine's `/usr`. Cost if wrong: found at packaging, in stage E.
- `text_for_range` can panic on an out-of-range input-method query, and `character_index_for_point` assumes the canvas fills the window: throwaway UI. Cost if wrong: the spike window crashes.
- Typing straight after a bold word continues bold: editor policy for plan D. Cost if wrong: a behaviour change later.
- `escape` doesn't cover `|`, `!` or setext underlines: GFM is off and paragraphs are single-line, so they can't trigger. Cost if wrong: plan D revisits it with multi-line paragraphs.
- Stale library variants copied into the bundle: they only make the 465 MB figure conservative. Cost if wrong: none.
- `env_clear()` on Windows dropping `PATH`: GitHub's Windows build ran fine, and the owner's check covers the clean-machine case. Cost if wrong: the owner's check fails.
- The owner's uncommitted edit to `2026-11-09-roadmap.md`: deliberately left alone. Cost if wrong: none.

## Deferred items

- The progress ledger recorded the doc-test summary line rather than the real test counts.
- `report_tag_fallback_rate` shares global counters with other tests, so its figure is only right when run alone. The results file now says so.
