// dired-7qs
//! Emacs-style key notation: `C-x C-f`, `M-<`, `RET`, `<up>`.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub meta: bool,
}

impl Key {
    pub const fn plain(code: KeyCode) -> Key {
        Key { code, ctrl: false, meta: false }
    }
    pub const fn ch(c: char) -> Key {
        Key::plain(KeyCode::Char(c))
    }
    pub const fn ctrl(c: char) -> Key {
        Key { code: KeyCode::Char(c), ctrl: true, meta: false }
    }
    pub const fn meta(c: char) -> Key {
        Key { code: KeyCode::Char(c), ctrl: false, meta: true }
    }

    /// A printable, unmodified character (self-inserting in text fields).
    pub fn printable(&self) -> Option<char> {
        match self.code {
            KeyCode::Char(c) if !self.ctrl && !self.meta && !c.is_control() => Some(c),
            _ => None,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("C-")?;
        }
        if self.meta {
            f.write_str("M-")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("SPC"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Enter => f.write_str("RET"),
            KeyCode::Tab => f.write_str("TAB"),
            KeyCode::BackTab => f.write_str("<backtab>"),
            KeyCode::Backspace => f.write_str("DEL"),
            KeyCode::Delete => f.write_str("<delete>"),
            KeyCode::Esc => f.write_str("ESC"),
            KeyCode::Up => f.write_str("<up>"),
            KeyCode::Down => f.write_str("<down>"),
            KeyCode::Left => f.write_str("<left>"),
            KeyCode::Right => f.write_str("<right>"),
            KeyCode::Home => f.write_str("<home>"),
            KeyCode::End => f.write_str("<end>"),
            KeyCode::PageUp => f.write_str("<prior>"),
            KeyCode::PageDown => f.write_str("<next>"),
            KeyCode::F(n) => write!(f, "<f{n}>"),
        }
    }
}

pub fn format_seq(seq: &[Key]) -> String {
    seq.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(" ")
}

pub fn parse_key(tok: &str) -> Result<Key, String> {
    let mut rest = tok;
    let (mut ctrl, mut meta) = (false, false);
    loop {
        if rest.len() > 2 && rest.starts_with("C-") {
            ctrl = true;
            rest = &rest[2..];
        } else if rest.len() > 2 && rest.starts_with("M-") {
            meta = true;
            rest = &rest[2..];
        } else {
            break;
        }
    }
    let code = match rest {
        "RET" | "<return>" => KeyCode::Enter,
        "TAB" | "<tab>" => KeyCode::Tab,
        "<backtab>" => KeyCode::BackTab,
        "DEL" | "<backspace>" => KeyCode::Backspace,
        "<delete>" => KeyCode::Delete,
        "ESC" | "<escape>" => KeyCode::Esc,
        "SPC" => KeyCode::Char(' '),
        "<up>" => KeyCode::Up,
        "<down>" => KeyCode::Down,
        "<left>" => KeyCode::Left,
        "<right>" => KeyCode::Right,
        "<home>" => KeyCode::Home,
        "<end>" => KeyCode::End,
        "<prior>" | "<pageup>" => KeyCode::PageUp,
        "<next>" | "<pagedown>" => KeyCode::PageDown,
        s if s.starts_with("<f") && s.ends_with('>') => {
            let n: u8 = s[2..s.len() - 1]
                .parse()
                .map_err(|_| format!("invalid function key `{tok}`"))?;
            KeyCode::F(n)
        }
        s => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => return Err(format!("invalid key `{tok}`")),
            }
        }
    };
    Ok(Key { code, ctrl, meta })
}

pub fn parse_seq(s: &str) -> Result<Vec<Key>, String> {
    let keys: Result<Vec<Key>, String> = s.split_whitespace().map(parse_key).collect();
    let keys = keys?;
    if keys.is_empty() {
        return Err("empty key sequence".into());
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for s in ["C-x C-f", "M-<", "RET", "% m", "C-M-x", "<up>", "SPC", "<f5>"] {
            assert_eq!(format_seq(&parse_seq(s).unwrap()), s);
        }
        assert!(parse_seq("C-xx").is_err());
        assert!(parse_seq("").is_err());
    }
}
