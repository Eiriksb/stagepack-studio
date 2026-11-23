//! The bytes a key sends to a program in a terminal.
use gpui_kit::Keystroke;

/// What `key` sends, with the arrow keys as a program that hasn't asked for application mode takes
/// them. See [`key_bytes_in`].
pub fn key_bytes(key: &Keystroke) -> Option<Vec<u8>> {
    key_bytes_in(key, false)
}

/// What `key` sends to the program: printable text as it is, Enter as a carriage return, Backspace
/// as delete, Tab, Escape, the arrows (`ESC [ A` and so on, or `ESC O A` once the program has asked
/// for application cursor keys, as `application_cursor` says), Home, End, Page Up and Down, Delete,
/// the function keys, Alt with text as an escape before it, and Ctrl with a letter as the
/// control character. Ctrl+Shift+Escape is the show's, to give the keys back, so it sends nothing,
/// and neither do the shortcuts of the platform key.
pub fn key_bytes_in(key: &Keystroke, application_cursor: bool) -> Option<Vec<u8>> {
    let m = key.modifiers;
    if m.control && m.shift && key.key == "escape" {
        return None;
    }
    if m.platform || m.function {
        return None;
    }
    let name = key.key.as_str();
    let cursor = |letter: u8| -> Vec<u8> {
        if application_cursor && !m.shift && !m.control && !m.alt {
            vec![0x1b, b'O', letter]
        } else if m.shift || m.control || m.alt {
            // With a modifier: ESC [ 1 ; m X, where m is 1 + shift + 2 alt + 4 control.
            let code = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.control);
            format!("\x1b[1;{code}{}", letter as char).into_bytes()
        } else {
            vec![0x1b, b'[', letter]
        }
    };
    let tilde = |n: u8| -> Vec<u8> { format!("\x1b[{n}~").into_bytes() };
    let named: Option<Vec<u8>> = match name {
        "enter" => Some(b"\r".to_vec()),
        "backspace" => Some(vec![0x7f]),
        "tab" if m.shift => Some(b"\x1b[Z".to_vec()),
        "tab" => Some(b"\t".to_vec()),
        "escape" => Some(vec![0x1b]),
        "up" => Some(cursor(b'A')),
        "down" => Some(cursor(b'B')),
        "right" => Some(cursor(b'C')),
        "left" => Some(cursor(b'D')),
        "home" => Some(cursor(b'H')),
        "end" => Some(cursor(b'F')),
        "pageup" => Some(tilde(5)),
        "pagedown" => Some(tilde(6)),
        "delete" => Some(tilde(3)),
        "insert" => Some(tilde(2)),
        "f1" => Some(b"\x1bOP".to_vec()),
        "f2" => Some(b"\x1bOQ".to_vec()),
        "f3" => Some(b"\x1bOR".to_vec()),
        "f4" => Some(b"\x1bOS".to_vec()),
        "f5" => Some(tilde(15)),
        "f6" => Some(tilde(17)),
        "f7" => Some(tilde(18)),
        "f8" => Some(tilde(19)),
        "f9" => Some(tilde(20)),
        "f10" => Some(tilde(21)),
        "f11" => Some(tilde(23)),
        "f12" => Some(tilde(24)),
        _ => None,
    };
    if let Some(named) = named {
        // Alt and a key that has no modified form is an escape before it.
        let alt_prefixed = m.alt && !m.control && matches!(name, "enter" | "backspace" | "tab" | "escape");
        return Some(if alt_prefixed { [&[0x1b][..], &named].concat() } else { named });
    }
    // AltGr typing: Ctrl and Alt together giving a character is text.
    let altgr = m.control && m.alt && key.key_char.is_some();
    if m.control && !altgr {
        let mut letters = name.chars();
        let control = match (letters.next(), letters.next()) {
            (Some(c @ 'a'..='z'), None) => Some(c as u8 - b'a' + 1),
            (Some(c @ 'A'..='Z'), None) => Some(c as u8 - b'A' + 1),
            _ if name == "space" => Some(0),
            _ => None,
        }?;
        return Some(if m.alt { vec![0x1b, control] } else { vec![control] });
    }
    let text = match key.key_char.as_deref() {
        Some(text) if !text.is_empty() && !text.chars().any(char::is_control) => text.to_string(),
        Some(_) => return None,
        None if name == "space" => " ".to_string(),
        None => return None,
    };
    Some(if m.alt && !altgr { [&[0x1b][..], text.as_bytes()].concat() } else { text.into_bytes() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(source: &str) -> Keystroke {
        Keystroke::parse(Box::leak(source.to_string().into_boxed_str())).unwrap()
    }

    fn typed(source: &str, text: &str) -> Keystroke {
        Keystroke { key_char: Some(text.to_string()), ..key(source) }
    }

    #[test]
    fn keys_become_the_bytes_a_terminal_sends() {
        let table: [(Keystroke, Option<&[u8]>); 24] = [
            (typed("a", "a"), Some(b"a")),
            (typed("shift-a", "A"), Some(b"A")),
            (typed("o", "ø"), Some("ø".as_bytes())),
            (key("space"), Some(b" ")),
            (key("enter"), Some(b"\r")),
            (key("backspace"), Some(&[0x7f])),
            (key("tab"), Some(b"\t")),
            (key("shift-tab"), Some(b"\x1b[Z")),
            (key("escape"), Some(&[0x1b])),
            (key("up"), Some(b"\x1b[A")),
            (key("down"), Some(b"\x1b[B")),
            (key("right"), Some(b"\x1b[C")),
            (key("left"), Some(b"\x1b[D")),
            (key("ctrl-left"), Some(b"\x1b[1;5D")),
            (key("home"), Some(b"\x1b[H")),
            (key("end"), Some(b"\x1b[F")),
            (key("pageup"), Some(b"\x1b[5~")),
            (key("pagedown"), Some(b"\x1b[6~")),
            (key("delete"), Some(b"\x1b[3~")),
            (key("ctrl-a"), Some(&[1])),
            (key("ctrl-c"), Some(&[3])),
            (key("ctrl-z"), Some(&[26])),
            (key("ctrl-shift-escape"), None),
            (key("cmd-c"), None),
        ];
        for (key, bytes) in table {
            assert_eq!(key_bytes(&key).as_deref(), bytes, "{key:?}");
        }
    }

    #[test]
    fn a_program_can_ask_for_the_arrows_its_own_way() {
        assert_eq!(key_bytes_in(&key("up"), true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes_in(&key("left"), true).unwrap(), b"\x1bOD");
        assert_eq!(key_bytes_in(&key("up"), false).unwrap(), b"\x1b[A");
        // The others don't change.
        assert_eq!(key_bytes_in(&key("pageup"), true).unwrap(), b"\x1b[5~");
    }

    #[test]
    fn alt_is_an_escape_before_the_key_and_nothing_else_is_made_up() {
        assert_eq!(key_bytes(&typed("alt-b", "b")).unwrap(), b"\x1bb");
        assert_eq!(key_bytes(&key("f5")).unwrap(), b"\x1b[15~");
        // A key with no text and no name here sends nothing; a bare modifier too.
        assert_eq!(key_bytes(&key("shift")), None);
        assert_eq!(key_bytes(&key("ctrl-1")), None);
    }
}
