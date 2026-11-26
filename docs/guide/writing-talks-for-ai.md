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
- `picture` draws one slide as a PNG. Give it the slide number, and optionally a step (default the last, which is the whole slide) and a width. On the command line these are `--step N` and `--width W` (64 to 3840, default 1920). Through the server they are the arguments `slide`, `step` and `width` (64 to 1920, default 1280), and a picture over 3.5 MB is drawn again narrower, which the reply says. The picture is how you see overlap, overflow, colours and whether a thing is where you meant. Look at every slide you write, and at each step of a slide that has clicks.
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

A slide may begin with a block of settings, one `key: value` per line between two `---` lines. A line of the form `key: value` between two `---` lines is read as settings, whatever its words, and is not drawn. Don't start a slide with a `---` line and then a line like `Note: x`. The first block opens the file, and the slide's text follows the second `---`. Later, the `---` that ends the last slide opens the next slide's block. The first slide's block also sets the talk:

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

A layout is a set of regions that hold the slide's text. The layouts are `title`, `content`, `two-columns`, `section` and `blank`. `describe` gives each one's regions. Text goes to the layout's main region, the one that `describe` says content goes in: `body` for most, `left` for `two-columns`. The first heading goes in `title` when the layout has one, unless a `<Region>` line comes before it. Any later heading is ordinary text in the region it is in.

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

Anything that is not text is a tag. Write it on its own, with its settings as `name="value"`. Give it a place on the slide with `x`, `y`, `w` and `h`, and turn it with `rotate` in degrees clockwise. Give every tag at least one of `x`, `y`, `w` and `h`: a tag with none is not placed, so its `click`, `enter`, `rotate` and `align` are ignored, and `check` does not complain. Give all four:

```markdown
# A shape

<Shape kind="ellipse" fill="#2563eb" x="1240" y="300" w="480" h="320" />
```

The text of a tag that holds content, such as `<Box>`, goes between its opening and closing tags, with a blank line after the opening and before the closing one:

```markdown
# A box with words

<Box x="400" y="400" w="1120" h="260" fill="#fef3c7" outline="#d97706" outline-width="4" radius="24" padding="24" align="center">

A note, in a box.

</Box>
```

### Clicks

Without `click`, a thing is there from the start. On a placed tag, `click` is a whole number: `click="1"` shows it on the first click, `click="2"` on the second, up to 999. `click="each"` on a tag is a problem. `enter` says how it arrives: `appear` (the default), `fade` or `fly-up`. A slide's steps are 0 up to the highest click it uses, so use clicks 1, 2, 3 with none left out. The picture's `--step` takes any of them. To show the text of a region a line at a time, write `click=each` in a `region.NAME:` line of the settings block, as `region.body: click=each enter=fly-up`. For a chart, `build="true"` steps through its series (see below) and takes clicks of its own.

```markdown
# Step by step

<Box x="400" y="300" w="500" h="200" fill="#dbeafe" click="1" enter="fade" />

<Box x="1020" y="300" w="500" h="200" fill="#dcfce7" click="2" enter="fly-up" />
```

```markdown
---
region.body: click=each enter=fly-up
---

# One line at a time

- First
- Second
- Third
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

`<Terminal>` plays an asciinema `.cast` recording. Give it a `src`, or a `run` (below), or both:

```markdown
# A recording

