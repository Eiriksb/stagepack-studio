//! What a component's source may not say, checked before `rustc` ever sees it. This is the
//! line between a talk and the person's files: a compile-time read would copy a private
//! file into a build that the talk can then carry away.
//!
//! It works on tokens, so words in strings and comments don't count, and it walks into
//! every group, including the input of macros. For that to be enough, what the tokens say
//! has to be what `rustc` will do, which is why a file that won't lex is refused, a file
//! `rustc` would read differently from the lexer here is refused, and so is `macro_rules`.
//! Names are refused unless they're ASCII, literal suffixes and lifetimes included: the
//! lexer here knows a newer Unicode than `rustc` does, and takes characters into a name
//! that `rustc` drops, so `include_str` followed by U+0558 is one unknown name here, and
//! `include_str` there.

use std::str::FromStr;

use proc_macro2::{Delimiter, Group, Ident, Literal, Span, TokenStream, TokenTree};

use crate::Diagnostic;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInfo {
    pub tag: String,
}

/// Why a word can't appear anywhere at all. It's checked in macro input too, as a macro
/// can turn what it was given back into code.
fn why_never_allowed(word: &str) -> Option<&'static str> {
    Some(match word {
        "include" | "include_str" | "include_bytes" | "debugger_visualizer" => "it could read files from this computer",
        "env" | "option_env" => "it could read settings from this computer",
        // Assembly's `.incbin` reads a file. `rustc` doesn't allow assembly on WebAssembly
        // yet, but what this check lets through shouldn't rest on that.
        "asm" | "global_asm" | "naked_asm" => "assembly could read files from this computer",
        // A macro can put tokens together in an order this check never sees: both
        // `m!(mod a {})` and `m!(path)` can come out as `#[path = "x"] mod a;`.
        "macro_rules" | "macro" => "a macro could put together something that reads this computer",
        _ => return None,
    })
}

fn line_of(span: Span) -> u32 {
    span.start().line as u32
}

fn is_punct(tree: &TokenTree, wanted: char) -> bool {
    matches!(tree, TokenTree::Punct(punct) if punct.as_char() == wanted)
}

fn is_group(tree: Option<&TokenTree>, wanted: Delimiter) -> bool {
    matches!(tree, Some(TokenTree::Group(group)) if group.delimiter() == wanted)
}

