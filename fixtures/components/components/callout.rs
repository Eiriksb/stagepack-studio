use toolkit::prelude::*;

/// A title in the theme's accent colour, with what's written between the tags below it.
#[component]
fn Callout(title: String, children: Children, ctx: &Ctx) -> Element {
    let accent = ctx.theme().accent;
    col![
        row![icon("idea", 48.0).fill(accent), text(title).size(40.0).bold().fill(accent)].gap(16.0),
        children.into(),
    ]
    .gap(16.0)
}
