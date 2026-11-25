//! One setting of the thing selected: what kind of field shows it, and what's written when it's
//! changed. What's typed is worked out here, away from the window, so it can be tested alone.

use gpui_kit::SharedString;
use gpui_kit::component::searchable_list::SearchableListItem;
use toolkit::{PropField, PropKind};

use super::labels;

/// What's said under a field whose text can't be written as an attribute.
pub const BOTH_QUOTES: &str = "That can't hold both kinds of quote mark or a new line.";
/// What's said under a number field that holds something else.
pub const NOT_A_NUMBER: &str = "That has to be a number.";
/// What's said under a whole-number field that holds a fraction.
pub const NOT_WHOLE: &str = "That has to be a whole number.";
/// What's said under a text field that holds a tab or another control character.
pub const CONTROL: &str = "That can't hold a tab or other control character.";

/// The furthest from 0 a number with no range of its own is held to.
pub const NUMBER_LIMIT: f64 = 1_000_000.0;

/// The farthest a thing can be placed from the slide's corner, in slide units, either way, as
/// the engine holds it.
pub const MIN_POSITION: f64 = slides_engine::MIN_POSITION as f64;
pub const MAX_POSITION: f64 = slides_engine::MAX_POSITION as f64;
/// The largest a thing can be, as the engine holds it.
pub const MAX_SIZE: f64 = slides_engine::MAX_SIZE as f64;
/// The most a thing can be turned, either way, in degrees.
pub const MAX_TURN: f64 = slides_engine::MAX_ROTATION as f64;

/// The fields that place a thing on the slide: its attribute, its label and what it's held to.
pub const PLACE: [(&str, &str, (f64, f64)); 5] = [
    ("x", "Left", (MIN_POSITION, MAX_POSITION)),
    ("y", "Top", (MIN_POSITION, MAX_POSITION)),
    ("w", "Width", (1.0, MAX_SIZE)),
    ("h", "Height", (1.0, MAX_SIZE)),
    ("rotate", "Turn", (-MAX_TURN, MAX_TURN)),
];

/// What kind of field a setting has.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Text,
    Number { whole: bool, range: Option<(f64, f64)> },
    Flag,
    Color,
    Choice(Vec<String>),
    File(Vec<String>),
    /// Which of the bundled icons, chosen from the icon picker.
    Icon,
    /// A video's still: its first frame, or one of these pictures of the talk's, by their paths.
    Still(Vec<String>),
}

/// A setting as the form shows it: the attribute it writes, its label, its kind, whether it can
/// be left out, and whether it's one of the fields that place the thing.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub attr: String,
    pub label: String,
    pub kind: Kind,
    pub optional: bool,
    pub place: bool,
    /// What's said under the field of what it does, if anything is.
    pub note: Option<String>,
}

impl Spec {
    /// The field for `prop` of `tag`'s component, labelled `label`. A kind this app doesn't
    /// know of yet is typed as text. The built-in icon's name is chosen from the icon picker.
    pub fn of(tag: &str, prop: &PropField, label: String) -> Spec {
        let kind = match &prop.kind {
            PropKind::Text if tag == "Icon" && prop.name == "name" => Kind::Icon,
            PropKind::Whole => Kind::Number { whole: true, range: None },
            PropKind::Number => Kind::Number { whole: false, range: None },
            PropKind::Flag => Kind::Flag,
            PropKind::Color => Kind::Color,
            PropKind::Choice(options) => Kind::Choice(options.clone()),
            PropKind::File(extensions) => Kind::File(extensions.iter().map(|ext| ext.to_ascii_lowercase()).collect()),
            _ => Kind::Text,
        };
        Spec { attr: labels::attribute(&prop.name), label, kind, optional: prop.optional, place: false, note: None }
    }

    /// The fields that place a thing.
    pub fn place() -> impl Iterator<Item = Spec> {
        PLACE.into_iter().map(|(attr, label, range)| Spec {
            attr: attr.to_string(),
            label: label.to_string(),
            kind: Kind::Number { whole: false, range: Some(range) },
            optional: true,
            place: true,
            note: None,
        })
    }
}

/// What a video's still field calls its first frame, which is written as no still at all.
pub const FIRST_FRAME: &str = "First frame";

/// One choice of a choice field: the value written, and what it's called.
#[derive(Clone)]
pub struct Choice {
    pub value: String,
    pub title: SharedString,
}

impl SearchableListItem for Choice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &String {
        &self.value
    }
}

/// What a field's committed text comes to.
#[derive(Clone, Debug, PartialEq)]
pub enum Typed {
    /// Write this, or take the attribute away.
    Write(Option<String>),
    /// Write nothing, and say this under the field.
    Refused(&'static str),
    /// Write nothing, and show the talk's value again.
    PutBack,
}

/// Why `value` can't be written as an attribute's value, if it can't: both kinds of quote mark
/// or a new line, or a tab or another control character.
pub fn unwritable(value: &str) -> Option<&'static str> {
    if (value.contains('"') && value.contains('\'')) || value.contains(['\n', '\r']) {
        Some(BOTH_QUOTES)
    } else if value.chars().any(char::is_control) {
        Some(CONTROL)
    } else {
        None
    }
}

