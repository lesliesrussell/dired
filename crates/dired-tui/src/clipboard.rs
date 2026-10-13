// dired-vhh
//! System clipboard: the terminal's OSC 52 escape, plus the platform's own
//! tool (`pbcopy`, `wl-copy`, `xclip`, `xsel`) when not on an ssh session.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Standard base64 (RFC 4648, padded).
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The OSC 52 sequence that sets the system clipboard to `text`.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// Candidate clipboard programs, most specific first.
pub fn native_candidates() -> &'static [&'static [&'static str]] {
    if cfg!(target_os = "macos") {
        &[&["pbcopy"]]
    } else {
        &[&["wl-copy"], &["xclip", "-selection", "clipboard"], &["xsel", "--clipboard", "--input"]]
    }
}

/// The first executable called `name` in the `path` list (as in `$PATH`).
pub fn find_in_path(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path).map(|d| d.join(name)).find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Copy `text` to the clipboard: always OSC 52 (works over ssh and in most
/// modern terminals), and additionally the native tool outside ssh sessions.
pub fn copy(text: &str) -> io::Result<()> {
    let mut out = io::stdout();
    out.write_all(osc52(text).as_bytes())?;
    out.flush()?;
    if std::env::var_os("SSH_CONNECTION").is_none() {
        if let Some(path) = std::env::var_os("PATH") {
            for cmd in native_candidates() {
                if let Some(prog) = find_in_path(cmd[0], &path) {
                    let mut child = Command::new(prog)
                        .args(&cmd[1..])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()?;
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(text.as_bytes());
                    }
                    // Reap in the background; clipboard owners (xclip) may linger.
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want, "{input:?}");
        }
        // Non-ASCII and newlines survive: "é\n" is c3 a9 0a.
        assert_eq!(base64("é\n".as_bytes()), "w6kK");
    }

    #[test]
    fn osc52_wraps_the_payload() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
        assert_eq!(osc52(""), "\x1b]52;c;\x07");
    }

    #[test]
    fn find_in_path_returns_the_first_executable() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("dired-clip-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        for (dir, mode) in [(&a, 0o644), (&b, 0o755)] {
            let f = dir.join("tool");
            std::fs::write(&f, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let path = std::env::join_paths([&a, &b]).unwrap();
        // `a/tool` is not executable, so the one in `b` wins.
        assert_eq!(find_in_path("tool", &path), Some(b.join("tool")));
        assert_eq!(find_in_path("missing", &path), None);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_native_tool_is_listed_for_this_platform() {
        assert!(!native_candidates().is_empty());
        assert!(native_candidates().iter().all(|c| !c.is_empty()));
    }
}
