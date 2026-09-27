// dired-7qs
//! Directory entries, their metadata, and display helpers.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Kind of a filesystem object, as far as it is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    Fifo,
    Socket,
    BlockDevice,
    CharDevice,
    Unknown,
}

impl FileKind {
    /// Indicator appended to names (similar to `ls -F`).
    pub fn indicator(self, executable: bool, target_is_dir: bool) -> &'static str {
        match self {
            FileKind::Dir => "/",
            FileKind::Symlink if target_is_dir => "@/",
            FileKind::Symlink => "@",
            FileKind::Fifo => "|",
            FileKind::Socket => "=",
            FileKind::File if executable => "*",
            _ => "",
        }
    }

    pub fn type_char(self) -> char {
        match self {
            FileKind::Dir => 'd',
            FileKind::Symlink => 'l',
            FileKind::Fifo => 'p',
            FileKind::Socket => 's',
            FileKind::BlockDevice => 'b',
            FileKind::CharDevice => 'c',
            FileKind::File | FileKind::Unknown => '-',
        }
    }
}

/// Metadata for an entry. Fields are optional where platforms may not provide them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMetadata {
    pub kind: FileKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Unix mode bits, if the platform exposes them.
    pub mode: Option<u32>,
    pub readonly: bool,
    pub dev: Option<u64>,
    pub ino: Option<u64>,
    pub symlink_target: Option<PathBuf>,
    pub target_is_dir: bool,
}

impl EntryMetadata {
    pub fn simple(kind: FileKind, size: u64) -> Self {
        EntryMetadata {
            kind,
            size,
            modified: None,
            mode: None,
            readonly: false,
            dev: None,
            ino: None,
            symlink_target: None,
            target_is_dir: false,
        }
    }

    pub fn executable(&self) -> bool {
        self.mode.map(|m| m & 0o111 != 0).unwrap_or(false)
    }

    /// Permission string; adapts to platform capabilities instead of fabricating Unix bits.
    pub fn permissions_string(&self) -> String {
        match self.mode {
            Some(mode) => {
                let mut s = String::with_capacity(10);
                s.push(self.kind.type_char());
                let bits = [
                    (0o400, 'r'),
                    (0o200, 'w'),
                    (0o100, 'x'),
                    (0o040, 'r'),
                    (0o020, 'w'),
                    (0o010, 'x'),
                    (0o004, 'r'),
                    (0o002, 'w'),
                    (0o001, 'x'),
                ];
                for (bit, c) in bits {
                    s.push(if mode & bit != 0 { c } else { '-' });
                }
                s
            }
            None => {
                let rw = if self.readonly { "r-" } else { "rw" };
                format!("{}{}", self.kind.type_char(), rw)
            }
        }
    }
}

/// Mark state of an entry. A line carries exactly one state, as in Dired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryState {
    Unmarked,
    Marked,
    DeleteFlagged,
}

/// A directory entry as held by a directory buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Raw file name.
    pub os_name: OsString,
    /// Display name with control characters escaped.
    pub name: String,
    /// Stable identity: path relative to the buffer root.
    pub key: PathBuf,
    pub kind: FileKind,
    pub meta: Option<EntryMetadata>,
    /// Non-actionable rows such as `..`.
    pub special: bool,
}

impl Entry {
    pub fn new(os_name: OsString, parent_key: &Path, kind: FileKind) -> Self {
        let name = display_name(&os_name);
        let key = parent_key.join(&os_name);
        Entry {
            os_name,
            name,
            key,
            kind,
            meta: None,
            special: false,
        }
    }

    pub fn parent_link() -> Self {
        Entry {
            os_name: OsString::from(".."),
            name: "..".to_string(),
            key: PathBuf::from(".."),
            kind: FileKind::Dir,
            meta: None,
            special: true,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == FileKind::Dir
    }

    /// Directory, or symlink known to point at a directory.
    pub fn is_dir_like(&self) -> bool {
        self.kind == FileKind::Dir
            || (self.kind == FileKind::Symlink
                && self.meta.as_ref().map(|m| m.target_is_dir).unwrap_or(false))
    }

    pub fn is_hidden(&self) -> bool {
        !self.special && self.name.starts_with('.')
    }

    /// Whether the displayed name round-trips exactly to the on-disk name.
    pub fn name_is_editable(&self) -> bool {
        !self.special && self.os_name.to_str() == Some(self.name.as_str())
    }

    pub fn extension(&self) -> &str {
        match self.name.rfind('.') {
            Some(0) | None => "",
            Some(i) => &self.name[i + 1..],
        }
    }

    pub fn indicator(&self) -> &'static str {
        let (exec, tdir) = self
            .meta
            .as_ref()
            .map(|m| (m.executable(), m.target_is_dir))
            .unwrap_or((false, false));
        self.kind.indicator(exec, tdir)
    }
}

/// Render an OS file name for display: lossy UTF-8, with control characters escaped
/// so a name can never break the line-oriented buffer.
pub fn display_name(name: &std::ffi::OsStr) -> String {
    let lossy = name.to_string_lossy();
    if !lossy.chars().any(|c| c.is_control()) {
        return lossy.into_owned();
    }
    let mut out = String::with_capacity(lossy.len() + 4);
    for c in lossy.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Human-readable size, `ls -h` style.
pub fn format_size(size: u64) -> String {
    const UNITS: [&str; 6] = ["K", "M", "G", "T", "P", "E"];
    if size < 1024 {
        return size.to_string();
    }
    let mut value = size as f64;
    let mut unit = 0;
    value /= 1024.0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{:.1}{}", value, UNITS[unit])
    } else {
        format!("{:.0}{}", value, UNITS[unit])
    }
}

static LOCAL_OFFSET: OnceLock<time::UtcOffset> = OnceLock::new();

/// Capture the local UTC offset. Call early, before other threads start.
pub fn init_local_offset() {
    let off = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let _ = LOCAL_OFFSET.set(off);
}

pub fn format_mtime(t: Option<SystemTime>) -> String {
    let Some(t) = t else {
        return "                ".to_string();
    };
    let off = *LOCAL_OFFSET.get().unwrap_or(&time::UtcOffset::UTC);
    let dt = time::OffsetDateTime::from(t).to_offset(off);
    let fmt = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]");
    dt.format(&fmt).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_control_chars() {
        assert_eq!(display_name(std::ffi::OsStr::new("a\nb")), "a\\nb");
        assert_eq!(display_name(std::ffi::OsStr::new("plain")), "plain");
    }

    #[test]
    fn sizes() {
        assert_eq!(format_size(12), "12");
        assert_eq!(format_size(2048), "2.0K");
        assert_eq!(format_size(50 * 1024 * 1024), "50M");
    }

    #[test]
    fn perms_adapt() {
        let mut m = EntryMetadata::simple(FileKind::File, 0);
        assert_eq!(m.permissions_string(), "-rw");
        m.mode = Some(0o755);
        assert_eq!(m.permissions_string(), "-rwxr-xr-x");
    }
}
