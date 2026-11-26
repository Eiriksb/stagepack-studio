# Writing your own components

A component is a small piece of Rust code that draws something on a slide. You write it once, then use it on any slide by its tag, the way you use `<Chart>` or `<Box>`.

You don't need to know Rust well. If you have written a function in any language, you can follow this. Every piece of code below is taken from a small example talk, kept in `fixtures/components` in the app's source, and every one of those examples builds and runs.

## Words used here

- A **function** is a named piece of code that takes some values and gives one back. In Rust it starts with `fn`.
- An **argument** is one of the values a function takes. `team: String` is an argument called `team`.
- A **type** says what kind of value something is. `String` is text, `u32` is a whole number that can't be negative, `f32` is a number that can have a fraction, such as `1.5`, and `bool` is `true` or `false`.
- A **macro** is a word ending in `!`, such as `row!`. It's used like a function, but it can take any number of values.
- An **element** is one thing on the slide: some text, a shape, a picture, or a group of other elements. A component gives back one element, which can hold many others.
- **Building** turns your Rust file into something the app can run. The app has its own compiler for this, so you don't need to install anything.

## Where a component goes

A talk is a folder with `slides.md` in it. To start one, choose a template on the app's start screen, or *New presentation…* in the File menu, and save it with *Save As* as a talk folder; or make the folder and `slides.md` yourself. Put each component in its own file in a folder called `components`, next to `slides.md`. Pictures, videos, 3D models, terminal recordings and data files go in a folder called `assets`. The example talk has:

- `slides.md`, the slides.
- `components/score.rs`, `components/callout.rs` and the other examples.
- `assets/sales.csv`, the data for its charts.

The file's name can have only letters, digits, `_` and `-`, and must end in `.rs`. After building, the app puts what it built in a folder called `build`. That folder travels with the talk, so someone who only opens or presents it never needs a compiler.

## Your first component

This is `components/score.rs`:

```rust
use toolkit::prelude::*;

/// A team's name, and its points on a badge of its colour.
#[component]
fn Score(team: String, points: u32, colour: Color) -> Element {
    row![text(team).size(48.0), badge(points, colour)].gap(24.0)
}
```

Line by line:

- `use toolkit::prelude::*;` makes everything the toolkit offers available. Every component starts with it.
- Lines starting `///` are comments for people. The app ignores them.
- `#[component]` marks the function as the file's component.
- `fn Score(...)` names it. **The function's name is the tag**, so this is used as `<Score>`. Start it with a capital letter.
- `-> Element` says that it gives back an element.
- `row![...]` puts elements side by side. `.gap(24.0)` keeps 24 units between them. A slide is 1920 units wide and 1080 high.
- `text(team).size(48.0)` is the team's name, 48 units high.

