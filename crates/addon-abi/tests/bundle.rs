use addon_abi::{decode, encode, safe_name, Bundle, Notice, NoticeKind, Report, ABI_VERSION, ARGS, ERROR, MAGIC, MANIFEST, OP, OWN_NAMES, REPORT};

fn file(name: &str, data: &[u8]) -> (String, Vec<u8>) {
    (name.to_string(), data.to_vec())
}

/// A bundle's bytes, written out by hand: the count, then each name and its data.
fn raw(count: u32, entries: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend(count.to_le_bytes());
    for (name, data) in entries {
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(*name);
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(*data);
    }
    bytes
}

#[test]
fn the_interface_is_version_1() {
    assert_eq!(ABI_VERSION, 1);
    assert_eq!(MAGIC, *b"SPB1");
}

#[test]
fn round_trip() {
    let bundle: Bundle = vec![file("slides.md", b"# Hello\n"), file("public/empty.png", b""), file("public/smörgåsbord ☃.png", &[0, 255, 7])];
    let bytes = encode(&bundle);
    assert_eq!(&bytes[..4], b"SPB1");
    assert_eq!(&bytes[4..8], &3u32.to_le_bytes());
    assert_eq!(decode(&bytes), Ok(bundle));
}

#[test]
fn an_empty_bundle_round_trips() {
    assert_eq!(decode(&encode(&Vec::new())), Ok(Vec::new()));
}

#[test]
fn the_bundles_own_names_round_trip() {
    let bundle: Bundle = vec![file(OP, b"export"), file(ARGS, b"{}"), file(REPORT, br#"{"notices":[]}"#), file(ERROR, b"No."), file(MANIFEST, br#"{"title":"T"}"#)];
    assert_eq!(decode(&encode(&bundle)), Ok(bundle));
}

#[test]
fn the_layout_is_as_the_interface_says() {
    let bytes = encode(&vec![file("a", b"xy")]);
    assert_eq!(bytes, raw(1, &[(b"a", b"xy")]));
}

#[track_caller]
fn refused(bytes: &[u8], naming: &str) {
    match decode(bytes) {
        Err(message) => assert!(message.contains(naming), "{message:?} doesn't name {naming:?}"),
        Ok(bundle) => panic!("decoded {bundle:?}"),
    }
}

#[test]
fn decode_refuses() {
    let good = raw(1, &[(b"slides.md", b"# A")]);
    assert!(decode(&good).is_ok());

    refused(b"", "magic");
    refused(b"SPB2\0\0\0\0", "magic");
    refused(b"SPB1\0\0", "count");
    refused(&raw(5, &[(b"slides.md", b"# A")]), "count");
    refused(&raw(u32::MAX, &[]), "count");

    let mut name_past_end = raw(1, &[]);
    name_past_end.extend(100u32.to_le_bytes());
    name_past_end.extend(b"short");
    refused(&name_past_end, "name");

    let mut data_past_end = raw(1, &[]);
    data_past_end.extend(1u32.to_le_bytes());
    data_past_end.extend(b"a");
    data_past_end.extend(100u32.to_le_bytes());
    data_past_end.extend(b"short");
    refused(&data_past_end, "data");

    refused(&raw(1, &[(b"bad\xff.md", b"")]), "UTF-8");
    refused(&raw(1, &[(b"../x", b"")]), "../x");
    refused(&raw(1, &[(b".hidden", b"")]), ".hidden");
    refused(&raw(2, &[(b"a.md", b""), (b"a.md", b"")]), "twice");
    refused(&raw(2, &[(b"public/A.png", b""), (b"public/a.png", b"")]), "letter case");
    refused(&raw(2, &[(b"public/\xc3\x84.png", b""), (b"public/\xc3\xa4.png", b"")]), "letter case");

    let mut trailing = good.clone();
    trailing.push(0);
    refused(&trailing, "after");
}

#[test]
fn safe_name_table() {
    for name in ["slides.md", "public/a b.png", "public/fonts/x.woff2", "ä.md"] {
        assert!(safe_name(name), "{name:?} should be safe");
    }
    for name in [
        "../x", "/x", "a\\b", "a/../b", "", "a\0b", "C:x", "a//b", "./a", "a/", ".hidden", "public/.git/config", "a\nb", "con.txt",
        "a.", "a ", "a\u{200B}b",
    ] {
        assert!(!safe_name(name), "{name:?} should be refused");
    }
}

#[test]
fn the_bundles_own_names_are_not_safe_file_names() {
    assert_eq!(OWN_NAMES, [OP, ARGS, REPORT, ERROR, MANIFEST]);
    assert_eq!(MANIFEST, ".stagepack.json");
    for name in OWN_NAMES {
        assert!(!safe_name(name), "{name} must never be a file of the talk");
    }
}

#[test]
fn a_report_reads_as_json() {
    let report = Report {
        notices: vec![
            Notice { slide: Some(3), kind: NoticeKind::Placeholder, text: "A Vue component became a placeholder.".into() },
            Notice { slide: None, kind: NoticeKind::LeftOut, text: "The theme was left out.".into() },
        ],
    };
    let json = serde_json::to_string(&report).unwrap();
    assert_eq!(
        json,
        r#"{"notices":[{"slide":3,"kind":"placeholder","text":"A Vue component became a placeholder."},{"slide":null,"kind":"left_out","text":"The theme was left out."}]}"#
    );
    assert_eq!(serde_json::from_str::<Report>(&json).unwrap(), report);
    assert_eq!(serde_json::from_str::<Report>(r#"{"notices":[]}"#).unwrap(), Report::default());
    let kinds: Vec<NoticeKind> = serde_json::from_str(r#"["placeholder","approximation","left_out","frozen"]"#).unwrap();
    assert_eq!(kinds, [NoticeKind::Placeholder, NoticeKind::Approximation, NoticeKind::LeftOut, NoticeKind::Frozen]);
}
