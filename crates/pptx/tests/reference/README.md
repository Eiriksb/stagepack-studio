# Reference output from the JavaScript writer

Every `.xml` file here (and `package-entries.txt`) is the output of the original exporter,
`slidev-pptx-export` (`src/ooxml/*.mjs`), for the call listed below. `tests/parts.rs` asserts that the
Rust port gives the same bytes. They were written by a throwaway Node script that is not kept in the
repo; regenerate by making the same calls.

Shared inputs:

- `geometry` = `slideGeometry({ width: 13.333, height: 7.5, marginX: 0.6 }, null)` from `src/geometry.mjs`
- `exact` = `slideGeometry({ width: 1920 / 144, height: 1080 / 144, marginX: 0.6 }, null)` (exactly 12192000 x 6858000 EMU)
- `colors` = `themeColors({ ...defaultConfig.colors, text: "#1e293b", accent: "#2563eb" })` from `src/colors.mjs`, i.e.
  `dk1` 1E293B, `lt1` FFFFFF, `dk2` 1E3A8A, `lt2` F1F5F9, `accent1` 2563EB, `accent2` 7C3AED, `accent3` 6B7280,
  `accent4` E2E8F0, `accent5` 9CA3AF, `accent6` F59E0B, `hlink` 2563EB, `folHlink` 1E3A8A.
  These match `toolkit::Theme::default()` (background `#ffffff` is `lt1`, text `#1e293b` is `dk1`, accent `#2563eb` is `accent1`).
- `fonts` = `{ heading: "Inter", body: "Inter", mono: "JetBrains Mono" }`
- `ctx` = `{ lang: "en-US", fonts }`
- `layoutIds` = `[{ id: 2147483649, rId: "rId2" }, { id: 2147483650, rId: "rId3" }]`

