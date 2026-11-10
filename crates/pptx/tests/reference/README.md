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