/// What text typed in a text field comes to. Emptied, an optional one is taken away, and a
/// required one is written empty.
pub fn text_typed(text: &str, optional: bool) -> Typed {
    if let Some(why) = unwritable(text) {
        Typed::Refused(why)
    } else if text.is_empty() {
        Typed::Write((!optional).then(String::new))
    } else {
        Typed::Write(Some(text.to_string()))
    }
}

/// What text typed in a number field comes to: the number, held to `range`, or refused. A
/// fraction is refused in a `whole` one. Emptied, an optional one is taken away, and a required
/// one shows the talk's value again.
pub fn number_typed(text: &str, whole: bool, range: Option<(f64, f64)>, optional: bool) -> Typed {
    let text = text.trim();
    if text.is_empty() {
        return if optional { Typed::Write(None) } else { Typed::PutBack };
    }
    match number_in(text, whole) {
        Ok(value) => Typed::Write(Some(number_text(held(value, range)))),
        Err(why) => Typed::Refused(why),
    }
}

/// The number `text` says: digits, perhaps a sign in front, and at most one decimal mark, a dot
/// or a comma, so `1,5` is one and a half. Anything else, an exponent such as `1e3` included,
/// isn't a number.
pub fn parse_number(text: &str) -> Result<f64, &'static str> {
    let text = text.trim();
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    let marks = unsigned.chars().filter(|c| matches!(c, '.' | ',')).count();
    let readable = marks <= 1
        && unsigned.chars().any(|c| c.is_ascii_digit())
        && unsigned.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ','));
    if !readable {
        return Err(NOT_A_NUMBER);
    }
    unsigned
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|value| if text.starts_with('-') { -value } else { value })
        .ok_or(NOT_A_NUMBER)
}

/// The number `text` says, as [`parse_number`] reads it, refused if it's a fraction where a
/// `whole` one is wanted.
fn number_in(text: &str, whole: bool) -> Result<f64, &'static str> {
    let value = parse_number(text)?;
    if whole && value.fract() != 0.0 { Err(NOT_WHOLE) } else { Ok(value) }
}

/// The number one step up or down from what a number field holds, `text`, held to `range`, or
/// from `talk`, the talk's value, when the field is empty; from 0 if both are. Something that
/// isn't a number, or a fraction in a `whole` one, is refused, and nothing is stepped.
pub fn stepped(text: &str, talk: &str, up: bool, whole: bool, range: Option<(f64, f64)>) -> Result<String, &'static str> {
    let from = if text.trim().is_empty() { talk } else { text };
    // In the rounded terms it's written in, so 1.1 steps down to 0.1.
    let value = if from.trim().is_empty() { 0.0 } else { rounded(number_in(from, whole)?) };
    Ok(number_text(held(if up { value + 1.0 } else { value - 1.0 }, range)))
}

/// `value` held to `range`, or to [`NUMBER_LIMIT`] either way for a number with no range of
/// its own.
fn held(value: f64, range: Option<(f64, f64)>) -> f64 {
    let (low, high) = range.unwrap_or((-NUMBER_LIMIT, NUMBER_LIMIT));
    value.clamp(low, high)
}

/// The most decimal places a number is written with.
pub const DECIMALS: i32 = 4;

