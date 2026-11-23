//! What the inspector calls each setting, and the order the built-in components show theirs in.
//! A talk's own component's settings are named from its arguments, in the order it gives them.

use toolkit::{PropField, PropKind};

/// A built-in's setting: `(argument, label, meaning)`.
type Setting = (&'static str, &'static str, &'static str);

/// A built-in component's settings, by argument, in the order they're shown, with their labels
/// and what each means: `(argument, label, meaning)`.
const BUILT_INS: [(&str, &[Setting]); 8] = [
    (
        "Shape",
        &[
            ("kind", "Shape", "Which shape to draw."),
            ("fill", "Fill", "The colour inside the shape, as `#rrggbb`."),
            ("outline", "Outline", "The colour of the line round the shape, as `#rrggbb`."),
            ("outline-width", "Outline width", "How thick the outline is, in slide units."),
        ],
    ),
    (
        "Box",
        &[
            ("fill", "Fill", "The colour inside the box, as `#rrggbb`."),
            ("outline", "Outline", "The colour of the line round the box, as `#rrggbb`."),
            ("outline-width", "Outline width", "How thick the outline is, in slide units."),
            ("radius", "Corner radius", "How round the box's corners are, in slide units."),
            ("padding", "Padding", "How much room is left between the box's edge and what is inside it, in slide units."),
        ],
    ),
    (
        "Icon",
        &[
            ("name", "Icon", "The name of the icon to draw."),
            ("size", "Size", "How big the icon is, in slide units."),
            ("color", "Colour", "The icon's colour, as `#rrggbb`."),
        ],
    ),
    ("Picture", &[("src", "Picture", "The picture to show, a picture file in the talk, such as `assets/logo.png`.")]),
    (
        "Video",
        &[
            ("loop", "Loop", "Plays the video again from the start when it ends."),
            ("poster", "Still picture", "A picture to show in place of the video's first frame."),
        ],
    ),
    ("Model", &[("turn", "Turns on its own", "`auto` turns the model slowly on its own while the slide shows.")]),
    // Its place alone: its file is named above, and put on the slide from the Insert tab.
    ("Terminal", &[]),
    (
        "Chart",
        &[
            ("kind", "Kind", "Which chart to draw: `bar`, `line` or `pie`."),
            ("title", "Title", "A heading to show above the chart."),
            ("labels", "Labels", "The name of each bar, point or slice, set apart by commas."),
            ("values", "Values", "The number for each label, set apart by commas."),
            ("data", "Data file", "A CSV file in the talk to take the labels and values from instead."),
            ("build", "Build up by clicks", "Shows the chart a bar, point or slice at a time, one for each click."),
        ],
    ),
];

/// The built-ins' arguments the form leaves out: `(tag, argument, meaning)`. A video's, a model's
/// or a terminal recording's file is named above its fields, and a new one is put on the slide
/// from the Insert tab; a model's views are set by turning it, and counted under its fields.
const LEFT_OUT: [(&str, &str, &str); 5] = [
    ("Video", "src", "The video to play, an MP4 file in the talk, such as `assets/demo.mp4`."),
    ("Model", "src", "The 3D model to show, a `.glb` or `.ply` file in the talk."),
    ("Model", "view", "The view to start from, six numbers set apart by spaces: yaw and pitch in degrees, the distance, then the x, y and z of the point looked at."),
    ("Model", "views", "Views to move through, one for each click, each written as `view` is, with a semicolon between views."),
    ("Terminal", "src", "The terminal recording to play, an asciinema `.cast` file in the talk."),
];

/// What the built-in `tag`'s setting `argument` means, in a plain sentence, or `None` for a
/// setting it doesn't have.
pub fn meaning(tag: &str, argument: &str) -> Option<&'static str> {
    let (_, known) = BUILT_INS.iter().find(|(name, _)| *name == tag)?;
    known
        .iter()
        .find(|(name, _, _)| *name == argument)
        .map(|(_, _, meaning)| *meaning)
        .or_else(|| LEFT_OUT.iter().find(|(t, a, _)| *t == tag && *a == argument).map(|(_, _, meaning)| *meaning))
}

