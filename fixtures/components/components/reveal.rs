use toolkit::prelude::*;

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