/// `value` rounded to [`DECIMALS`] places, as it's written.
pub fn rounded(value: f64) -> f64 {
    let scale = 10f64.powi(DECIMALS);
    let rounded = (value * scale).round() / scale;
    // Never -0.
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// `value` as it's written: rounded to [`DECIMALS`] places, in as few digits as say that, with
/// a dot, and never `-0`.
pub fn number_text(value: f64) -> String {
    rounded(value).to_string()
}

/// The name of the file at `path` in the talk, without the folder it's kept in.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_written_as_typed_or_refused() {
        assert_eq!(text_typed("Sales", false), Typed::Write(Some("Sales".into())));
        assert_eq!(text_typed("Bob's", false), Typed::Write(Some("Bob's".into())));
        assert_eq!(text_typed("\"Hi\" Bob's", false), Typed::Refused(BOTH_QUOTES));
        assert_eq!(text_typed("a\nb", false), Typed::Refused(BOTH_QUOTES));
        assert_eq!(text_typed("a\0b", false), Typed::Refused(CONTROL));
        assert_eq!(text_typed("a\tb", false), Typed::Refused(CONTROL));
        assert_eq!(text_typed("a\rb", false), Typed::Refused(BOTH_QUOTES));
        assert_eq!(text_typed("", false), Typed::Write(Some(String::new())));
        assert_eq!(text_typed("", true), Typed::Write(None));
    }

    #[test]
    fn numbers_are_held_to_their_range() {
        let place = Some((MIN_POSITION, MAX_POSITION));
        assert_eq!(number_typed("9000", false, place, true), Typed::Write(Some("7680".into())));
        assert_eq!(number_typed(" -5 ", false, place, true), Typed::Write(Some("-5".into())));
        assert_eq!(number_typed("-9000", false, place, true), Typed::Write(Some("-3840".into())));
        assert_eq!(number_typed("12.5", false, None, false), Typed::Write(Some("12.5".into())));
        assert_eq!(number_typed("100.0", false, None, false), Typed::Write(Some("100".into())));
        assert_eq!(number_typed("-0", false, None, false), Typed::Write(Some("0".into())));
        assert_eq!(number_typed("2.5", true, None, false), Typed::Refused(NOT_WHOLE));
        assert_eq!(number_typed("4", true, None, false), Typed::Write(Some("4".into())));
        assert_eq!(number_typed("abc", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("inf", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("1e999", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("1e3", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("1,5", false, None, false), Typed::Write(Some("1.5".into())));
        assert_eq!(number_typed("-0,25", false, None, false), Typed::Write(Some("-0.25".into())));
        assert_eq!(number_typed("1.2,5", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("1,5", true, None, false), Typed::Refused(NOT_WHOLE));
        assert_eq!(number_typed("--1", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed(",", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("1e300", false, None, false), Typed::Refused(NOT_A_NUMBER));
        let huge = format!("1{}", "0".repeat(300));
        assert_eq!(number_typed(&huge, true, None, false), Typed::Write(Some("1000000".into())));
        assert_eq!(number_typed("-5000000", false, None, false), Typed::Write(Some("-1000000".into())));
        assert_eq!(number_typed("", false, None, true), Typed::Write(None));
        assert_eq!(number_typed("", false, None, false), Typed::PutBack);
    }

    #[test]
    fn a_step_goes_one_up_or_down_within_the_range() {
        assert_eq!(stepped("560", "", true, false, None), Ok("561".into()));
        assert_eq!(stepped("", "", false, false, None), Ok("-1".into()));
        assert_eq!(stepped("", "96", true, false, None), Ok("97".into()), "an empty field steps from the talk's");
        assert_eq!(stepped("7680", "", true, false, Some((0.0, MAX_POSITION))), Ok("7680".into()));
        assert_eq!(stepped("1.5", "", false, false, Some((1.0, MAX_SIZE))), Ok("1".into()));
        assert_eq!(stepped("1,5", "", true, false, None), Ok("2.5".into()));
        assert_eq!(stepped("abc", "4", true, false, None), Err(NOT_A_NUMBER), "never from 0");
        assert_eq!(stepped("1,5", "", true, true, None), Err(NOT_WHOLE));
    }

    #[test]
    fn a_number_is_written_with_at_most_four_places() {
        assert_eq!(stepped("1.1", "", false, false, None), Ok("0.1".into()));
        assert_eq!(stepped("0,3", "", true, false, None), Ok("1.3".into()));
        assert_eq!(number_typed("0.123456", false, None, false), Typed::Write(Some("0.1235".into())));
        assert_eq!(number_typed("2.50000", false, None, false), Typed::Write(Some("2.5".into())));
        assert_eq!(number_typed("-0.00001", false, None, false), Typed::Write(Some("0".into())));
    }

    proptest::proptest! {
        #[test]
        fn what_is_written_reads_back_rounded_and_writes_the_same_again(value in -NUMBER_LIMIT..NUMBER_LIMIT, comma: bool) {
            let typed = if comma { value.to_string().replace('.', ",") } else { value.to_string() };
            let Typed::Write(Some(written)) = number_typed(&typed, false, None, false) else {
                panic!("{typed} is a number");
            };
            proptest::prop_assert_eq!(written.parse::<f64>().unwrap(), rounded(value), "{} gave {}", typed, written);
            proptest::prop_assert!(!written.contains(['e', ',']));
            let decimals = written.split_once('.').map_or(0, |(_, places)| places.len());
            proptest::prop_assert!(decimals <= 4, "{}", written);
            proptest::prop_assert_eq!(number_typed(&written, false, None, false), Typed::Write(Some(written.clone())));
        }
    }

    #[test]
    fn the_built_in_icons_name_is_chosen_from_the_picker() {
        let name = PropField::new("name", PropKind::Text, false);
        assert_eq!(Spec::of("Icon", &name, "Icon".into()).kind, Kind::Icon);
        assert_eq!(Spec::of("Score", &name, "Name".into()).kind, Kind::Text, "a talk's own is typed");
        let size = PropField::new("size", PropKind::Number, true);
        assert_eq!(Spec::of("Icon", &size, "Size".into()).kind, Kind::Number { whole: false, range: None });
    }

    #[test]
    fn a_file_is_named_without_its_folder() {
        assert_eq!(file_name("assets/q3-sales.csv"), "q3-sales.csv");
        assert_eq!(file_name("photo.png"), "photo.png");
        assert_eq!(file_name(""), "");
    }
}
