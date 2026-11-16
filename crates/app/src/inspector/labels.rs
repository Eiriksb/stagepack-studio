//! What the inspector calls each setting, and the order the built-in components show theirs in.
//! A talk's own component's settings are named from its arguments, in the order it gives them.

use toolkit::{PropField, PropKind};

/// A built-in component's settings, by argument, in the order they're shown, with their labels.
const BUILT_INS: [(&str, &[(&str, &str)]); 5] = [
    ("Shape", &[("kind", "Shape"), ("fill", "Fill"), ("outline", "Outline"), ("outline-width", "Outline width")]),
    (
        "Box",
        &[
            ("fill", "Fill"),
            ("outline", "Outline"),
            ("outline-width", "Outline width"),
            ("radius", "Corner radius"),
            ("padding", "Padding"),
        ],
    ),
    ("Icon", &[("name", "Icon"), ("size", "Size"), ("color", "Colour")]),
    ("Picture", &[("src", "Picture")]),
    (
        "Chart",
        &[
            ("kind", "Kind"),
            ("title", "Title"),
            ("labels", "Labels"),
            ("values", "Values"),
            ("data", "Data file"),
            ("build", "Build up by clicks"),
        ],
    ),
];

/// The built-ins' arguments that are colours, though they take them as text, so that one the
/// toolkit can't read is no colour rather than black: `(tag, argument)`.
const COLOURS: [(&str, &str); 5] = [("Shape", "fill"), ("Shape", "outline"), ("Box", "fill"), ("Box", "outline"), ("Icon", "color")];

/// What a choice of a built-in is called where its own name won't do: `(tag, argument, value,
/// label)`.
const CHOICES: [(&str, &str, &str, &str); 2] = [
    ("Shape", "kind", "rect", "Rectangle"),
    ("Shape", "kind", "rounded", "Rounded rectangle"),
];

/// `name`, an argument's or a choice's, as words: `_` and `-` are spaces, and the first letter
/// is a capital. "outline_width" is "Outline width".
pub fn plain(name: &str) -> String {
    let spaced = name.replace(['_', '-'], " ");
    let mut letters = spaced.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// The settings of `tag`'s component `props`, in the order they're shown, each with its label:
/// a built-in's as it has them, any it doesn't know of after, and a talk's own as it gives them.
pub fn ordered(tag: &str, props: &[PropField]) -> Vec<(PropField, String)> {
    let Some((_, known)) = BUILT_INS.iter().find(|(name, _)| *name == tag) else {
        return props.iter().map(|prop| (prop.clone(), plain(&prop.name))).collect();
    };
    let mut ordered: Vec<(PropField, String)> = known
        .iter()
        .filter_map(|(name, label)| {
            let prop = props.iter().find(|prop| prop.name == *name)?;
            let colour = COLOURS.contains(&(tag, prop.name.as_str())) && prop.kind == PropKind::Text;
            let prop = if colour { PropField::new(prop.name.clone(), PropKind::Color, prop.optional) } else { prop.clone() };
            Some((prop, label.to_string()))
        })
        .collect();
    for prop in props {
        if !known.iter().any(|(name, _)| *name == prop.name) {
            ordered.push((prop.clone(), plain(&prop.name)));
        }
    }
    ordered
}

/// What value `value` of `tag`'s choice `argument` is called.
pub fn choice(tag: &str, argument: &str, value: &str) -> String {
    CHOICES
        .iter()
        .find(|(t, a, v, _)| *t == tag && *a == argument && *v == value)
        .map_or_else(|| plain(value), |(_, _, _, label)| label.to_string())
}

/// The attribute an argument is written as: its name with `-` for `_`.
pub fn attribute(argument: &str) -> String {
    argument.replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_reads_as_words() {
        assert_eq!(plain("outline_width"), "Outline width");
        assert_eq!(plain("line-chart"), "Line chart");
        assert_eq!(plain("x"), "X");
        assert_eq!(plain(""), "");
        assert_eq!(plain("élan"), "Élan");
    }

    #[test]
    fn a_built_in_has_its_own_order_and_a_talks_own_keeps_its() {
        let field = |name: &str| PropField::new(name, PropKind::Text, false);
        let chart = [field("kind"), field("labels"), field("values"), field("data"), field("title"), field("build"), field("extra")];
        let labels: Vec<String> = ordered("Chart", &chart).into_iter().map(|(_, label)| label).collect();
        assert_eq!(labels, ["Kind", "Title", "Labels", "Values", "Data file", "Build up by clicks", "Extra"]);
        let own = [field("team_name"), field("points")];
        let labels: Vec<String> = ordered("Score", &own).into_iter().map(|(_, label)| label).collect();
        assert_eq!(labels, ["Team name", "Points"]);
    }

    #[test]
    fn a_built_ins_colours_are_colours() {
        let shape = [PropField::new("fill", PropKind::Text, true), PropField::new("kind", PropKind::Text, false)];
        let kinds: Vec<PropKind> = ordered("Shape", &shape).into_iter().map(|(prop, _)| prop.kind).collect();
        assert_eq!(kinds, [PropKind::Text, PropKind::Color]);
        let own = [PropField::new("fill", PropKind::Text, true)];
        assert_eq!(ordered("Score", &own)[0].0.kind, PropKind::Text, "a talk's own says what it takes");
    }

    #[test]
    fn a_shapes_kinds_have_their_own_names() {
        assert_eq!(choice("Shape", "kind", "rect"), "Rectangle");
        assert_eq!(choice("Shape", "kind", "ellipse"), "Ellipse");
        assert_eq!(choice("Chart", "kind", "rect"), "Rect");
        assert_eq!(attribute("outline_width"), "outline-width");
    }
}