/// An identifier's name, the same whether or not it was written `r#name`.
fn word_of(ident: &Ident) -> String {
    let name = ident.to_string();
    match name.strip_prefix("r#") {
        Some(plain) => plain.to_string(),
        None => name,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Place {
    Code,
    /// Directly inside `#[...]`, or somewhere deeper in one.
    Attribute,
    /// Directly inside `#![...]`.
    InnerAttribute,
}

/// A place for the group at `index`, given the tokens around it.
fn place_of_group(trees: &[TokenTree], index: usize, around: Place) -> Place {
    let is_attribute = is_group(trees.get(index), Delimiter::Bracket) && index > 0;
    if is_attribute && is_punct(&trees[index - 1], '#') {
        Place::Attribute
    } else if is_attribute && index > 1 && is_punct(&trees[index - 1], '!') && is_punct(&trees[index - 2], '#') {
        Place::InnerAttribute
    } else if around == Place::Code {
        Place::Code
    } else {
        Place::Attribute
    }
}

/// True of `component`, and of a path ending in it such as `toolkit::component`.
fn names_component(attribute: &Group) -> bool {
    let trees: Vec<TokenTree> = attribute.stream().into_iter().collect();
    let is_path = trees.iter().all(|tree| matches!(tree, TokenTree::Ident(_)) || is_punct(tree, ':'));
    is_path && matches!(trees.last(), Some(TokenTree::Ident(last)) if last == "component")
}

/// The name of the function that the attribute just before `start` is on.
fn function_after(trees: &[TokenTree], start: usize) -> Option<&Ident> {
    for (index, tree) in trees.iter().enumerate().skip(start) {
        match tree {
            TokenTree::Ident(word) if word == "fn" => {
                return match trees.get(index + 1) {
                    Some(TokenTree::Ident(name)) => Some(name),
                    _ => None,
                };
            }
            // The end of whatever this is, and it had no `fn`.
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => return None,
            tree if is_punct(tree, ';') => return None,
            _ => {}
        }
    }
    None
}

const ONLY_ASCII_NAMES: &str = "Names in a component can only use ASCII letters, digits and underscores";

/// The suffix of a literal, such as `u8` in `1u8`, which is a name too. For a number it's
/// all of the literal, which is fine here, as only whether it's ASCII is asked.
fn suffix_of(literal: &Literal) -> String {
    let text = literal.to_string();
    // A suffix can't hold a quote, so the last one closes the string or character.
    match text.rfind(['"', '\'']) {
        Some(end) => text[end + 1..].trim_start_matches('#').to_string(),
        None => text,
    }
}

struct Found {
    at: (u32, u32),
    message: String,
}

struct FoundComponent {
    at: (u32, u32),
    name: String,
}

#[derive(Default)]
struct Scan {
    refusals: Vec<Found>,
    components: Vec<FoundComponent>,
}

impl Scan {
    fn refuse(&mut self, span: Span, message: String) {
        let start = span.start();
        self.refusals.push(Found { at: (start.line as u32, start.column as u32), message });
    }

    fn read_word(&mut self, ident: &Ident, trees: &[TokenTree], index: usize, place: Place) {
        let word = word_of(ident);
        if !word.is_ascii() {
            self.refuse(ident.span(), ONLY_ASCII_NAMES.to_string());
            return;
        }
        if let Some(reason) = why_never_allowed(&word) {
            self.refuse(ident.span(), format!("`{word}` can't be used in a component, as {reason}"));
        }
        match word.as_str() {
            "mod" => {
                let named = matches!(trees.get(index + 1), Some(TokenTree::Ident(_)));
                if !named || !is_group(trees.get(index + 2), Delimiter::Brace) {
                    self.refuse(
                        ident.span(),
                        "A `mod` in a component needs a name and a `{ }` body, as `mod name;` would read another file from this computer".to_string(),
                    );
                }
            }
            // `link` hands its name to the linker, which goes looking for it.
            "path" | "link" if place != Place::Code => {
                self.refuse(
                    ident.span(),
                    format!("`{word}` can't be used in an attribute in a component, as it could read files from this computer"),
                );
            }
            // Both spellings, as `#![cfg_attr(all(), feature(x))]` doesn't start with it.
            "feature"
                if (place == Place::InnerAttribute && index == 0)
                    || (place != Place::Code && is_group(trees.get(index + 1), Delimiter::Parenthesis)) =>
            {
                self.refuse(ident.span(), "`feature` can't be used in a component, as components are built with the stable compiler".to_string());
            }
            _ => {}
        }
    }

    fn read_attribute(&mut self, attribute: &Group, trees: &[TokenTree], index: usize) {
        if !names_component(attribute) {
            return;
        }
        if let Some(name) = function_after(trees, index + 1) {
            let start = attribute.span().start();
            self.components.push(FoundComponent { at: (start.line as u32, start.column as u32), name: name.to_string() });
        }
    }
}

fn walk(stream: TokenStream) -> Scan {
    let mut scan = Scan::default();
    let top: Vec<TokenTree> = stream.into_iter().collect();
    // `rustc` may take a block of `---` at the top of the file for metadata, and would then
    // read differently from here. No Rust file can start with `-`.
    if let Some(first) = top.first().filter(|first| is_punct(first, '-')) {
        scan.refuse(first.span(), "A component can't start with `-`, as the compiler can read what comes after it differently".to_string());
    }
    // A list, not recursion: a file can nest as deeply as it's long, and the stack can't.
    let mut levels = vec![(top, Place::Code)];
    while let Some((trees, place)) = levels.pop() {
        for (index, tree) in trees.iter().enumerate() {
            match tree {
                TokenTree::Ident(ident) => scan.read_word(ident, &trees, index, place),
                TokenTree::Group(group) => {
                    let inside = place_of_group(&trees, index, place);
                    if place == Place::Code && inside == Place::Attribute {
                        scan.read_attribute(group, &trees, index);
                    }
                    levels.push((group.stream().into_iter().collect(), inside));
                }
                TokenTree::Literal(literal) if !suffix_of(literal).is_ascii() => {
                    scan.refuse(literal.span(), ONLY_ASCII_NAMES.to_string());
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => {}
            }
        }
    }
    scan.refusals.sort_by_key(|found| found.at);
    scan.components.sort_by_key(|component| component.at);
    scan
}

/// `Err` holds the line the lexer gave up on.
fn scan(source: &str) -> Result<Scan, u32> {
    let scanned = match TokenStream::from_str(source) {
        Ok(stream) => Ok(walk(stream)),
        Err(error) => Err(line_of(error.span())),
    };
    // Each lex keeps a copy of the whole file for its thread, to answer questions about
    // spans, and the app lexes sources for as long as it's open. Only numbers are left now.
    proc_macro2::extra::invalidate_current_thread_spans();
    scanned
}

/// `rustc` skips a first line starting `#!`, unless the next thing after that is `[`.
/// The lexer here doesn't, so `#!/*` starts a comment for it and for `rustc` starts nothing.
fn starts_with_a_shebang(source: &str) -> bool {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    source.starts_with("#!") && !source.starts_with("#![")
}

/// Whether this source may be given to the compiler, and the name of its component.
pub fn check_source(file: &str, source: &str) -> Result<SourceInfo, Vec<Diagnostic>> {
    let refusal = |line: u32, message: String| Diagnostic::new(file, line, message);
    if starts_with_a_shebang(source) {
        return Err(vec![refusal(1, "A component can't start with a `#!` line, as the compiler would skip it and this check wouldn't".to_string())]);
    }
    let mut scanned = match scan(source) {
        Ok(scanned) => scanned,
        Err(line) => {
            return Err(vec![refusal(line, "This can't be read as Rust, so it can't be checked: look for an unclosed quote or bracket".to_string())]);
        }
    };
    if scanned.components.is_empty() {
        scanned.refusals.insert(0, Found { at: (1, 0), message: "This file has no `#[component]` function, and it needs one".to_string() });
    }
    for extra in scanned.components.iter().skip(1) {
        scanned.refusals.push(Found { at: extra.at, message: format!("`{}` is a second `#[component]` function, and a file can have only one", extra.name) });
    }
    if scanned.refusals.is_empty() {
        return Ok(SourceInfo { tag: scanned.components.swap_remove(0).name });
    }
    scanned.refusals.sort_by_key(|found| found.at);
    Err(scanned.refusals.into_iter().map(|found| refusal(found.at.0, found.message)).collect())
}

/// The name of the first `#[component]` function, without judging the file.
pub fn component_tag(source: &str) -> Option<String> {
    scan(source).ok()?.components.into_iter().next().map(|component| component.name)
}
