//! The talk's Markdown, to read beside the slide.

use std::ops::Range;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

#[derive(Default)]
pub struct MarkdownPanel {
    lines: Vec<SharedString>,
}

impl MarkdownPanel {
    pub fn set_source(&mut self, source: &str, cx: &mut Context<Self>) {
        self.lines = source.lines().map(|line| line.to_string().into()).collect();
        cx.notify();
    }
}

impl Render for MarkdownPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // On a parent, as a list measures its first row before applying its own text style.
        div()
            .size_full()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .line_height(px(18.))
            .child(
                uniform_list(
                    "markdown-lines",
                    self.lines.len(),
                    cx.processor(|this, visible: Range<usize>, _, _| {
                        visible
                            .map(|index| {
                                let line = this.lines[index].clone();
                                div()
                                    .id(("markdown-line", index))
                                    .test_support()
                                    .aria_label(line.clone())
                                    .px_2()
                                    .truncate()
                                    .child(line)
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .size_full(),
            )
    }
}