/// The built-ins' arguments that are ticked or not, though they take a word: `(tag, argument)`.
/// A model's `turn` is `auto`, ticked, or left out.
const FLAGS: [(&str, &str); 1] = [("Model", "turn")];

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
        .filter_map(|(name, label, _)| {
            let prop = props.iter().find(|prop| prop.name == *name)?;
            let colour = COLOURS.contains(&(tag, prop.name.as_str())) && prop.kind == PropKind::Text;
            let flag = FLAGS.contains(&(tag, prop.name.as_str()));
            let prop = match () {
                _ if colour => PropField::new(prop.name.clone(), PropKind::Color, prop.optional),
                _ if flag => PropField::new(prop.name.clone(), PropKind::Flag, prop.optional),
                _ => prop.clone(),
            };
            Some((prop, label.to_string()))
        })
        .collect();
    for prop in props {
        if !known.iter().any(|(name, _, _)| *name == prop.name) && !LEFT_OUT.iter().any(|(t, a, _)| *t == tag && *a == prop.name) {
            ordered.push((prop.clone(), plain(&prop.name)));
        }
    }
    ordered
}

/// Whether `value`, what `tag` has as a yes-or-no setting, says yes. A video's `loop` says yes
/// written bare, or as `true`, `yes` or `1`, and a model's `turn` as `auto`, as the engine reads
/// them; anything else's only as `true`.
pub fn says_yes(tag: &str, value: Option<&str>) -> bool {
    match value {
        Some(value) if tag == "Video" => matches!(value.trim().to_ascii_lowercase().as_str(), "" | "true" | "yes" | "1"),
        Some(value) if tag == "Model" => value.trim().eq_ignore_ascii_case("auto"),
        value => value == Some("true"),
    }
}

/// What a yes-or-no setting of `tag` is written as, ticked if `yes`, or `None` to leave it out:
/// a model turns on its own as `auto`, and stays still with none.
pub fn written_flag(tag: &str, yes: bool) -> Option<&'static str> {
    match (tag, yes) {
        ("Model", true) => Some("auto"),
        ("Model", false) => None,
        (_, true) => Some("true"),
        (_, false) => Some("false"),
    }
}

/// What the form says of a 3D model of `kind`, of `triangles` or `points`: "42,000 triangles",
/// or "1.2 million points".
pub fn model_facts(kind: slides_model::ModelKind, triangles: u64, points: u64) -> String {
    let (count, one) = match kind {
        slides_model::ModelKind::Mesh => (triangles, "triangle"),
        slides_model::ModelKind::Points => (points, "point"),
    };
    match count {
        0 => format!("No {one}s"),
        1 => format!("1 {one}"),
        count if count < 1_000_000 => format!("{} {one}s", with_commas(count)),
        count => {
            let tenths = (count + 50_000) / 100_000;
            let millions = if tenths % 10 == 0 { (tenths / 10).to_string() } else { format!("{}.{}", tenths / 10, tenths % 10) };
            format!("{millions} million {one}s")
        }
    }
}

/// `count` with a comma between each three figures: "42,000".
fn with_commas(count: u64) -> String {
    let figures = count.to_string();
    let mut written = String::new();
    for (at, figure) in figures.chars().enumerate() {
        if at > 0 && (figures.len() - at).is_multiple_of(3) {
            written.push(',');
        }
        written.push(figure);
    }
    written
}

/// What the form says of `views` views: "No views", "1 view", "3 views".
pub fn views_counted(views: usize) -> String {
    match views {
        0 => "No views".to_string(),
        1 => "1 view".to_string(),
        views => format!("{views} views"),
    }
}

/// What the form says of a terminal recording as `facts` give it: its screen's size, how long it
/// plays and how many times it stops, "80 by 24, 3 min 20 s, 2 stops".
pub fn terminal_facts(facts: slides_terminal::RecordingFacts) -> String {
    let whole = facts.length.as_secs_f64().round() as u64;
    let length = if whole == 0 {
        "under a second".to_string()
    } else {
        let parts = [(whole / 3600, "h"), (whole / 60 % 60, "min"), (whole % 60, "s")];
        let said: Vec<String> = parts.iter().filter(|(count, _)| *count > 0).map(|(count, unit)| format!("{count} {unit}")).collect();
        said.join(" ")
    };
    let stops = match facts.marks {
        0 => "no stops".to_string(),
        1 => "1 stop".to_string(),
        marks => format!("{marks} stops"),
    };
    format!("{} by {}, {length}, {stops}", facts.width, facts.height)
}