| File | Call |
| --- | --- |
| `color-rgb.xml` | `colorXml("#2563eb")` |
| `color-rgb-alpha.xml` | `colorXml("#2563EB", 0.5)` |
| `color-slot-dk1.xml` | `colorXml("dk1")` |
| `color-slot-lt2-alpha.xml` | `colorXml("lt2", 0.16)` |
| `color-slot-accent3.xml` | `colorXml("accent3")` |
| `color-opaque-alpha.xml` | `colorXml("#000000", 1)` |
| `solid-fill.xml` | `solidFill("#1e293b", 0.25)` |
| `solid-fill-slot.xml` | `solidFill("accent1")` |
| `theme.xml` | `themeXml({ name: "Deck", colors, fonts })` |
| `theme-escaped-name.xml` | `themeXml({ name: 'A & B <"c">', colors, fonts })` |
| `layout-title-only.xml` | `layoutXml(layoutDefs(geometry).find(d => d.key === "titleOnly"), ctx)` |
| `layout-blank.xml` | `layoutXml(layoutDefs(geometry).find(d => d.key === "blank"), ctx)` |
| `master.xml` | `masterXml({ geometry, ctx, layoutIds })` |
| `master-exact.xml` | `masterXml({ geometry: exact, ctx, layoutIds })` |
| `presentation.xml` | `presentationXml({ masterId: 2147483648, masterRId: "rId1", notesMasterRId: null, slides: [], width: 13.333, height: 7.5 })` |
| `presentation-slides-notes.xml` | as above with `notesMasterRId: "rId2"` and `slides: [{ id: 256, rId: "rId3" }, { id: 257, rId: "rId4" }]` |
| `presentation-exact.xml` | `slides: [{ id: 256, rId: "rId2" }]`, `width: 1920 / 144`, `height: 1080 / 144` |
| `presentation-clamped.xml` | `width: 0.2`, `height: 100` (PowerPoint's 1" to 56" limits) |
| `presentation-fonts.xml` | `slides: [{ id: 256, rId: "rId2" }]`, 13.333 x 7.5, `embeddedFonts: [{ typeface: "Inter", regular: "rId5", bold: "rId6", italic: "rId7", boldItalic: "rId8" }, { typeface: 'Odd & "Font"', regular: "rId9" }]` |
| `pres-props.xml` | `presPropsXml()` |
| `view-props.xml` | `viewPropsXml()` |
| `app.xml` | `appXml({ slides: 12, notes: 3 })` |
| `core.xml` | `coreXml({ title: "My talk", author: "Ada", date: null })` |
| `core-dated.xml` | `coreXml({ title: "My talk", author: "Ada", date: new Date(Date.UTC(2026, 10, 10, 9, 8, 7, 654)) })` |
| `core-escaped.xml` | `coreXml({ title: "A & B <c>", author: "\"Q\" 'x'", date: null })` |

The `package-*` files come from one `Package`, built by these calls in order, then read back out of the zip it writes:

```js
pkg.rel("", PRES, REL.officeDocument);               // PRES   = /ppt/presentation.xml
pkg.rel("", "/docProps/core.xml", REL.coreProps);
pkg.addXml(THEME, CT.theme, "<t/>");                  // THEME  = /ppt/theme/theme1.xml
pkg.addXml(MASTER, CT.slideMaster, "<m/>");           // MASTER = /ppt/slideMasters/slideMaster1.xml
pkg.rel(MASTER, THEME, REL.theme);
pkg.addXml(SLIDE, CT.slide, "<s/>");                  // SLIDE  = /ppt/slides/slide1.xml
pkg.addMedia("/ppt/media/image1.PNG", Buffer.from([1, 2, 3]));
pkg.addMedia("/ppt/media/image2.png", Buffer.from([4]));
pkg.addMedia("/ppt/fonts/font1.fntdata", Buffer.from([5]));
pkg.rel(SLIDE, "/ppt/media/image1.PNG", REL.image);
pkg.rel(SLIDE, "/ppt/media/image1.PNG", REL.image);   // reused, same rId
pkg.rel(SLIDE, MASTER, REL.slideMaster);
pkg.rel(SLIDE, "https://example.com/?a=1&b=2", REL.hyperlink, { external: true });
pkg.rel(PRES, MASTER, REL.slideMaster);
pkg.addXml(PRES, CT.presentation, "<p/>");
pkg.addXml("/docProps/core.xml", CT.coreProps, "<c/>");
```

| File | Content |
| --- | --- |
| `package-entries.txt` | the zip's entry names, in order |
| `package-content-types.xml` | `[Content_Types].xml` |
| `package-root-rels.xml` | `_rels/.rels` |
| `package-presentation-rels.xml` | `ppt/_rels/presentation.xml.rels` |
| `package-master-rels.xml` | `ppt/slideMasters/_rels/slideMaster1.xml.rels` |
| `package-slide-rels.xml` | `ppt/slides/_rels/slide1.xml.rels` |

## Text, shapes, tables and pictures (`tests/elements.rs`)

Made by a throwaway script (not kept) calling `src/ooxml/{text,shapes,table,xml}.mjs`. The Rust side is given the native
type that stands for each JavaScript object: a slide unit is 1/144 inch, a run's `size` is slide units (so points x 2),
an outline width is slide units (points x 2). `ctx` = `{ lang: "en-US", fonts, link: () => "rId7" }`, `ink` = `#1e293b`,
`run(text, extra)` = `{ text, size: 24, color: ink, typeface: "Inter", ...extra }`, `u(n)` = `n / 144` (units to inches),
`box` = `{ x: u(144), y: u(288), w: u(720), h: u(216) }` (exactly 1", 2", 5" x 1.5"), and `body(paras, o)` =
`txBodyXml(paras, ctx, { anchor: "t", inset: u(12), ...o })`. A paragraph is `{ runs, align: "l" }` unless said.

| File | Call |
| --- | --- |
| `rpr-plain.xml` | `rPrXml(run("x"), ctx)` |
| `rpr-styled.xml` | `rPrXml(run("x", { size: 18, b: true, i: true, u: true, color: "#2563eb", typeface: "JetBrains Mono" }), ctx)` |
| `rpr-alpha.xml` | `rPrXml(run("x", { alpha: 128 / 255 }), ctx)` |
| `rpr-link.xml` | `rPrXml(run("x", { link: "https://example.com/a?b=1&c=2", u: true, color: "#2563eb" }), ctx)` |
| `body-bullet.xml` | `body([{ runs: [run("Item")], bullet: "char", align: "l" }])` |
| `body-numbered.xml` | two paragraphs, `bullet: "number"`, the first `number: 1`, the second with none (the Rust gives `Number(1)`, `Number(2)`) |
| `body-numbered-start.xml` | one paragraph, `bullet: "number", number: 3` |
| `body-level1.xml` | `bullet: "char", level: 1` |
| `body-centered.xml` | `align: "ctr"`, body option `anchor: "ctr"` |
| `body-breaks.xml` | one bold run `"a\nb\n"` |
| `body-empty.xml` | `body([])` |
| `body-pr-autofit.xml` | `bodyPrXml({ placeholder: true, anchor: "b", autofit: "norm" })` then `bodyPrXml({ wrap: false, inset: 0, fontScale: 0.8 })` |
| `text-box.xml` | `shapeXml({ ...box, paras: [{ runs: [run("Hello")], align: "l" }], inset: u(12) }, 4, ctx)` |
| `shape-rect-text.xml` | `shapeXml({ ...box, fill: "#2563eb", paras: [{ runs: [run("Go", { color: "#ffffff" })], align: "ctr" }], anchor: "ctr", inset: u(12) }, 4, ctx)` |
| `shape-rounded.xml` | `shapeXml({ x: 0, y: 0, w: u(400), h: u(200), geometry: "roundRect", radius: u(24), line: { width: 2, color: ink }, paras: [], anchor: "t", inset: 0 }, 4, ctx)` |
| `shape-ellipse.xml` | `geometry: "ellipse"`, `fill: "#7c3aed"`, `fillAlpha: 128 / 255`, `line: { width: 1, color: "#000000" }`, on `box` |
| `shape-line.xml` | `geometry: "line"`, `line: { width: 2, color: ink }`, on `box` |
| `picture.xml` | `pictureXml({ id: 5, name: "Picture 5", descr: 'A "cat" & dog', rId: "rId3", box })` |
| `picture-no-alt.xml` | the same without `descr` |
| `table-2x2.xml` | `tableXml` of a `variant: "plain"` table on `box`, `colW: [u(300), u(420)]`, `rowH: [u(100), u(116)]`, cells with margins `u(12)`, anchor `t`, 0.5pt `ink` borders on every side, paragraphs `{ runs, align: "l", bullet: "none" }`, header cells bold |
| `table-styles.xml` | `tableStylesXml()` |
| `graphic-frame.xml` | `graphicFrameXml({ id: 6, name: "Table 6", box, uri: TABLE_URI, inner: "<x/>" })` |
| `xfrm-plain.xml`, `xfrm-rot90.xml`, `xfrm-rot-minus90.xml` | `xfrmXml(box)` with `rot` unset, 90, -90 |
| `placeholder-inherits.xml`, `placeholder-placed.xml` | `placeholderXml({ id: 2, name: "Title 1", ph: { type: "title" }, box: null or box, paras: [{ runs: [{ text: "Hi & bye", size: 26, color: ink }], align: "l", bullet: "none" }], ctx })` |
| `background-color.xml`, `background-picture.xml` | `backgroundXml({ color: "#ffffff" })`, and with rId `"rId2"` |
| `slide-one-shape.xml` | `slideXml(shapeXml({ ...box, fill: "#2563eb", paras: [], anchor: "t", inset: 0 }, 2, ctx), backgroundXml({ color: "#ffffff" }))` |
| `guid.txt` | `guid("deck-table-default")`, newline, `guid("é🎉")` |

## Animations, transitions and notes (`tests/timing.rs`)

Made by a throwaway script (not kept) calling `src/ooxml/{timing,notes}.mjs`. `e(spid, kind, click, fill)` =
`{ spid, kind, fill, click, dir: "in" }` (`fill` false unless said). The Rust `Effect` has `is_text` for `kind === "text"`,
`has_fill` for `fill`, and an `Enter` that stands for the JavaScript fade (`dir: "in"` is the only direction here).
`videos` are left out, as the Rust side has none. `Appear` and `FlyUp` have no JavaScript counterpart, so no reference file.
`ctx` is as for the text files above.

| File | Call |
| --- | --- |
| `timing-fade-one.xml` | `animationTimingXml({ effects: [e(4, "picture", 1)] })` |
| `timing-two-clicks.xml` | `effects: [e(4, "text", 1), e(5, "picture", 2)]` |
| `timing-text-groups.xml` | `effects: [e(4, "text", 1, true), e(5, "picture", 1), e(4, "text", 2), e(6, "text", 1)]` (grpId counts per shape, `animBg`, two effects in one click) |
| `transition-fade.xml` | `transitionXml("fade")` |
| `transition-push.xml` | `transitionXml({ push: "l" })` |
| `notes-master.xml` | `notesMasterXml(ctx)` |
| `notes-slide.xml` | `notesSlideXml("First line\n\n  second & <third>  \n\n\n\n", ctx)` |
| `notes-slide-empty.xml` | `notesSlideXml("\n  \n", ctx)` |
