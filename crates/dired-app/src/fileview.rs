// dired-6xv
//! Building the read-only file view: pure functions from bytes and facts to
//! lines. The reading itself happens in `tasks::file_view`.

use std::path::PathBuf;
use std::time::SystemTime;

use dired_domain::{format_mtime, format_size, FileKind};

use crate::buffers::{LineStyle, TextLine};

/// Most lines of a text file shown.
pub const MAX_LINES: usize = 500;
/// Most bytes read from a file.
pub const MAX_BYTES: usize = 64 * 1024;
/// Bytes shown in a binary file's hex dump.
pub const HEX_BYTES: usize = 256;
/// Most directory entries shown.
pub const DIR_ENTRIES: usize = 200;

/// Whether `bytes` (the start of a file) should be treated as binary: a NUL
/// byte early on, or UTF-8 that is invalid somewhere other than a character
/// cut off by the read limit.
pub fn is_binary(bytes: &[u8]) -> bool {
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return true;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => false,
        Err(e) => e.error_len().is_some(),
    }
}

fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let n = 4 - col % 4;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

/// The lines of the text in `bytes` (at most [`MAX_LINES`]), and whether the
/// file continues past what is shown. `total` is the file's full size.
pub fn text_lines(bytes: &[u8], total: u64) -> (Vec<String>, bool) {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or(""),
    };
    let mut truncated = (bytes.len() as u64) < total;
    let mut lines = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if i == MAX_LINES {
            truncated = true;
            break;
        }
        lines.push(expand_tabs(l));
    }
    (lines, truncated)
}

/// `hexdump -C` style rows: offset, two groups of eight bytes, ASCII.
pub fn hex_dump(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(row, chunk)| {
            let mut hex = String::new();
            for i in 0..16 {
                match chunk.get(i) {
                    Some(b) => hex.push_str(&format!("{b:02x} ")),
                    None => hex.push_str("   "),
                }
                if i == 7 {
                    hex.push(' ');
                }
            }
            let ascii: String =
                chunk.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
            format!("{:08x}  {} |{}|", row * 16, hex, ascii)
        })
        .collect()
}

/// What may safely be read for a (symlink-resolved) kind of file. Pipes,
/// sockets and devices are never opened: reading them can block forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPlan {
    Text,
    Dir,
    Skip,
}

pub fn plan_for(kind: FileKind) -> ReadPlan {
    match kind {
        FileKind::File => ReadPlan::Text,
        FileKind::Dir => ReadPlan::Dir,
        _ => ReadPlan::Skip,
    }
}

/// What was found when reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text { lines: Vec<String>, truncated: bool },
    Binary { head: Vec<u8> },
    /// Names (directories end in `/`), and how many more were not listed.
    Dir { names: Vec<String>, more: usize },
    NotRegular,
    Error(String),
}

/// What is known about the viewed path.
#[derive(Debug, Clone)]
pub struct Facts {
    pub path: PathBuf,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub link_target: Option<PathBuf>,
}

