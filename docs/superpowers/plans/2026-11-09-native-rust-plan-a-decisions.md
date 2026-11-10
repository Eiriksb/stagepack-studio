# Plan A: decisions made while building

The calls made during plan A without asking the owner, and the smaller findings left for later. Each decision says what it costs if it's wrong. The owner decided separately, on 2026-11-10, that the project doesn't use GitHub Actions.

## Rulings

- **Setup.** No linked worktree; Task 1 creates branch native-rust in this checkout. The approved plan's Global Constraints name it, and a worktree would rebuild the 31 GB GPUI cache. Cost if wrong: the owner's checkout sits on the feature branch.
- **Pre-flight.** Task 8 skips Block::Tag in layout, without a notice, until Task 9 adds tags. T8's tests have no tags and T9 owns them. Cost if wrong: none.
- **Pre-flight.** Task 6 adds toolkit, tiny-skia, cosmic-text and image to crates/engine/Cargo.toml. Interfaces name them though the file list doesn't. Cost if wrong: none.
- **Pre-flight.** Task 9 writes `fn Box` in its own module and uses ::std::boxed::Box there if needed. The tag must be Box and the macro takes the function's name. Cost if wrong: a rename.
- **Pre-flight.** Task 6's $TMP is the session scratchpad. The plan doesn't define it. Cost if wrong: none.
- **Task 2.** Implementer wrote code before its RED run; it then proved all 7 tests fail against an empty split_slides. The tests are shown able to fail, though not written first. Cost if wrong: none seen.
- **Task 5.** The `column!` macro is named `col!`. Std already has `column!`, so a glob import is ambiguous and every component author would need an extra import, against "as easy as Slidev". Cost if wrong: one rename.
- **Task 8.** Render_layer sizes its pixmap to the rect plus its items' extents. Text overflowing a region was being sliced at the region's edge, and PowerPoint-style boxes show overflow. Cost if wrong: a slightly larger layer.
- **Task 8.** Render_layer never panics on absurd rects, capping each side at 16384 pixels. A value in a talk must not crash the app. Cost if wrong: very large layers are cut off.
- **Task 8.** Syntect uses default-fancy. Default features need a C compiler for Oniguruma, against "all our code is Rust" and the Windows build. Cost if wrong: slower highlighting.
- **Task 8.** Region values are clamped (x, y to -3840.7680; w, h to 0.3840; rotate to -360.360). An automated security review noted one talk line could otherwise allocate about 1 GB; Task 9 reuses the limits for tag positions. Cost if wrong: a box can't go further off-slide than two slide widths.
- **Task 8.** A <Region> marker before the first heading keeps that heading out of title. The marker explicitly "sends what follows it", and an explicit marker should beat a default. Cost if wrong: a one-line change in slide.rs.
- **Task 8.** Click in a region line is accepted and not yet used. Plan B owns click steps and adds it to LayerSpec. Cost if wrong: none now.
- **Task 8.** Text size is clamped to 1.=512 in shape_text rather than in the flow. Every caller, including Task 9's components, goes through it. Cost if wrong: a very large display size is capped.
- **Task 9.** Element::Shape gains radius. Box's required radius had nowhere to go, and Element isn't frozen until plan C. Cost if wrong: none.
- **Task 9.** Parse_talk refuses slides nested more than 64 levels with a slide error. Deep nesting overflows the stack, and a talk must not be able to crash the app. Cost if wrong: a very deeply nested slide shows an error.
- **Task 9.** The nesting limit stays a line-scan estimate, not a guarantee. It stops every crash the implementer could construct, and a guarantee needs a non-recursive parser. Cost if wrong: a crafted file could still crash the app; revisit before opening untrusted talks is a headline feature.
- **Task 9.** Replace the nesting heuristic with a 256 KiB slide cap, parsing on a 1 GiB-reserved thread, and a real depth check on the tree. Two reviews showed a line scan can't be made complete. Cost if wrong: a very large slide shows an error.
- **Task 9.** SVGs are checked against a 256 MiB budget for pattern tiles and group layers before rendering. A short SVG could otherwise take over 1 GB or abort the process. Cost if wrong: some legitimate heavy SVGs show a placeholder.
- **Task 9.** A nested tag loses x, y, w, h with a notice for now. Relative placement belongs with the editor in plan D. Cost if wrong: nested positioned tags flow instead.
- **Task 9.** Block-level <Text> styles its children. The format names <Text>, and a placeholder for it is wrong. Cost if wrong: none.
- **Task 9.** Fix round 1 goes to a fresh implementer on the same model rather than resuming. The original's context has passed 400k tokens. Cost if wrong: a little re-reading.
- **Task 9.** The slide size cap is 32 KiB, as the ruling allowed for inputs slower than 5 seconds. A run of * took 18 s at 64 KiB. Cost if wrong: an unusually long slide shows an error until the cap is raised.
- **Task 9.** 7.9 seconds on a crafted slide stands, against the "about 5 seconds" target. The target was this controller's own, it costs time only, on a hostile file, with no crash or memory growth. Cost if wrong: the window stalls for 8 seconds on a deliberately hostile slide.
- **Task 10.** A <Box> with a fill or outline gets 24 units of inner padding by default, overridable with padding. Text touched the border, and PowerPoint's boxes have insets. Cost if wrong: boxes need a padding attribute.
- **Task 10.** The blank layout stays flush at 0,0 as the plan fixes it, and the fixture uses a positioned Box there. Cost if wrong: flow text on a blank slide starts at the corner.
- **Task 10.** Arrow and line go corner to corner of their box, like a PowerPoint connector. Cost if wrong: a horizontal arrow needs h=0.
- **Task 10.** Dropped an accidentally committed report file by amending the local, unpushed commit. It was working scratch. Cost if wrong: none. The new padding test has no RED run; it asserts x and y of at least 24, which an unpadded box can't meet.
- **Task 11.** "the canvas's device width" means the letterboxed slide's width. Using the whole canvas would draw too large in a height-limited window. Cost if wrong: none.
- **Task 13.** The owner said on 2026-11-10 not to use GitHub Actions at all; the implementer was stopped, run 42773312774 cancelled before reporting, and Task 13 rewritten as local tests plus the owner's hand check on Windows. Cost if wrong: nothing has run on Windows.
- **Task 13.** The removal of the workflow is not pushed. Pushing was approved only for the check that is now cancelled; origin/native-rust stays at bf0a075 with the workflow, which only fires on a push to that branch, and the next push carries the removal so triggers nothing. Cost if wrong: a dormant file on origin.
- **Final.** Pictures are re-validated by length and modified time on every update, and a changed one reports every slide as changed. Simple and rare. Cost if wrong: one extra redraw of the deck when a picture changes.
- **Final.** Drawn layers are bounded at 512 MiB, evicting whole slides least recently used, with one shared background and a 64-million-pixel cap per layer. Stepping through a talk on a 4K screen reached gigabytes. Cost if wrong: revisiting an evicted slide redraws it.
- **Final.** Attribute values escape & and the quote in use, and the scanner decodes them. Write and re-parse must round-trip for the editor. Cost if wrong: none.
- **Final.** A list's indent grows past the plan's fixed 48 when its widest marker needs it — "10." overlapped its text. Cost if wrong: long lists indent slightly more.
- **Final.** On the reviewer's declined list: a bare < in prose such as 5 <= 10 being a slide error stands, as the format uses MDX-style tags; the message wording stays a deferred minor. Cost if wrong: coders must escape <.
- **Final.** Declined items stand as deferred: the 32 KiB slide cap and marker scan, hostile SVG time, FIFOs in a talk folder, assets/ not watched, click and enter unused until plans B and D, blurry emoji at 4K, <Text bold="false"> in a bold block. None affects an ordinary talk, or a later plan owns it. Cost if wrong: the listed behaviour remains.

