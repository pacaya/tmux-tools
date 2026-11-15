//! P9.4: tmux's key grammar (`key_string_lookup_string` in tmux `key-string.c`) and the
//! bytes tmux's xterm key output (`input-keys.c`) produces for a key in normal cursor
//! and keypad mode. Orca writes the bytes with `terminal send --text`.

use anyhow::{bail, Result};

pub(in crate::host) const CTRL: u8 = 4;
pub(in crate::host) const META: u8 = 2;
pub(in crate::host) const SHIFT: u8 = 1;

/// The bytes one `send` argument stands for: the key's bytes when tmux parses the
/// argument as a key, else the argument as literal text.
pub(in crate::host) fn translate(argument: &str) -> Result<String> {
    match parse(argument) {
        Some((modifiers, key)) => key_bytes(modifiers, key),
        None => Ok(argument.to_owned()),
    }
}

/// A key without its modifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::host) enum Key {
    Char(char),
    Named(Named),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::host) enum Named {
    F(u8),
    Insert,
    Delete,
    Home,
    End,
    PageDown,
    PageUp,
    BTab,
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Tab,
    Space,
    BSpace,
    Keypad(&'static str),
}

/// tmux's names for keys, matched case-insensitively.
fn named(name: &str) -> Option<Named> {
    let lower = name.to_ascii_lowercase();
    if let Some(number) = lower.strip_prefix('f') {
        if let Ok(number) = number.parse::<u8>() {
            if (1..=12).contains(&number) && !number.to_string().starts_with('0') {
                return Some(Named::F(number));
            }
        }
    }
    Some(match lower.as_str() {
        "ic" | "insert" => Named::Insert,
        "dc" | "delete" => Named::Delete,
        "home" => Named::Home,
        "end" => Named::End,
        "npage" | "pagedown" | "pgdn" => Named::PageDown,
        "ppage" | "pageup" | "pgup" => Named::PageUp,
        "btab" => Named::BTab,
        "up" => Named::Up,
        "down" => Named::Down,
        "left" => Named::Left,
        "right" => Named::Right,
        "enter" => Named::Enter,
        "escape" => Named::Escape,
        "tab" => Named::Tab,
        "space" => Named::Space,
        "bspace" => Named::BSpace,
        "kp/" => Named::Keypad("/"),
        "kp*" => Named::Keypad("*"),
        "kp-" => Named::Keypad("-"),
        "kp+" => Named::Keypad("+"),
        "kp." => Named::Keypad("."),
        "kpenter" => Named::Keypad("\n"),
        "kp0" => Named::Keypad("0"),
        "kp1" => Named::Keypad("1"),
        "kp2" => Named::Keypad("2"),
        "kp3" => Named::Keypad("3"),
        "kp4" => Named::Keypad("4"),
        "kp5" => Named::Keypad("5"),
        "kp6" => Named::Keypad("6"),
        "kp7" => Named::Keypad("7"),
        "kp8" => Named::Keypad("8"),
        "kp9" => Named::Keypad("9"),
        _ => return None,
    })
}

/// tmux's parse of a key string: its modifiers and key, or `None` when tmux does not
/// read the string as a key.
pub(in crate::host) fn parse(string: &str) -> Option<(u8, Key)> {
    // `0x<hex>` is a key by code point.
    if let Some(hex) = string.strip_prefix("0x") {
        let code = u32::from_str_radix(hex, 16).ok()?;
        return char::from_u32(code).map(|c| (0, Key::Char(c)));
    }

    let mut modifiers = 0;
    let mut rest = string;
    // `^x` is C-x.
    if let Some(after) = rest.strip_prefix('^') {
        if !after.is_empty() {
            let mut chars = after.chars();
            let first = chars.next()?;
            if chars.next().is_none() {
                return Some((CTRL, Key::Char(first.to_ascii_lowercase())));
            }
            modifiers |= CTRL;
            rest = after;
        }
    }
    // `C-`, `M-` and `S-` prefixes.
    loop {
        let bytes = rest.as_bytes();
        if bytes.len() < 2 || bytes[1] != b'-' {
            break;
        }
        modifiers |= match bytes[0] {
            b'C' | b'c' => CTRL,
            b'M' | b'm' => META,
            b'S' | b's' => SHIFT,
            _ => return None,
        };
        rest = &rest[2..];
    }
    if rest.is_empty() {
        return None;
    }
    if let Some(named) = named(rest) {
        return Some((modifiers, Key::Named(named)));
    }
    let mut chars = rest.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        return Some((modifiers, Key::Char(first)));
    }
    None
}

/// The xterm modifier parameter: 1 + shift + 2·meta + 4·ctrl.
fn parameter(modifiers: u8) -> u8 {
    1 + modifiers
}

fn key_bytes(modifiers: u8, key: Key) -> Result<String> {
    match key {
        Key::Char(c) => char_bytes(modifiers, c),
        Key::Named(named) => named_bytes(modifiers, named),
    }
}

