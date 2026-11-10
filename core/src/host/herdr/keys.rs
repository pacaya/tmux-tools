//! P5.2: `send` arguments as `hc keys` key names. An argument that tmux's key grammar
//! parses as a key is its `hc keys` name; any other argument is literal text, one key
//! per character from the typeable set.

use anyhow::{anyhow, Result};

use crate::host::orca::keys::{parse, Key, Named, CTRL, META, SHIFT};

/// One `hc keys` key name and the refusal that names the argument it came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Pressed {
    pub name: String,
    pub refusal: String,
}

/// Whether tmux's key grammar reads `argument` as a key.
pub(super) fn is_key(argument: &str) -> bool {
    parse(argument).is_some()
}

/// The key names one `send` argument stands for. Nothing is sent when this fails.
pub(super) fn translate(argument: &str) -> Result<Vec<Pressed>> {
    match parse(argument) {
        Some((modifiers, key)) => {
            let refusal = format!("cannot type on herdr: {argument}");
            let name = key_name(modifiers, key).ok_or_else(|| anyhow!("{refusal}"))?;
            Ok(vec![Pressed { name, refusal }])
        }
        None => literal(argument),
    }
}

/// Literal text, one key per character; a character outside the typeable set fails
/// with its code point.
pub(super) fn literal(text: &str) -> Result<Vec<Pressed>> {
    text.chars()
        .map(|c| {
            let refusal = format!("cannot type on herdr: U+{:04X}", c as u32);
            typeable(c)
                .map(|name| Pressed {
                    name,
                    refusal: refusal.clone(),
                })
                .ok_or_else(|| anyhow!("{refusal}"))
        })
        .collect()
}

/// The typeable set of P5.2: space, letters (upper case as `shift+<letter>`), digits,
/// and `' , - . / ; = [ \ ] ``.
fn typeable(c: char) -> Option<String> {
    match c {
        ' ' => Some("space".to_owned()),
        'a'..='z' | '0'..='9' => Some(c.to_string()),
        'A'..='Z' => Some(format!("shift+{}", c.to_ascii_lowercase())),
        '\'' | ',' | '-' | '.' | '/' | ';' | '=' | '[' | '\\' | ']' | '`' => Some(c.to_string()),
        _ => None,
    }
}

/// The `hc keys` name of a parsed tmux key, `None` when hc has none.
fn key_name(modifiers: u8, key: Key) -> Option<String> {
    let ctrl = modifiers & CTRL != 0;
    let alt = modifiers & META != 0;
    let mut shift = modifiers & SHIFT != 0;
    let base = match key {
        Key::Char(' ') => "space".to_owned(),
        Key::Char(c) if c.is_ascii_uppercase() => {
            shift = true;
            c.to_ascii_lowercase().to_string()
        }
        Key::Char(c) if c.is_ascii_lowercase() => c.to_string(),
        Key::Char(c) if c.is_ascii_digit() => {
            if shift {
                return None;
            }
            c.to_string()
        }
        Key::Char(c) => {
            // Punctuation: alone, with alt, and `ctrl+/` and `ctrl+]`.
            typeable(c)?;
            if shift || (ctrl && !matches!(c, '/' | ']')) {
                return None;
            }
            c.to_string()
        }
        Key::Named(named) => match named {
            Named::F(number) => format!("f{number}"),
            Named::Insert => "insert".to_owned(),
            Named::Delete => "delete".to_owned(),
            Named::Home => "home".to_owned(),
            Named::End => "end".to_owned(),
            Named::PageDown => "pgdn".to_owned(),
            Named::PageUp => "pgup".to_owned(),
            Named::BTab => {
                if modifiers != 0 {
                    return None;
                }
                return Some("shift+tab".to_owned());
            }
            Named::Up => "up".to_owned(),
            Named::Down => "down".to_owned(),
            Named::Left => "left".to_owned(),
            Named::Right => "right".to_owned(),
            Named::Enter => "enter".to_owned(),
            Named::Escape => "esc".to_owned(),
            Named::Tab => "tab".to_owned(),
            Named::Space => "space".to_owned(),
            Named::BSpace => "backspace".to_owned(),
            Named::Keypad(_) => return None,
        },
    };
    let letter = matches!(key, Key::Char(c) if c.is_ascii_alphabetic());
    // hc has no `ctrl+shift+<letter>`.
    if letter && ctrl && shift {
        return None;
    }
    let mut name = String::new();
    if ctrl {
        name.push_str("ctrl+");
    }
    if alt {
        name.push_str("alt+");
    }
    if shift {
        name.push_str("shift+");
    }
    name.push_str(&base);
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(argument: &str) -> Vec<String> {
        translate(argument)
            .unwrap()
            .into_iter()
            .map(|pressed| pressed.name)
            .collect()
    }

    #[test]
    fn tmux_keys_become_hc_key_names() {
        assert_eq!(names("Enter"), ["enter"]);
        assert_eq!(names("Escape"), ["esc"]);
        assert_eq!(names("C-c"), ["ctrl+c"]);
        assert_eq!(names("^c"), ["ctrl+c"]);
        assert_eq!(names("M-x"), ["alt+x"]);
        assert_eq!(names("F1"), ["f1"]);
        assert_eq!(names("Up"), ["up"]);
        assert_eq!(names("y"), ["y"]);
        assert_eq!(names("A"), ["shift+a"]);
        assert_eq!(names("BTab"), ["shift+tab"]);
        assert_eq!(names("BSpace"), ["backspace"]);
    }

    #[test]
    fn literal_text_is_one_key_per_character() {
        assert_eq!(names("Hi x"), ["shift+h", "i", "space", "x"]);
    }

    #[test]
    fn untypeable_input_names_the_argument_or_the_code_point() {
        assert_eq!(
            translate("hi!").unwrap_err().to_string(),
            "cannot type on herdr: U+0021"
        );
        assert_eq!(
            translate("KP0").unwrap_err().to_string(),
            "cannot type on herdr: KP0"
        );
        assert_eq!(
            literal("é").unwrap_err().to_string(),
            "cannot type on herdr: U+00E9"
        );
    }
}