## Deferred findings

- **Task 1.** Cargo.toml workspace.dependencies lists serde, serde_json, thiserror with no user yet.
- **Task 1.** WorkspaceView::new added beyond the listed interface, because main needs a constructor.
- **Task 2.** Split.rs fence handling is looser than CommonMark: a longer opener is closed by any three-character run, and indentation of any depth counts.
- **Task 2.** Open.to_string().repeat(3) allocates per line inside a fence; implicit-first-slide check is repeated.
- **Task 2.** No tests for the unclosed-settings fallback, empty input, or content before the first break.
- **Task 3.** A nested or duplicate <Notes>, and a nameless fragment, are dropped silently; unmodelled nodes such as thematic breaks are dropped, and blockquotes flatten.
- **Task 3.** An unquoted attribute value keeps a trailing `/`; size="64px" silently gives None; Attr doc comment is inaccurate.
- **Task 3.** No tests for Tag::open_end, Notes::range, bare or single-quoted attributes, Break, links, inline code, ordered lists or code blocks.
- **Task 4.** A new value containing ' written into a single-quoted attribute breaks the markup; set_setting doesn't guard against a newline in a value.
- **Task 4.** No tests for a BOM, a bare rule at end of file, a single-quoted attribute, or appending after a bare last attribute.
- **Task 5.** Color::parse accepts a leading + inside hex; colour names are case-sensitive.
- **Task 5.** Macro compile errors have no span; a path like toolkit::Children isn't recognised; Option<T> reports "Option" in the schema.
- **Task 5.** No tests for bool, f32 or Option props, or the size, outline and gap builders.
- **Task 6.** Underline thickness uses the buffer's largest size, with unnamed constants; physical.y rounds the baseline in slide units; image dependency unused until Task 7.
- **Task 6.** Colour emoji bitmaps are rasterised at slide size and blur when zoomed in.
- **Task 6.** No tests for load_talk_font, Align::Center or Right, or the system-fallback notice; NotoColorEmoji-OFL.txt carries an editorial parenthetical.
- **Task 7.** Render.rs adds 1 pixel to every layer's width and height; expect panics on NaN or huge rects.
- **Task 7.** Golden images compare encoded PNG bytes and only cover Fill and rotation; the golden-actual path ignores CARGO_TARGET_DIR.
- **Task 8.** No test covers body heading sizes 64/48/40; duplicate Region notices aren't de-duplicated.
- **Task 8.** Plan-mandated indent of 48 lets two-digit list markers overlap their text by up to 12 units.
- **Task 9.** Icon aliases show as unknown icons; a block-level <Text> becomes a placeholder; SVG <text> isn't drawn, and small SVGs blur when stretched.
- **Task 9.** Size="inf" on an icon draws at 1 unit; failed pictures are decoded twice per layout; a natural size over 3840 is clamped per side and distorts; a 1000-deep SVG overflows a 2 MiB debug stack only.
- **Task 9.** An SVG 1000 groups deep overflows a 2 MiB stack in debug only; a hostile SVG within budget can still take minutes; many hostile slides add up, since the 5-second limit is per slide; parse_talk blocks its caller.
- **Task 9.** An inline <Text bold="false"> inside a block-level <Text bold> stays bold; the SVG filter budget sums groups as if held at once and can refuse some heavy legitimate SVGs.
- **Task 10.** The reuse key ignores a picture file's contents, so a changed picture needs its slide's text to change; no tests for notices, source, or fewer slides after an update; photo.png is 141 KB.
- **Task 10.** Elements.rs computes the limited padding twice.
- **Task 11.** Slide_canvas discards paint_image errors; a refused image grows the atlas on each repaint and vanishes with no notice.
- **Task 11.** The notices line changes the canvas height, so a height-limited window jumps between slides with and without notices.
- **Task 11.** No test distinguishes letterboxed width from canvas width, or covers the stretch factor; elements.rs has an unused import, zero.
- **Task 12.** Watcher.rs drops Err and rescan events, so an inotify overflow misses one update until the next save; a save between open and the watcher starting is missed until the next save.
- **Task 12.** No automated test covers the open-to-follow_saves wiring or the "contains the current slide" condition; panel lines over about 50 characters are cut off with no horizontal scroll.
- **Task 13.** Plan-a.md doesn't say whether doc tests are in the total.
- **Final.** Font set to a family that isn't loaded silently uses Inter; slide-error placeholders show the parser's own wording; a non-UTF-8 save is ignored with no notice; a partial write that drops slides loses your place.
- **Final.** Laid specs' source ranges go stale for unchanged slides; text is shaped twice per layout; Option<T> schema name; Registry uses static names; thiserror unused; Deck holds a non-Send TalkFiles, which plan B needs if layout moves off the main thread.
- **Final.** A picture that fails to read isn't cached, so after it's repaired it only appears when its slide's text changes or the app restarts; a same-length replacement with the same modified time reads as unchanged.