<Terminal src="assets/run.cast" x="460" y="300" w="1000" h="560" />
```

### Live terminals

`run` is a command for the terminal to run live while the owner presents, such as `run="bash"`. Spaces set its words apart, double quotes group words, and no shell is added. `cols` and `rows` are the size of the live screen, 100 by 30 if you leave them out. You can only write the tag. This tool never runs a command: not `check`, `outline`, `describe`, `picture`, or anything else it does. Only the owner's app runs it, and only after the owner has said yes to that exact command, so tell the owner that the slide has one. Always give a `src` recording beside a `run`: it is what shows when the owner says no, in exports, in thumbnails and when the add-on isn't there. If you can't make one, say so: the owner can run the command and choose *Keep as recording* in the app, which saves what it showed and fills in `src`. Never write a command that deletes, sends or changes anything. If a command starts a container, give it `--rm` and `-it`, as in `docker run --rm -it alpine sh`, so that nothing is left running when the talk ends. The program is found only in the system's own folders (never in the talk), and a program written as a path, like `./demo`, is one the talk brings, which the owner is told. On Windows only a program (`.exe` or `.com`) runs: name the `.exe` (for example `node.exe`), or run a script through `cmd.exe /c …` written in full (an argument can't hold a double quote on Windows, and a program on a network folder isn't run). Leave out invisible or unusual spacing characters: a command that has them isn't run.

```markdown
# A live terminal

<Terminal run="bash" src="assets/run.cast" cols="100" rows="30" x="460" y="300" w="1000" h="560" />
```

### Buttons

`<Button>` is a button the owner presses in the show. Give it a `label` and exactly one action. A button with none shows *This button has no action.*, and one with more than one shows *This button has more than one action: run, do or step.* Pressing it does nothing then. `look` is `primary` (the default), `secondary` or `quiet`. Any placed tag can take an `id`, which is how a button names it; no two things on a slide may share one. A button is pressed with a left click only. It looks pressed while the pointer is down on it, and acts when the pointer is let go over it. A click on a button never advances the show, and the keys still do.

- `step="N"` goes straight to click N of its slide, from 0 to the slide's last click. Anything else shows *This button's step is past the slide's last click.*
- `run="…"` with `in="<id>"` types that text, then Enter, into the live terminal on the same slide whose `id` it names, starting that terminal first if it isn't running. The terminal's own command has the owner's yes, as always. The typed text has a yes of its own: the owner is asked on the first press, with the whole text shown, once for each exact text (*Always for this folder* is offered for a talk in a folder only), and asked again if the text changes. A text that is empty, holds control characters other than a tab, is over 4,096 bytes, or holds the invisible or unusual characters a live terminal's command can't hold, is refused and never typed. A tab is allowed, but a shell completes at a tab, so keep tabs out of it. Don't write a text that deletes, sends or changes anything. As with a live terminal, this tool never types anything.
- `do="<action>"` with `on="<id>"`, and an optional `value`, sends the named action, with the value (or an empty one), to the interactive component with that `id` on the same slide. An `on` that names nothing, or something that isn't interactive, shows *This button's `on` names nothing it can act on.*

`check` reports every one of these notices, and every `in`, `on` and `step` is checked. In the editor, `run` and `step` do nothing, and the button's form says *Runs in the show.* A `do` works there while its target is being tried: the owner selects the component, presses *Try*, and then the button.

```markdown
# Press to go on

<Box x="360" y="300" w="1200" h="200" click="1">

The second half

</Box>

<Button label="Show the rest" step="1" x="810" y="700" w="300" h="96" />
```

```markdown
# A button for the terminal

<Terminal id="shell" run="bash" src="assets/run.cast" x="460" y="200" w="1000" h="560" />

<Button label="List the files" run="ls" in="shell" look="secondary" x="810" y="820" w="300" h="96" />
```

### Charts

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

The tool builds a component only for a folder the owner has set to *Always build this folder* in the app, and only when the app has its compiler. It never builds in any other folder, and never a packed talk. Otherwise a component that is not built, or built from an older file, is reported by `check` as a problem. Tell the owner to open the talk in the app and build it. Don't try to work around it.

## The loop

1. Call `describe` to see what is allowed.
2. Write `slides.md`, and the `assets` the slides need.
3. Call `check`. Fix every problem it names, by slide.
4. Call `picture` for every slide, and for each step of a slide with clicks. Look at them. Fix what is off: text that runs out of its box, things on top of each other, a colour too faint to read.
5. Call `check` again, then stop when nothing is wrong and every picture is right.

Keep to a few words a slide. A slide is read from a distance.
