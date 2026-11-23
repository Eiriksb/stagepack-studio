# Writing a talk, for an AI

You write a talk by writing files. The tool never edits them for you. It gives you eyes: `check` says what is wrong, and `picture` draws a slide so you can look at it. Write, check, look, fix, and repeat until both are happy.

## The talk

A talk is a folder. In it:

- `slides.md` holds every slide. This is the only file you must write.
- `assets/` holds the pictures, videos, 3D models, terminal recordings and CSV files the slides use. A slide names one as `assets/name.png`. A path can't leave the talk.
- `components/` holds the talk's own components, if it has any. See the end of this guide.
- `fonts/` holds fonts the talk carries.

A talk can also be one `.sptx` file, a pack of the same folder. Read a pack the same way. Don't try to write one.

When the server starts on an empty folder, there is no talk yet. Write `slides.md` in that folder, and the next call finds it. The tool reads the files afresh at every call, so you can change them between calls.

A slide is 1920 units wide and 1080 high. Every position and size below is in those units. The top left corner is 0, 0. Slides are numbered from 1, in the order of the file, hidden slides included.

## The tools

- `describe` says what a talk may hold: layouts, settings, and every built-in tag with each of its settings. Call it with no talk or with one. With a talk, it adds the talk's own built components. This guide shows each tag once. `describe` is the full list, and it is always right.
- `check` says every problem, slide by slide, as the app's window would show it. No problems means the talk is sound. Run it after every change.
- `outline` gives each slide's number, layout, first words, number of clicks, whether it is hidden and whether it has notes.
- `picture` draws one slide as a PNG. Give it the slide number, and optionally a step (`--step N`, default the last, which is the whole slide) and a width (`--width W`, 64 to 3840, default 1920). The picture is how you see overlap, overflow, colours and whether a thing is where you meant. Look at every slide you write, and at each step of a slide that has clicks.
- `guide` prints this guide.

On the command line the picture needs `--out FILE.png`. Through the server, the picture comes back in the reply.

In the settings lists that `describe` prints, a setting marked *needed* must be given: the tag does nothing useful without it. Every other setting takes its default, or is empty, when you leave it out. Leave out what you don't need.

## Slides

Slides are separated by a line holding only `---`. The first slide starts the file. Text in a slide is Markdown:

```markdown
# A heading

A paragraph of **bold** and *italic* words.

- A list item
- Another one

1. A numbered item
2. The next
```

A heading, paragraphs and lists go into the slide's layout, in its main region. A slide with no layout is `content`.

### The settings block

A slide may begin with a block of settings, one `key: value` per line between two `---` lines. The first block opens the file, and the slide's text follows the second `---`. Later, the `---` that ends the last slide opens the next slide's block. The first slide's block also sets the talk:

```markdown
---
title: A short talk
layout: title
color.background: #0f172a
color.text: #f8fafc
color.accent: #38bdf8
---

# A short talk

Said in a few minutes

---
layout: section
transition: fade
---

# A new part
```

The talk's settings, in the first slide's block: `title`, `color.background`, `color.text`, `color.accent` (all colours are `#rrggbb`), `font` and `font.mono` (a font the talk carries in `fonts/`). The slide's settings: `layout`, `transition` (`none`, `fade` or `push`) and `hidden: true` (the slide stays in the text, but is left out of the show and the exports). `describe` lists them all.

### Layouts and regions

A layout is a set of regions that hold the slide's text. The layouts are `title`, `content`, `two-columns`, `section` and `blank`. `describe` gives each one's regions. Text goes to the layout's main region, the one that `describe` says content goes in: `body` for most, `left` for `two-columns`. A heading goes in `title` when the layout has one.

To put text in another region, write `<Region name="right" />` on a line of its own. What follows it, up to the next such line, goes in that region:

```markdown
---
layout: two-columns
---

# Two sides

The left side's words.

<Region name="right" />

The right side's words.
```

A region's text can be placed and animated as a placed thing is. In the settings block write `region.NAME: x=… y=… w=… h=… rotate=… click=… enter=… align=…` with the words you need. For example, `region.body: click=each enter=fly-up` shows a list one line at a time.

### Tables, code, notes

A table is a header row, a line of dashes, then rows:

```markdown
# A table

| Day | Talks |
| --- | --- |
| Monday | 3 |
| Tuesday | 4 |
```

Code is a fenced block with the language after the opening fence. Nothing in it is read as a slide or a tag. A fence of tildes is as good as one of backticks:

~~~markdown
# Some code

```rust
fn main() {
    println!("hello");
}
```
~~~

Speaker notes go in `<Notes>`, at the end of the slide. They are not drawn:

```markdown
# With notes

Words on the slide.

<Notes>
What to say. Pause here.
</Notes>
```

## Placing a tag

Anything that is not text is a tag. Write it on its own, with its settings as `name="value"`. Give it a place on the slide with `x`, `y`, `w` and `h`. Every tag takes these, and `rotate` in degrees clockwise:

```markdown
# A shape

<Shape kind="ellipse" fill="#2563eb" x="1240" y="300" w="480" h="320" />
```

Without a place a tag goes where the layout flows it. The text of a tag that holds content, such as `<Box>`, goes between its opening and closing tags, with a blank line after the opening and before the closing one:

```markdown
# A box with words

<Box x="400" y="400" w="1120" h="260" fill="#fef3c7" outline="#d97706" outline-width="4" radius="24" padding="24" align="center">

A note, in a box.

</Box>
```

### Clicks

Without `click`, a thing is there from the start. `click="1"` shows it on the first click, `click="2"` on the second, up to 999. `enter` says how it arrives: `appear` (the default), `fade` or `fly-up`. A slide's steps are 0, then one for each click it uses. The picture's `--step` takes any of them. On text, `click="each"` shows a line at a time. For a chart, `build="true"` steps through its series (see below) and takes clicks of its own.

```markdown
# Step by step

<Box x="400" y="300" w="500" h="200" fill="#dbeafe" click="1" enter="fade" />

<Box x="1020" y="300" w="500" h="200" fill="#dcfce7" click="2" enter="fly-up" />
```

## The built-in tags

One example each. Settings you leave out take their defaults. For every setting's meaning and choices, call `describe`.

`<Box>` is a rectangle with content between its tags, shown above. `<Shape>` draws `rect`, `rounded`, `ellipse`, `line`, `arrow` or `callout`, and may hold content:

```markdown
# Shapes

<Shape kind="arrow" fill="#dc2626" x="300" y="400" w="600" h="200" />

<Shape kind="callout" fill="#fef9c3" outline="#ca8a04" outline-width="3" x="1000" y="360" w="560" h="280">

Look here.

</Shape>
```

`<Icon>` draws a named icon from the Carbon set:

```markdown
# An icon

<Icon name="idea" size="160" color="#ca8a04" x="900" y="400" w="160" h="160" />
```

`<Picture>` shows a picture from the talk. `src` is needed:

```markdown
# A picture

<Picture src="assets/photo.png" x="460" y="300" w="1000" h="560" />
```

`<Video>` plays an MP4 from the talk. `src` is needed. Give it a `poster` to show a still, and `loop="true"` to play again from the start:

```markdown
# A video

<Video src="assets/clip.mp4" poster="assets/photo.png" x="460" y="300" w="1000" h="560" />
```

`<Model>` shows a 3D model, a `.glb` or `.ply` file. `src` is needed. `view` is six numbers, set apart by spaces: yaw and pitch in degrees, the distance, then x, y and z of the point looked at. `views` is several views, set apart by semicolons, one for each click. `turn="auto"` turns it slowly:

```markdown
# A model

<Model src="assets/box.glb" turn="auto" x="460" y="300" w="1000" h="560" />
```

`<Terminal>` plays an asciinema `.cast` recording. `src` is needed:

```markdown
# A recording

<Terminal src="assets/run.cast" x="460" y="300" w="1000" h="560" />
```

`<Chart>` draws `kind` `bar`, `line` or `pie`. Give `labels` and `values` set apart by commas, with one value for each label, or `data` naming a CSV file in the talk whose first column holds labels and whose other columns are series:

```markdown
# A chart

<Chart kind="bar" title="Visitors" labels="Spring, Summer, Autumn, Winter" values="4, 7, 5, 2" x="360" y="260" w="1200" h="640" />
```

```markdown
# A chart from a file

<Chart kind="line" data="assets/sales.csv" x="360" y="260" w="1200" h="640" />
```

## Pictures, videos and the files they use

`check` says when a file a tag names is missing or can't be read. You can't make a binary file by writing text, but an SVG picture is text, so you can write one into `assets/`. If you need a picture, a video or a model, use one the owner put in `assets/`, or use a shape, an icon or a chart. Say so in your reply when a file is missing.

## The talk's own components

A talk may have its own tags, written in Rust in `components/`, and built into `build/`. `describe` with the talk lists the ones that are built, with their settings. Use them as you use a built-in tag.

The tool never builds a component. It builds only in a folder where the owner has set *Always build this folder* in the app, and only when the app has its compiler. A packed talk is never built. Otherwise a component that is not built, or built from an older file, is reported by `check` as a problem. Tell the owner to open the talk in the app and build it. Don't try to work around it.

## The loop

1. Call `describe` to see what is allowed.
2. Write `slides.md`, and the `assets` the slides need.
3. Call `check`. Fix every problem it names, by slide.
4. Call `picture` for every slide, and for each step of a slide with clicks. Look at them. Fix what is off: text that runs out of its box, things on top of each other, a colour too faint to read.
5. Call `check` again, then stop when nothing is wrong and every picture is right.

Keep to a few words a slide. A slide is read from a distance.