`badge` is an ordinary function of your own, further down the same file. A component can call as many of these as it likes. You'll see how it's drawn under [Placing things by hand](#placing-things-by-hand).

On a slide, it's used like this:

```markdown
<Score team="Oslo" points="3" colour="#16a34a" />
```

Each attribute in the tag is given to the argument of the same name. An argument written with `_`, such as `outline_width`, is written with `-` in the tag: `outline-width="4"`.

Some names are kept for the slide, for where a tag is placed and when it appears, and for what a later version may add: `x`, `y`, `w`, `h`, `rotate`, `align`, `click`, `enter`, `z`, `opacity`, `id`, `class`, `style`, `exit`, `key`, `ref` and `slot`. An argument can't have one of them, and building a component that tries says so.

## Arguments and their form fields

The app knows the type of each argument, so a form for the tag can show a field to suit each one. A component never fails because of a missing or wrong value: it uses the value given here instead.

| Type | What it holds | Form field | If missing or wrong |
| --- | --- | --- | --- |
| `String` | Text | Text box | Empty text |
| `u32` or `i32` | A whole number (`i32` can be negative) | Number box | `0` |
| `f32` | A number with a fraction | Number box | `0.0` |
| `bool` | `true` or `false` | Checkbox | `false` |
| `Color` | `#rgb`, `#rrggbb`, or a name such as `red` | Colour picker | Black |
| `PictureFile` | The path of a picture in the talk | File picker for pictures | Empty path |
| `DataFile` | The path of a CSV file in the talk | File picker for CSV files | Empty path |
| `Option<String>`, or `Option` of any type above | A value that can be left out | The same field, marked optional | `None`, which means "not given", when the attribute is left out. A value that is there but wrong gives that type's usual fallback, such as `Some(0)` for `Option<u32>` |

For a fixed list of choices, write an `enum` above the component with `#[derive(Prop)]` on it, and use it as an argument's type. The form shows a dropdown, and the first choice is used when the value is missing or wrong. A choice written `LineChart` in Rust is written `line-chart` in the tag.

### What the form looks like to someone who doesn't code

Once the component is built, it's in the list under *Component* on the Insert tab. Putting it on a slide writes its tag with a starting value for every argument it must have, selects it, and shows its form in the panel at the right of the window. Selecting it later shows the same form. Each argument is one field, under a label, in the order the arguments are written:

- **Text** (`String`): a box to type in. What's typed is written when the field is left or Enter is pressed, and Escape puts back what was there. Quotes, `<` and `&` are written so they read back as typed. A value holding both kinds of quote mark, or a new line, can't be written, and the field says so.
- **Whole number** (`u32`, `i32`) and **number** (`f32`): a box with − and + beside it, and the Up and Down keys step it. A comma is taken as the decimal mark, so `1,5` is 1.5. Text that isn't a number, or a fraction in a whole number, is refused with a sentence under the field.
- **Yes or no** (`bool`): a checkbox, written at once.
- **Colour** (`Color`): a swatch that opens a palette and sliders, written at once.
- **Picture** and **data file** (`PictureFile`, `DataFile`): *Choose file…* and the file's name. A file chosen from outside the talk is added to it, and saved with it.
- **A fixed list of choices** (a `#[derive(Prop)]` enum): a dropdown, its choices as words.
- **Can be left out** (`Option<…>`): the same field. Emptied, the argument is taken out of the tag; a colour has a *None* button for it.

Under the arguments are the tag's place on the slide: *Left*, *Top*, *Width*, *Height* and *Turn*. A component placed on a slide is also moved, resized and turned there like any other thing: dragged, by its handles, or with the arrow keys. Each writes the same `x`, `y`, `w`, `h` and `rotate` on its tag, which is why an argument can't have those names. The Animate tab writes its `click` and `enter` in the same way, to bring it in on a click. A component that steps through clicks of its own, as a chart built up by clicks does, keeps those steps: the tab never renumbers them.

A label is the argument's name as words: each `_` is a space and the first letter is a capital, so `team_name` shows as *Team name* and `points` as *Points*. A choice is shown the same way, so `LineChart`, written `line-chart`, shows as *Line chart*. Name the arguments for the person who will fill the form in.

An argument called `children` has no field. It makes the component one whose words can be typed on the slide: a click on it selects it, and a second click, or a double-click, puts the caret in its words. A component without `children` is set up through its form alone.

## Putting elements together

These make elements:

- `text("words")`: text.
- `shape(ShapeKind::Rounded, width, height)`: a shape. The kinds are `Rect`, `Rounded`, `Ellipse`, `Line`, `Arrow` and `Callout`.
- `icon("idea", size)`: an icon from the Carbon set, by its name.
- `picture("assets/photo.png", width, height)`: a picture from the talk's `assets` folder.
- `path("M 0 0 L 100 100", width, height)`: a line or shape in the words of an SVG path.
- `canvas(width, height, items)`: an area where you place each thing yourself. See [Placing things by hand](#placing-things-by-hand).
- `problem("what's wrong")`: a labelled box saying what's wrong, in place of what you meant to draw.
- `Element::Empty`: nothing at all.

These change an element, written after it with a dot:

- `.size(48.0)`: the size of text or an icon.
- `.bold()`: bold text.
- `.fill(colour)`: the colour of text or an icon, or the inside of a shape or path.
- `.outline(colour)`: a line around a shape.
- `.stroke(colour, width)`: the line along a path.
- `.gap(24.0)`: the space between the elements in a row or a column.
- `.child(element)`: what's inside a shape, such as `shape(ShapeKind::Rounded, 120.0, 80.0).child(text("3"))`.
- `.padding(8.0)`: the space a shape keeps clear between its edge and what's inside it.
- `.radius(12.0)`: how round the corners of a `Rounded` shape or a `Callout` are.

Build elements with these, rather than by writing out an `Element`'s fields: a later version of the toolkit can add fields, and code that names them all would stop compiling.

`row![a, b, c]` puts elements side by side, and `col![a, b, c]` puts them one under the other.

Numbers that are sizes are written with a fraction, as `48.0`, not `48`, because they are `f32`.

## Wrapping content with `children`

A component can wrap what's written between its tags. This is `components/callout.rs`:

```rust
#[component]
fn Callout(title: String, children: Children, ctx: &Ctx) -> Element {
    let accent = ctx.theme().accent;
    col![
        row![icon("idea", 48.0).fill(accent), text(title).size(40.0).bold().fill(accent)].gap(16.0),
        children.into(),
    ]
    .gap(16.0)
}
```

An argument called `children` of type `Children` is where the content goes: `children.into()` turns it into an element. Place it once: the content can only be in one place, and a component that places it twice gives an answer that can't be read. It has no form field, because you edit it on the slide. Leave a blank line after the opening tag and before the closing one, so the content is read as Markdown:

```markdown
<Callout title="Remember">

What's written between the tags goes **below the title**.

</Callout>
```

`ctx` is the **context**: what the component can find out about where and when it's drawn. `ctx.theme()` gives the talk's colours: `background`, `text` and `accent`. Using them, rather than colours of your own, makes the component fit any theme. `let accent = ...;` gives a value a name, to use below.

`ctx` also has `ctx.width()`, the width of the box the tag sits in, `ctx.height()`, its height, or 0 if it grows with what's in it, and `ctx.slide_width()` and `ctx.slide_height()`.

## Changing with clicks: `ctx.steps`

This is `components/reveal.rs`:

```rust
/// Three lines, one more on each click.
#[component]
fn Reveal(first: String, second: String, third: String, ctx: &Ctx) -> Element {
    // Asks the slide for three clicks, and gives how many have happened so far.
    let shown = ctx.steps(3);
    col![
        line(first, shown >= 1),
        line(second, shown >= 2),
        line(third, shown >= 3),
    ]
    .gap(24.0)
}

/// `words` if they are shown yet, or else nothing at all.
fn line(words: String, shown: bool) -> Element {
    if shown { text(words).size(40.0) } else { Element::Empty }
}
```

`ctx.steps(3)` asks the slide for three clicks, and gives how many of them have happened: 0 before the first, then 1, 2 and 3. The slide waits for those clicks before moving on. A line starting `//` is a comment, like `///`.

## Changing over time: `ctx.time`

`ctx.time()` gives the seconds since the slide appeared. Using it makes the app draw the component again and again while the slide is shown.

This is `components/clock.rs`:

```rust
/// A clock face whose hand goes round once a minute, from when the slide appears.
#[component]
fn Clock(size: f32, ctx: &Ctx) -> Element {
    // A missing or bad number is 0, so this is the size to use then.
    let size = if size > 0.0 { size } else { 200.0 };
    let middle = size / 2.0;
    let turned = ctx.time() / 60.0 * std::f32::consts::TAU;
    let tip_x = middle + turned.sin() * middle * 0.8;
    let tip_y = middle - turned.cos() * middle * 0.8;
    // A line from the middle to the tip, in the words of an SVG path.
    let hand = format!("M {middle} {middle} L {tip_x} {tip_y}");
    let white = Color::rgb(255, 255, 255);
```

The function goes on to draw the face and the hand, shown under [Placing things by hand](#placing-things-by-hand). `TAU` is a full turn, so `turned` goes once round in 60 seconds. `format!` makes text with values put in where the names are in `{ }`. `Color::rgb(255, 255, 255)` is a colour by its red, green and blue, each from 0 to 255: this one is white.

## Placing things by hand

Rows and columns place things for you. A **canvas** lets you put each thing exactly where you want it. It's an area of a width and height, and a list of items, each made by `at(x, y, element)`. `x` is how far from the left and `y` how far from the top the element's top-left corner goes. Later items are drawn over earlier ones.

The clock draws its face, then its hand over it:

```rust
    canvas(
        size,
        size,
        vec![
            at(0.0, 0.0, shape(ShapeKind::Ellipse, size, size).fill(ctx.theme().accent)),
            at(0.0, 0.0, path(hand, size, size).stroke(white, size / 25.0)),
        ],
    )
```

`vec![...]` is a list. The path's numbers are counted from the canvas's top-left corner.

The score's badge puts its number over a rounded box. `.centered_in(120.0)` centres the text across 120 units from where it's placed, and `.right_aligned_in(...)` would put it at the right:

```rust
fn badge(points: u32, colour: Color) -> Element {
    let white = Color::rgb(255, 255, 255);
    canvas(
        120.0,
        80.0,
        vec![
            at(0.0, 0.0, shape(ShapeKind::Rounded, 120.0, 80.0).fill(colour)),
            at(0.0, 10.0, text(points.to_string()).size(48.0).bold().fill(white)).centered_in(120.0),
        ],
    )
}
```

`points.to_string()` turns the number into text.

## A worked example: a bar chart

`components/bars.rs` draws a bar for each row of a CSV file. It's built the same way as the ready-made `<Chart>`, which is an ordinary component too, only longer. On a slide:

```markdown
<Bars data="assets/sales.csv" />
```

And the file it reads:

```text
Quarter,Sales
Q1,120
Q2,180
Q3,150
Q4,240
```

It starts with a few fixed sizes. `const` gives a value a name for the whole file:

```rust
const WIDTH: f32 = 1200.0;
const HEIGHT: f32 = 600.0;
/// The room below the bars, for their labels.
const LABEL_ROOM: f32 = 48.0;
```

It reads the file with `ctx.asset_text`, which gives the text of a file in the talk's `assets` folder. A path given to `ctx.asset_text`, `ctx.asset` or `picture` always starts with `assets/`, as in `assets/sales.csv`, and is written with `/` between folders on every computer. Any other path gives nothing, or a labelled box for a picture. If the file can't be read, it gives back a labelled box saying so, rather than nothing:

```rust
fn Bars(data: DataFile, ctx: &Ctx) -> Element {
    let Some(table) = ctx.asset_text(&data.0) else {
        return problem(format!("Can't read {}", data.0));
    };
```

`data.0` is the path held by the `DataFile`. `let Some(table) = ... else { ... };` means: if there's a text, call it `table` and go on, and if not, do what's in the `else`. `return` gives back a value straight away. (`ctx.asset(...)` gives a file's bytes instead, for a file that isn't text.)

Then it reads each line after the first into a label and a value, and stops with a labelled box at a value that isn't a number:

```rust
    let mut rows = Vec::new();
    for line in table.lines().skip(1) {
        let Some((label, number)) = line.split_once(',') else { continue };
        let Ok(value) = number.trim().parse::<f32>() else {
            return problem(format!("\"{}\" isn't a number", number.trim()));
        };
        rows.push((label.trim().to_string(), value));
    }
```

`Vec::new()` is an empty list, and `mut` lets it change. `for ... in` goes through each line, `split_once(',')` cuts a line at its first comma, `continue` skips a line without one, and `parse` reads text as a number.

It finds the largest value, so that the tallest bar fills the height:

```rust
    let largest = rows.iter().map(|(_, value)| *value).fold(0.0, f32::max);
    if largest <= 0.0 {
        return problem("There are no bars to draw");
    }
```

Then it gives each row an equal slot across the width, and puts a bar and a label in each:

```rust
    let slot = WIDTH / rows.len() as f32;
    let floor = HEIGHT - LABEL_ROOM;
    let mut items = Vec::new();
    for (index, (label, value)) in rows.into_iter().enumerate() {
        let x = index as f32 * slot;
        let height = value / largest * floor;
        if height > 0.0 {
            let bar = shape(ShapeKind::Rect, slot * 0.8, height).fill(ctx.theme().accent);
            items.push(at(x + slot * 0.1, floor - height, bar));
        }
        items.push(at(x, floor + 8.0, text(label).size(24.0)).centered_in(slot));
    }
    canvas(WIDTH, HEIGHT, items)
}
```

`as f32` turns a count into a number with a fraction. Each bar takes the middle 80% of its slot, and stands on the `floor`, so its top is at `floor - height`. A bar with no height is left out, as a shape of no size would be given a size of its own.

The ready-made chart adds axes, ticks, several series, lines and pies, but it's made of nothing more than this.

## Building

Opening a talk never builds anything. A talk can come from anyone, and building runs the compiler on its code, so the app waits until you say so. Until a component is built, its place on the slide shows a labelled box saying it needs building, and a bar under the slide offers two buttons:

- **Build components** builds once, now.
- **Always build this folder** builds now, and again every time a component in the folder changes. That includes changes that arrive from somewhere else, such as a `git pull`.

A talk you made in the app and saved as a talk folder starts with *Always build this folder* on, as its code is your own. A packed talk never builds.

The app remembers *Always build this folder* by the folder's path, not by what's in it. So don't turn it on for a folder that you'll later replace with someone else's talk: their components would then be built without asking you.

If this copy of the app has no compiler, the bar says *This copy of the app has no compiler, so components can't be built.* instead, and offers no buttons. Components that were already built still show.

If a component doesn't build, its errors appear below the slide with the file and line, and the slide keeps showing its last good build.

## Labelled boxes

When a component can't be shown, a labelled box takes its place and says why:

| The box says | What it means |
| --- | --- |
| *Score needs building* | It has never been built here. Build it with the bar under the slide. |
| *Score: took too long* | One drawing of it took more than 250 ms. Look for a loop that doesn't end. |
| *Score: used too much memory* | It used more than 64 MiB. |
| *Score: stopped working* | It stopped partway, as Rust does when something goes wrong it can't carry on from, such as reading past the end of a list. |
| *Score: gave an answer that can't be read* | What it drew was too large or too deep. See the limits below. |
| *Score: can't be loaded*, or *too large to run* | Its build is damaged or larger than 4 MiB, or the talk's components have already loaded 16 MiB between them. Build it again, or use fewer large components. |
| *Score: built by a newer version of this app* | Someone built it with a newer app than yours. Build it again here, or update the app. |
| *Score: not run, as this slide's components took too long* | The components on this slide took more than a second between them, so the rest were skipped. |
| *There is no component called Score* | The talk has no component of that name, and nor does the app. Check the spelling, and that its file is in `components`. |
| *This slide has too much on it to show* | The slide, with everything its components drew, came to more than the app can lay out in time. Show less on it. |
| *Nested too deeply* | Lists, tags and what components drew sit more than 64 levels inside each other. |
| Your own words | The component gave back `problem("...")`. Only the first 200 characters are shown. |

A note under the slide saying *Score is out of date, so this is its last build* means that you changed the file since its last build.

## Reacting to clicks: `ctx.events` and `ctx.state`

A component can react to the pointer, and to buttons on its slide. Mark it `#[component(interactive)]` and give it two new things: `ctx.events()` says what has reached it since it was last drawn, and `ctx.state()` and `ctx.set_state(&value)` keep a value from one drawing to the next.

This is `components/counter.rs`:

```rust
use toolkit::prelude::*;

/// A count that goes up when its plus is pressed, or when a button on the slide says `add`, and
/// goes back to 0 when a button says `reset`.
#[component(interactive)]
fn Counter(ctx: &Ctx) -> Element {
    // What the count was after the last drawing: 0 when the slide has just appeared.
    let mut count: u32 = ctx.state();
    for event in ctx.events() {
        match event {
            // A press let go of on the plus.
            Event::Pointer { kind: PointerKind::Up, hit: Some(name), .. } if name == "plus" => count += 1,
            // A button's `do="add" value="1"` adds its value; a bad value adds nothing.
            Event::Action { name, value } if name == "add" => count += value.parse::<u32>().unwrap_or(0),
            Event::Action { name, .. } if name == "reset" => count = 0,
            _ => {}
        }
    }
    // Kept for the next drawing, for as long as the slide is shown.
    ctx.set_state(&count);
```

The function goes on to draw the number and the plus, shown under [Placing things by hand](#placing-things-by-hand). To use it, put its tag on a slide with an `id`, and give buttons the same name in `on`. This slide is in the example talk:

```markdown
# A counter, with buttons

<Counter id="count" x="660" y="300" w="400" h="160" />

<Button label="Add 1" do="add" value="1" on="count" x="560" y="620" w="300" h="96" />

<Button label="Reset" do="reset" on="count" look="secondary" x="1060" y="620" w="300" h="96" />
```

**Events.** `ctx.events()` is the list of what happened, first to last. There are two kinds:

- `Event::Pointer { kind, x, y, hit }` is the pointer. `kind` is `PointerKind::Down`, `Move`, `Up` or `Wheel { dy }`. `x` and `y` are in the component's own units, with the top-left of its box at 0, 0. `hit` is the name of the part under the pointer, or `None`.
- `Event::Action { name, value }` is a button's `do` and `value` (an empty `value` if the button gave none).

**Parts that can be pressed.** Wrap an element in `hit("plus", element)`, which gives back an `Element::Hit`, and the pointer events over it carry `hit: Some("plus")`. A wrapped element looks the same as before. A press that starts on one part and is let go of on another is not a click on either, so a component that counts clicks should count the `Up` and look at its `hit`, as the counter does.

**State.** `ctx.state()` gives back what the last drawing gave to `ctx.set_state(&value)`, or the default of its type (0 for a number) when there is none or it can't be read as that type. `ctx.set_state` keeps anything that can be turned into JSON, so a number, some text, a list or a small record all work. If a drawing doesn't call `ctx.set_state`, the component keeps what it had.

**What the app does with them:**

- **Only a component placed on the slide itself is interactive.** One inside another component, or inside a `<Callout>`'s children, is refused with *An interactive component works only when it's placed on the slide itself.*
- **State lives while its slide is shown.** Leaving the slide clears it, and it is never written to the talk. Editing the talk while the slide is shown keeps the state of a component whose tag and settings haven't changed, and clears the rest.
- **At most 60 drawings a second.** Events that come faster are gathered up and sent together, keeping every `Down`, `Up` and `Action` and only the last `Move`. At most 64 events go to one drawing, and a `Move` is dropped before anything else.
- **State is at most 64 KiB, in and out.** A component that gives back more shows *This component's state is over 64 KiB.*
- **The usual limits apply to every drawing:** 250 ms, 64 MiB of memory and 4 MiB of answer. A component that fails under events, by running out of time, trapping or giving back bad state, shows its problem, and the show moves on.
- **A click on a part that is interactive goes to the component, and the show doesn't advance.** Everywhere else a click advances as usual, and the keys always move the show.
- **In the editor,** select the component and press *Try* (or Space) to click it in place; selecting something else stops it.
- **A component without `#[component(interactive)]` stays as it was.** It gets no events and no state, and it still runs in older apps. An interactive one needs an app that knows interface version 2.

## What a component can't do, and why

A talk is shared like any other file, so a component is held to some limits. They keep a talk you open from reaching anything else on your computer.

- **Names can only use ASCII letters, digits and `_`.** The app checks your code before building it, and it can only be sure it reads names the same way the compiler does when they're plain ASCII.
- **You can't define your own macros with `macro_rules`.** A macro can put together code that the check never sees.
- **`include!`, `include_str!`, `include_bytes!`, `env!`, `option_env!`, `#[path]`, `mod name;` and `#![feature]` are refused**, because a talk's source could otherwise copy a private file from your computer into its build, which then travels with the talk. A `mod` with its body in `{ }` is fine.
- **One `#[component]` per file.** The file's component is its tag, and each file is built on its own.
- **Only the toolkit, no outside crates** (a crate is a Rust library). A component is one file with no project around it, so there's nowhere to name another one, and building never downloads anything.
- **Files are read only from the talk's `assets` folder.** A component can't see anything else on the computer of whoever opens the talk. `ctx.asset_text` gives nothing for a path outside it, or for a path longer than 256 bytes.
- **One drawing can read at most 64 MiB of files between them.** A read past that gives nothing.
- **A component can't play a video.** A video goes on a slide by itself, from the Insert tab or as `<Video src="assets/clip.mp4" />`, and plays there.
- **A component can't draw a 3D model.** A model goes on a slide by itself, from the Insert tab or as `<Model src="assets/model.glb" />`, and is drawn and turned there.
- **A component can't play a terminal recording.** A recording goes on a slide by itself, from the Insert tab or as `<Terminal src="assets/run.cast" />`, and plays there.
- **A component's elements can be nested at most 40 levels deep, and it can give back at most 20,000 elements and 256 KiB of text**, counting its text, problems, icon names, picture paths and path data together. It can place the content between its tags once. Past that, what it drew can't be read back safely or laid out in time.
- **Each drawing has 250 ms and 64 MiB.** A component that never finishes or keeps taking memory is stopped, so it can't freeze the app.
- **A source file is at most 1 MiB, a build at most 4 MiB, and a talk has at most 256 components.** These keep a talk from taking a long time to open. A talk loads at most 16 MiB of builds between them, and loading that much takes several seconds the first time.
- **Building is stopped after 60 seconds, or once it has written 64 MiB, or, on Linux, once it uses 4 GiB of memory.** A real component builds in a second or two.

## Using a code editor

The app doesn't need anything else to build your components. But if you have Rust installed and want your editor to check your components with Cargo, open the talk and choose **File → Set up for a code editor**. The app then says where it has put a small Cargo project for the talk: a folder in the app's own data folder, named after the talk's folder, holding one `Cargo.toml`. That file points your editor at the toolkit the app came with, and at each of the talk's components by its full path.

Open that folder in your editor, as the project. The component files you edit are still the talk's own, in its `components` folder: the project only tells the editor where they are. Choose **Set up for a code editor** again after adding a component, and the file is written afresh to include it. Running `cargo check --examples` in that folder checks every component. Whether an editor also completes names in files that sit outside the project's folder depends on the editor, and this hasn't been tried yet.

The app never puts a `Cargo.toml` in a talk. Cargo and rustup look for their settings in the folder they are run from and every folder above it. With the project kept out of the talk, running them from the project's folder reads nothing from the talk but the components themselves.

That protection ends if you open the talk's own folder in your editor as a project, or run Cargo from inside it. An editor reads settings from any folder you open that way, and those settings can make it run programs. So only open a talk's folder in a code editor if you trust where the talk came from. Don't add a `Cargo.toml` to a talk yourself either, least of all one you got from someone else: it would make the talk's folder a Rust project, and files in it could then make Cargo or your editor run programs on your computer.