fn char_bytes(modifiers: u8, c: char) -> Result<String> {
    let mut c = c;
    if modifiers & SHIFT != 0 {
        c = c.to_ascii_uppercase();
    }
    let mut out = String::new();
    if modifiers & META != 0 {
        out.push('\x1b');
    }
    if modifiers & CTRL != 0 {
        let control = match c {
            'a'..='z' | 'A'..='Z' => (c.to_ascii_lowercase() as u8) & 0x1f,
            '@' | ' ' | '2' => 0x00,
            '[' | '3' => 0x1b,
            '\\' | '4' => 0x1c,
            ']' | '5' => 0x1d,
            '^' | '6' | '~' => 0x1e,
            '_' | '7' | '/' | '-' => 0x1f,
            '?' | '8' => 0x7f,
            other => {
                out.push(other);
                return Ok(out);
            }
        };
        if control == 0 {
            bail!("cannot send C-{c}: it is the NUL byte, which Orca cannot carry as text");
        }
        out.push(control as char);
        return Ok(out);
    }
    out.push(c);
    Ok(out)
}

fn named_bytes(modifiers: u8, named: Named) -> Result<String> {
    let modified = modifiers != 0;
    let p = parameter(modifiers);
    // Keys whose modified form is `ESC [ 1 ; <p> <final>`.
    let csi_final = |final_char: char, plain: &str| {
        if modified {
            format!("\x1b[1;{p}{final_char}")
        } else {
            plain.to_owned()
        }
    };
    // Keys whose modified form is `ESC [ <n> ; <p> ~`.
    let tilde = |number: u8| {
        if modified {
            format!("\x1b[{number};{p}~")
        } else {
            format!("\x1b[{number}~")
        }
    };
    let meta_prefixed = |plain: &str| {
        if modifiers & META != 0 {
            format!("\x1b{plain}")
        } else {
            plain.to_owned()
        }
    };
    Ok(match named {
        Named::F(1) => csi_final('P', "\x1bOP"),
        Named::F(2) => csi_final('Q', "\x1bOQ"),
        Named::F(3) => csi_final('R', "\x1bOR"),
        Named::F(4) => csi_final('S', "\x1bOS"),
        Named::F(5) => tilde(15),
        Named::F(6) => tilde(17),
        Named::F(7) => tilde(18),
        Named::F(8) => tilde(19),
        Named::F(9) => tilde(20),
        Named::F(10) => tilde(21),
        Named::F(11) => tilde(23),
        Named::F(12) => tilde(24),
        Named::F(_) => unreachable!("F1-F12 only"),
        Named::Insert => tilde(2),
        Named::Delete => tilde(3),
        Named::Home => csi_final('H', "\x1b[1~"),
        Named::End => csi_final('F', "\x1b[4~"),
        Named::PageUp => tilde(5),
        Named::PageDown => tilde(6),
        Named::BTab => "\x1b[Z".to_owned(),
        Named::Up => csi_final('A', "\x1b[A"),
        Named::Down => csi_final('B', "\x1b[B"),
        Named::Right => csi_final('C', "\x1b[C"),
        Named::Left => csi_final('D', "\x1b[D"),
        Named::Enter => meta_prefixed("\r"),
        Named::Escape => meta_prefixed("\x1b"),
        Named::Tab => meta_prefixed("\t"),
        Named::Space => return char_bytes(modifiers, ' '),
        Named::BSpace => meta_prefixed("\x7f"),
        Named::Keypad(bytes) => meta_prefixed(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_p10_3_arguments_translate_to_tmux_xterm_bytes() {
        assert_eq!(translate("M-x").unwrap(), "\x1bx");
        assert_eq!(translate("F1").unwrap(), "\x1bOP");
        assert_eq!(translate("Up").unwrap(), "\x1b[A");
        assert_eq!(translate("y").unwrap(), "y");
        assert_eq!(translate("^c").unwrap(), "\x03");
        assert_eq!(translate("hello").unwrap(), "hello");
    }

    #[test]
    fn named_and_control_keys() {
        assert_eq!(translate("Enter").unwrap(), "\r");
        assert_eq!(translate("Escape").unwrap(), "\x1b");
        assert_eq!(translate("C-c").unwrap(), "\x03");
        assert_eq!(translate("C-d").unwrap(), "\x04");
        assert_eq!(translate("C-[").unwrap(), "\x1b");
        assert_eq!(translate("BSpace").unwrap(), "\x7f");
        assert_eq!(translate("C-Up").unwrap(), "\x1b[1;5A");
        assert_eq!(translate("F5").unwrap(), "\x1b[15~");
        assert_eq!(translate("PageUp").unwrap(), "\x1b[5~");
        assert_eq!(translate("M-Enter").unwrap(), "\x1b\r");
        assert_eq!(translate("enter").unwrap(), "\r");
    }

    #[test]
    fn strings_tmux_does_not_parse_as_keys_are_literal() {
        for text in [
            "ls -la",
            "C-",
            "M-",
            "X-a",
            "F13",
            "Upward",
            "a b",
            "printf '%s'",
        ] {
            assert_eq!(translate(text).unwrap(), text, "{text:?}");
        }
        assert_eq!(translate("-").unwrap(), "-");
    }
}
