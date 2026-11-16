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

/// The farthest a thing can be placed from the slide's corner, in slide units, as the engine
/// holds it.
pub const MAX_POSITION: f64 = 7680.0;
/// The largest a thing can be, as the engine holds it.
pub const MAX_SIZE: f64 = 3840.0;
/// The most a thing can be turned, either way, in degrees.
pub const MAX_TURN: f64 = 360.0;

/// The fields that place a thing on the slide: its attribute, its label and what it's held to.
pub const PLACE: [(&str, &str, (f64, f64)); 5] = [
    ("x", "Left", (0.0, MAX_POSITION)),
    ("y", "Top", (0.0, MAX_POSITION)),
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
}

impl Spec {
    /// The field for `prop` of `tag`'s component, labelled `label`. A kind this app doesn't
    /// know of yet is typed as text.
    pub fn of(prop: &PropField, label: String) -> Spec {
        let kind = match &prop.kind {
            PropKind::Whole => Kind::Number { whole: true, range: None },
            PropKind::Number => Kind::Number { whole: false, range: None },
            PropKind::Flag => Kind::Flag,
            PropKind::Color => Kind::Color,
            PropKind::Choice(options) => Kind::Choice(options.clone()),
            PropKind::File(extensions) => Kind::File(extensions.iter().map(|ext| ext.to_ascii_lowercase()).collect()),
            _ => Kind::Text,
        };
        Spec { attr: labels::attribute(&prop.name), label, kind, optional: prop.optional, place: false }
    }

    /// The fields that place a thing.
    pub fn place() -> impl Iterator<Item = Spec> {
        PLACE.into_iter().map(|(attr, label, range)| Spec {
            attr: attr.to_string(),
            label: label.to_string(),
            kind: Kind::Number { whole: false, range: Some(range) },
            optional: true,
            place: true,
        })
    }
}

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

/// Whether `value` can be written as an attribute's value: not both kinds of quote mark, and no
/// new line or other control character.
pub fn writable(value: &str) -> bool {
    !(value.contains('"') && value.contains('\'')) && !value.chars().any(char::is_control)
}

/// What text typed in a text field comes to. Emptied, an optional one is taken away, and a
/// required one is written empty.
pub fn text_typed(text: &str, optional: bool) -> Typed {
    if !writable(text) {
        Typed::Refused(BOTH_QUOTES)
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
    let Some(value) = text.parse::<f64>().ok().filter(|value| value.is_finite()) else {
        return Typed::Refused(NOT_A_NUMBER);
    };
    if whole && value.fract() != 0.0 {
        return Typed::Refused(NOT_WHOLE);
    }
    Typed::Write(Some(number_text(held(value, range))))
}

/// The number one step up or down from what a number field holds, `text`, held to `range`.
/// Something that isn't a number steps from 0.
pub fn stepped(text: &str, up: bool, range: Option<(f64, f64)>) -> String {
    let value = text.trim().parse::<f64>().ok().filter(|value| value.is_finite()).unwrap_or(0.0);
    number_text(held(if up { value + 1.0 } else { value - 1.0 }, range))
}

fn held(value: f64, range: Option<(f64, f64)>) -> f64 {
    range.map_or(value, |(low, high)| value.clamp(low, high))
}

/// `value` as it's written: as few digits as say it, and never `-0`.
pub fn number_text(value: f64) -> String {
    if value == 0.0 { "0".to_string() } else { value.to_string() }
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
        assert_eq!(text_typed("a\0b", false), Typed::Refused(BOTH_QUOTES));
        assert_eq!(text_typed("", false), Typed::Write(Some(String::new())));
        assert_eq!(text_typed("", true), Typed::Write(None));
    }

    #[test]
    fn numbers_are_held_to_their_range() {
        let place = Some((0.0, MAX_POSITION));
        assert_eq!(number_typed("9000", false, place, true), Typed::Write(Some("7680".into())));
        assert_eq!(number_typed(" -5 ", false, place, true), Typed::Write(Some("0".into())));
        assert_eq!(number_typed("12.5", false, None, false), Typed::Write(Some("12.5".into())));
        assert_eq!(number_typed("100.0", false, None, false), Typed::Write(Some("100".into())));
        assert_eq!(number_typed("-0", false, None, false), Typed::Write(Some("0".into())));
        assert_eq!(number_typed("2.5", true, None, false), Typed::Refused(NOT_WHOLE));
        assert_eq!(number_typed("4", true, None, false), Typed::Write(Some("4".into())));
        assert_eq!(number_typed("abc", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("inf", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("1e999", false, None, false), Typed::Refused(NOT_A_NUMBER));
        assert_eq!(number_typed("", false, None, true), Typed::Write(None));
        assert_eq!(number_typed("", false, None, false), Typed::PutBack);
    }

    #[test]
    fn a_step_goes_one_up_or_down_within_the_range() {
        assert_eq!(stepped("560", true, None), "561");
        assert_eq!(stepped("", false, None), "-1");
        assert_eq!(stepped("7680", true, Some((0.0, MAX_POSITION))), "7680");
        assert_eq!(stepped("1.5", false, Some((1.0, MAX_SIZE))), "1");
    }

    #[test]
    fn a_file_is_named_without_its_folder() {
        assert_eq!(file_name("assets/q3-sales.csv"), "q3-sales.csv");
        assert_eq!(file_name("photo.png"), "photo.png");
        assert_eq!(file_name(""), "");
    }
}