/// What the form says of a video `seconds` long and `size` pixels big, as its pictures are
/// shown: "14 seconds, 1120 by 630".
pub fn video_facts(seconds: f64, size: (u32, u32)) -> String {
    format!("{}, {} by {}", length(seconds), size.0, size.1)
}

/// `seconds` as words, to the nearest second: "2 minutes 5 seconds", or "Under a second".
fn length(seconds: f64) -> String {
    let whole = if seconds.is_finite() { seconds.max(0.0).round() as u64 } else { 0 };
    if whole == 0 {
        return "Under a second".to_string();
    }
    let counted = |count: u64, one: &str| if count == 1 { format!("1 {one}") } else { format!("{count} {one}s") };
    let (hours, minutes, seconds) = (whole / 3600, whole / 60 % 60, whole % 60);
    let parts = [(hours, "hour"), (minutes, "minute"), (seconds, "second")];
    let said: Vec<String> = parts.iter().filter(|(count, _)| *count > 0).map(|(count, one)| counted(*count, one)).collect();
    said.join(" ")
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
    fn every_built_in_setting_has_a_meaning() {
        let registry = slides_engine::builtin_registry();
        assert_eq!(registry.names().len(), 8);
        for tag in registry.names() {
            let schema = (registry.get(tag).expect("a built-in").schema)();
            for prop in &schema.props {
                let said = meaning(tag, &prop.name);
                assert!(said.is_some_and(|said| !said.is_empty()), "{tag} {} has no meaning", prop.name);
            }
        }
    }

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
    fn a_videos_form_names_its_still_and_leaves_its_file_out() {
        let props = [
            PropField::new("src", PropKind::File(vec!["mp4".into()]), false),
            PropField::new("poster", PropKind::File(vec!["png".into()]), true),
            PropField::new("loop", PropKind::Flag, true),
        ];
        let labels: Vec<String> = ordered("Video", &props).into_iter().map(|(_, label)| label).collect();
        assert_eq!(labels, ["Loop", "Still picture"]);
    }

    #[test]
    fn a_videos_loop_says_yes_as_the_engine_reads_it() {
        for yes in ["", "true", "yes", "YES", "1"] {
            assert!(says_yes("Video", Some(yes)), "{yes}");
        }
        for no in ["false", "no", "0", "sometimes"] {
            assert!(!says_yes("Video", Some(no)), "{no}");
        }
        assert!(!says_yes("Video", None));
        assert!(says_yes("Chart", Some("true")));
        assert!(!says_yes("Chart", Some("yes")), "a component's own say");
    }

    #[test]
    fn a_models_form_ticks_turning_and_leaves_its_file_and_views_out() {
        let props = [
            PropField::new("src", PropKind::File(vec!["glb".into()]), false),
            PropField::new("turn", PropKind::Choice(vec!["auto".into()]), true),
            PropField::new("view", PropKind::Text, true),
            PropField::new("views", PropKind::Text, true),
        ];
        let ordered = ordered("Model", &props);
        let labels: Vec<&str> = ordered.iter().map(|(_, label)| label.as_str()).collect();
        assert_eq!(labels, ["Turns on its own"]);
        assert_eq!(ordered[0].0.kind, PropKind::Flag);
        assert!(says_yes("Model", Some("auto")) && says_yes("Model", Some(" AUTO ")));
        assert!(!says_yes("Model", None) && !says_yes("Model", Some("true")));
        assert_eq!((written_flag("Model", true), written_flag("Model", false)), (Some("auto"), None));
        assert_eq!((written_flag("Video", true), written_flag("Video", false)), (Some("true"), Some("false")));
        assert_eq!([0, 1, 256].map(views_counted), ["No views", "1 view", "256 views"]);
    }

    #[test]
    fn a_shapes_kinds_have_their_own_names() {
        assert_eq!(choice("Shape", "kind", "rect"), "Rectangle");
        assert_eq!(choice("Shape", "kind", "ellipse"), "Ellipse");
        assert_eq!(choice("Chart", "kind", "rect"), "Rect");
        assert_eq!(attribute("outline_width"), "outline-width");
    }
}