/// The view's lines: a two-line header, a blank line, then the body.
pub fn render(facts: &Facts, content: Content) -> Vec<TextLine> {
    let mut out = Vec::new();
    let title = match &facts.link_target {
        Some(t) => format!("{} → {}", facts.path.display(), t.display()),
        None => facts.path.display().to_string(),
    };
    out.push(TextLine::new(title, LineStyle::Header));
    let kind = match &content {
        Content::Text { .. } => Some("text"),
        Content::Binary { .. } => Some("binary"),
        Content::Dir { .. } => Some("directory"),
        Content::NotRegular => Some("special file"),
        Content::Error(_) => None,
    };
    let is_dir = matches!(content, Content::Dir { .. });
    let mut facts_line: Vec<String> = kind.map(str::to_string).into_iter().collect();
    if let (Some(s), false) = (facts.size, is_dir) {
        facts_line.push(format_size(s));
    }
    if facts.modified.is_some() {
        facts_line.push(format_mtime(facts.modified));
    }
    out.push(TextLine::new(facts_line.join("  "), LineStyle::Dim));
    out.push(TextLine::new("", LineStyle::Normal));
    match content {
        Content::Text { lines, truncated } => {
            if lines.is_empty() {
                out.push(TextLine::new("(empty file)", LineStyle::Dim));
            }
            out.extend(lines.into_iter().map(|l| TextLine::new(l, LineStyle::Normal)));
            if truncated {
                let total = facts.size.map(format_size).unwrap_or_else(|| "?".into());
                out.push(TextLine::new(format!("… truncated, {total} total"), LineStyle::Dim));
            }
        }
        Content::Binary { head } => {
            let size = facts.size.map(format_size).unwrap_or_else(|| format!("{} bytes", head.len()));
            out.push(TextLine::new(format!("Binary file ({size})"), LineStyle::Dim));
            out.push(TextLine::new("", LineStyle::Normal));
            let shown = &head[..head.len().min(HEX_BYTES)];
            out.extend(hex_dump(shown).into_iter().map(|r| TextLine::new(r, LineStyle::Normal)));
            if facts.size.map(|s| s > shown.len() as u64).unwrap_or(head.len() > shown.len()) {
                out.push(TextLine::new("…", LineStyle::Dim));
            }
        }
        Content::Dir { names, more } => {
            if names.is_empty() {
                out.push(TextLine::new("(empty directory)", LineStyle::Dim));
            }
            out.extend(names.into_iter().map(|n| TextLine::new(n, LineStyle::Normal)));
            if more > 0 {
                out.push(TextLine::new(format!("… and {more} more"), LineStyle::Dim));
            }
        }
        Content::NotRegular => out.push(TextLine::new("Not a regular file; not read", LineStyle::Dim)),
        Content::Error(m) => out.push(TextLine::new(format!("Cannot read: {m}"), LineStyle::Error)),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffers::LineStyle;
    use std::time::{Duration, UNIX_EPOCH};

    fn facts(size: Option<u64>) -> Facts {
        Facts {
            path: PathBuf::from("/w/a.txt"),
            size,
            modified: Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            link_target: None,
        }
    }

    fn texts(lines: &[TextLine]) -> Vec<String> {
        lines.iter().map(|l| l.text.clone()).collect()
    }

    #[test]
    fn binary_detection() {
        assert!(!is_binary(b""));
        assert!(!is_binary("plain text\nwith é and 日本\n".as_bytes()));
        assert!(is_binary(b"abc\0def"), "a NUL byte");
        assert!(is_binary(&[0x66, 0xff, 0xfe, 0x67]), "invalid UTF-8 in the middle");
        // A multibyte character cut off by the read limit is not invalid.
        let cut = &"é".as_bytes()[..1];
        assert!(!is_binary(cut));
        let mut v = b"text ".to_vec();
        v.extend_from_slice(cut);
        assert!(!is_binary(&v));
    }

    #[test]
    fn text_lines_expand_tabs_strip_crlf_and_report_truncation() {
        let (l, t) = text_lines(b"a\tb\r\nc\n", 7);
        assert_eq!(l, ["a   b", "c"]);
        assert!(!t);
        let (l, t) = text_lines(b"", 0);
        assert!(l.is_empty() && !t);
        // No trailing blank line for a final newline, but interior blanks stay.
        let (l, _) = text_lines(b"x\n\ny\n", 5);
        assert_eq!(l, ["x", "", "y"]);
        // The file is longer than what was read.
        let (_, t) = text_lines(b"short", 5000);
        assert!(t);
        // Too many lines.
        let big: String = (0..MAX_LINES + 50).map(|i| format!("l{i}\n")).collect();
        let (l, t) = text_lines(big.as_bytes(), big.len() as u64);
        assert_eq!(l.len(), MAX_LINES);
        assert_eq!(l.last().unwrap(), &format!("l{}", MAX_LINES - 1));
        assert!(t);
        // Exactly MAX_LINES lines is not truncated.
        let exact: String = (0..MAX_LINES).map(|i| format!("l{i}\n")).collect();
        assert!(!text_lines(exact.as_bytes(), exact.len() as u64).1);
    }

    #[test]
    fn hex_dump_matches_the_hexdump_c_layout() {
        let rows = hex_dump(b"Hello world\n\x00\x01\x02\x03");
        assert_eq!(rows, ["00000000  48 65 6c 6c 6f 20 77 6f  72 6c 64 0a 00 01 02 03  |Hello world.....|"]);
        let mut data = vec![b'A'; 17];
        data[16] = b'Z';
        let rows = hex_dump(&data);
        assert_eq!(rows.len(), 2);
        assert!(rows[1].starts_with("00000010  5a "), "{}", rows[1]);
        assert!(rows[1].ends_with("|Z|"));
        // The ASCII column lines up even on a short last row.
        assert_eq!(rows[0].find('|'), rows[1].find('|'));
        assert!(hex_dump(b"").is_empty());
    }

    #[test]
    fn only_regular_files_and_directories_are_ever_read() {
        use dired_domain::FileKind::*;
        assert_eq!(plan_for(File), ReadPlan::Text);
        assert_eq!(plan_for(Dir), ReadPlan::Dir);
        for k in [Fifo, Socket, BlockDevice, CharDevice, Unknown, Symlink] {
            assert_eq!(plan_for(k), ReadPlan::Skip, "{k:?}");
        }
    }

    #[test]
    fn a_text_view_has_a_header_then_the_lines() {
        let out = render(&facts(Some(12)), Content::Text { lines: vec!["hello".into(), "world".into()], truncated: false });
        let t = texts(&out);
        assert_eq!(t[0], "/w/a.txt");
        assert_eq!(out[0].style, LineStyle::Header);
        assert!(t[1].contains("text") && t[1].contains("12") && t[1].contains("2023-11-14"), "{}", t[1]);
        assert_eq!(out[1].style, LineStyle::Dim);
        assert_eq!(t[2], "");
        assert_eq!(&t[3..], ["hello", "world"]);
    }

    #[test]
    fn truncation_and_emptiness_are_said_out_loud() {
        let out = render(&facts(Some(5_000_000)), Content::Text { lines: vec!["x".into()], truncated: true });
        let last = out.last().unwrap();
        assert!(last.text.starts_with("… truncated") && last.text.contains("4.8M"), "{}", last.text);
        assert_eq!(last.style, LineStyle::Dim);
        let out = render(&facts(Some(0)), Content::Text { lines: vec![], truncated: false });
        assert_eq!(out.last().unwrap().text, "(empty file)");
    }

    #[test]
    fn binary_files_show_a_note_and_a_bounded_hex_dump() {
        let head: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let out = render(&facts(Some(123_456)), Content::Binary { head });
        let t = texts(&out);
        assert!(t.iter().any(|l| l.starts_with("Binary file")), "{t:?}");
        let rows: Vec<&String> = t.iter().filter(|l| l.len() > 8 && l.as_bytes()[8] == b' ' && l.contains('|')).collect();
        assert_eq!(rows.len(), HEX_BYTES / 16, "only the first {HEX_BYTES} bytes are dumped");
        assert!(t.last().unwrap().starts_with('…'), "says there is more: {:?}", t.last());
    }

    #[test]
    fn directories_list_names_and_count_the_rest() {
        let out = render(&facts(None), Content::Dir { names: vec!["sub/".into(), "a.txt".into()], more: 3 });
        let t = texts(&out);
        assert!(t[1].contains("directory"), "{}", t[1]);
        assert!(t.contains(&"sub/".to_string()) && t.contains(&"a.txt".to_string()));
        assert_eq!(t.last().unwrap(), "… and 3 more");
        let out = render(&facts(None), Content::Dir { names: vec![], more: 0 });
        assert_eq!(out.last().unwrap().text, "(empty directory)");
    }

    #[test]
    fn links_name_their_target_and_failures_are_errors() {
        let mut f = facts(Some(3));
        f.link_target = Some(PathBuf::from("/w/real"));
        let out = render(&f, Content::Text { lines: vec!["x".into()], truncated: false });
        assert_eq!(out[0].text, "/w/a.txt → /w/real");
        let out = render(&facts(None), Content::NotRegular);
        assert!(out.last().unwrap().text.contains("Not a regular file"));
        let out = render(&facts(None), Content::Error("denied".into()));
        let e = out.last().unwrap();
        assert_eq!((e.text.as_str(), e.style), ("Cannot read: denied", LineStyle::Error));
    }

    #[test]
    fn control_characters_in_the_path_header_are_left_for_the_renderer_to_escape() {
        // Sanitizing happens at draw time (render::truncate); the text is kept verbatim.
        let mut f = facts(Some(1));
        f.path = PathBuf::from("/w/a\u{1b}b");
        assert!(render(&f, Content::NotRegular)[0].text.contains('\u{1b}'));
    }
}
