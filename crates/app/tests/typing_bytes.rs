//! One letter typed at the start and at the end of each paragraph of hand-written talks
//! changes no byte outside that paragraph's lines, and undo gives the talk back.

use slides_engine::Fonts;
use slides_format::{Pos, text_flows};
use stagepack_studio::document::Document;

const TALKS: &[(&str, &str)] = &[
    ("4-space nested", "# T\n\n- a\n    - deep\n- b\n\nafter\n"),
    ("tab nested", "# T\n\n- a\n\t- deep\n- b\n\nafter\n"),
    ("3-space under number", "# T\n\n1. a\n   - nested\n2. b\n"),
    ("4-space under number", "# T\n\n1. a\n    - nested\n2. b\n"),
    ("star list", "# T\n\n* a\n* b\n"),
    ("para then tight list", "# T\n\nintro:\n- a\n- b\n"),
    ("heading then text tight", "# T\ntext right under\n\nmore\n"),
    ("loose list", "# T\n\n- a\n\n- b\n\n- c\n"),
    ("lazy continuation", "# T\n\n- a\n  continued\n- b\n"),
    ("two-space hard break", "# T\n\nline one  \nline two\n"),
    ("one-space indent list", "# T\n\n - a\n - b\n"),
    ("ten eleven", "# T\n\n10. ten\n11. eleven\n"),
    ("setext", "Title\n=====\n\nbody\n"),
    ("closed atx", "## Sub ##\n\nbody\n\n## Another ##\n"),
    ("wide marker", "# T\n\n-   wide\n-   two\n"),
    ("indented para", "# T\n\n   three spaces in\n\nnormal\n"),
    ("crlf 4-space nested", "# T\r\n\r\n- a\r\n    - deep\r\n- b\r\n"),
    ("three blank lines", "# T\n\n\n\npara one\n\n\n\npara two\n"),
    ("trailing spaces", "# T   \n\npara one   \n\npara two\n"),
    ("underscore emphasis", "# T\n\nsome _it_ and __bold__ here\n\nother\n"),
    ("nested ordered in bullets 4sp", "# T\n\n- a\n    1. one\n    2. two\n- b\n"),
    ("deep 2-space", "# T\n\n- a\n  - b\n    - c\n"),
    ("no trailing newline", "# T\n\n- a\n- b"),
    ("paren numbers", "# T\n\n1) a\n2) b\n"),
    ("all ones", "# T\n\n1. a\n1. b\n1. c\n"),
    ("soft wrapped para", "# T\n\nfirst line\nsecond line\nthird\n\nnext\n"),
    ("escaped", "# T\n\n\\* not a bullet\n\n2\\. not a list\n"),
    ("entity", "# T\n\nQ&amp;A and &copy; here\n\nother\n"),
    ("autolink", "# T\n\nsee <http://x.y> ok\n\nother\n"),
    ("html-ish", "# T\n\na <b>bold</b> b\n\nother\n"),
    ("link ref", "# T\n\nsee [x][1] ok\n\n[1]: http://x.y\n\nother\n"),
    ("strike", "# T\n\nsome ~~gone~~ text\n\nother\n"),
    ("image inline", "# T\n\ntext ![a](a.png) text\n\nother\n"),
    ("code span", "# T\n\nuse `x * y` here\n\nother\n"),
    ("emphasis star", "# T\n\nsome *it* and **bold** and ***both***\n\nother\n"),
];

fn line_start(s: &str, at: usize) -> usize { s[..at].rfind('\n').map_or(0, |i| i + 1) }
fn line_end(s: &str, at: usize) -> usize { s[at..].find('\n').map_or(s.len(), |i| at + i + 1) }

#[test]
fn one_letter_changes_one_paragraph() {
    let mut bad = 0;
    let mut total = 0;
    let mut refused = 0;
    let mut uneditable = Vec::new();
    for (name, talk) in TALKS {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("slides.md"), talk).unwrap();
        let probe = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
        let flows = text_flows(probe.talk(), 0);
        let read: usize = flows.iter().map(|f| f.paras.len()).sum();
        let lines = talk.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("==") && !l.starts_with("[1]")).count();
        if read < 2 { uneditable.push(format!("{name}: only {read} editable paragraphs of ~{lines} lines")); }
        for (f, flow) in flows.iter().enumerate() {
            for p in 0..flow.paras.len() {
                for at_end in [true, false] {
                    total += 1;
                    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
                    let len: usize = flow.paras[p].spans.iter().map(|s| s.text.len()).sum();
                    let offset = if at_end { len } else { 0 };
                    assert!(doc.begin_text(0, f, Pos { para: p, offset }));
                    let before = doc.source().to_string();
                    if doc.apply_text(|e| e.insert("x")).is_err() {
                        refused += 1;
                        eprintln!("REFUSED [{name}] flow {f} para {p} end={at_end}: {:?}", &before[flow.source[p].block.clone()]);
                        continue;
                    }
                    let after = doc.source().to_string();
                    let pre = before.bytes().zip(after.bytes()).take_while(|(a, b)| a == b).count();
                    let most = before.len().min(after.len()) - pre;
                    let suf = before.bytes().rev().zip(after.bytes().rev()).take(most).take_while(|(a, b)| a == b).count();
                    let (lo, hi) = (pre, before.len() - suf);
                    let block = flow.source[p].block.clone();
                    let (ls, le) = (line_start(&before, block.start), line_end(&before, block.end));
                    if lo < ls || hi > le {
                        bad += 1;
                        eprintln!("OUTSIDE [{name}] flow {f} para {p} end={at_end}\n  before {before:?}\n  after  {after:?}");
                    }
                    // and undo restores
                    doc.end_text().ok();
                    while doc.undo_label().is_some() { doc.undo().unwrap(); }
                    assert_eq!(doc.source(), before, "{name}: undo");
                }
            }
        }
    }
    eprintln!("total {total} outside-paragraph {bad} refused {refused}");
    for u in uneditable { eprintln!("UNEDITABLE {u}"); }
    assert_eq!(bad, 0, "a typed letter changed bytes outside its paragraph");
}
