// dired-99k
//! Git status for the directory listing: a pure parser for
//! `git status --porcelain=v1 -z --branch` and the roll-up of changes onto
//! directories. Running git lives behind [`crate::services::GitProbe`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A two-letter status like `git status -s`: index state, then worktree state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Xy {
    pub x: char,
    pub y: char,
}

impl Xy {
    pub fn text(&self) -> String {
        format!("{}{}", self.x, self.y)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// `None` on a detached HEAD.
    pub name: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: Option<Branch>,
    /// Repository-relative path -> status, including directories rolled up
    /// from the changes inside them.
    map: HashMap<PathBuf, Xy>,
    /// Where the listed directory sits inside the repository (`""` at the top).
    prefix: PathBuf,
}

fn rank_x(c: char) -> u8 {
    match c {
        'U' => 6,
        'R' => 5,
        'A' => 4,
        'C' => 3,
        'D' => 2,
        ' ' => 0,
        _ => 1, // M, T, anything else
    }
}

fn rank_y(c: char) -> u8 {
    match c {
        'U' => 5,
        'D' => 4,
        'A' => 2,
        '?' => 1,
        ' ' => 0,
        _ => 3, // M, T, R
    }
}

fn merge(a: Xy, b: Xy) -> Xy {
    Xy {
        x: if rank_x(b.x) > rank_x(a.x) { b.x } else { a.x },
        y: if rank_y(b.y) > rank_y(a.y) { b.y } else { a.y },
    }
}

fn path_of(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn number_after(s: &str, key: &str) -> u32 {
    s.find(key)
        .map(|i| s[i + key.len()..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn parse_branch(s: &str) -> Branch {
    if s.starts_with("HEAD (no branch)") {
        return Branch { name: None, ahead: 0, behind: 0 };
    }
    for prefix in ["No commits yet on ", "Initial commit on "] {
        if let Some(n) = s.strip_prefix(prefix) {
            return Branch { name: Some(n.to_string()), ahead: 0, behind: 0 };
        }
    }
    let (name, rest) = s.split_once("...").unwrap_or((s, ""));
    let bracket = rest.find('[').map(|i| &rest[i..]).unwrap_or("");
    Branch {
        name: Some(name.to_string()),
        ahead: number_after(bracket, "ahead "),
        behind: number_after(bracket, "behind "),
    }
}

impl GitStatus {
    /// Parse `git status --porcelain=v1 -z --branch` output.
    pub fn parse(bytes: &[u8]) -> GitStatus {
        let recs: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
        let mut out = GitStatus::default();
        let mut explicit: HashMap<PathBuf, Xy> = HashMap::new();
        let mut i = 0;
        while i < recs.len() {
            let r = recs[i];
            i += 1;
            if let Some(b) = r.strip_prefix(b"## ") {
                out.branch = Some(parse_branch(&String::from_utf8_lossy(b)));
                continue;
            }
            if r.len() < 4 || r[2] != b' ' {
                continue;
            }
            let (x, y) = (r[0] as char, r[1] as char);
            // A rename or copy is followed by one more record: the old name.
            if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
                i += 1;
            }
            let unmerged = matches!((x, y), ('D', 'D') | ('A', 'U') | ('U', 'D') | ('U', 'A') | ('D', 'U') | ('A', 'A') | ('U', 'U'));
            let xy = if unmerged { Xy { x: 'U', y: 'U' } } else { Xy { x, y } };
            let mut p = r[3..].to_vec();
            while p.last() == Some(&b'/') {
                p.pop();
            }
            explicit.insert(path_of(&p), xy);
        }
        // Roll the changes up onto every directory above them.
        let mut rolled: HashMap<PathBuf, Xy> = HashMap::new();
        for (p, xy) in &explicit {
            let contribution = if xy.x == '?' { Xy { x: ' ', y: '?' } } else { *xy };
            for anc in p.ancestors().skip(1).filter(|a| !a.as_os_str().is_empty()) {
                let e = rolled.entry(anc.to_path_buf()).or_insert(Xy { x: ' ', y: ' ' });
                *e = merge(*e, contribution);
            }
        }
        out.map = rolled;
        out.map.extend(explicit); // an entry git reported itself keeps its own status
        out
    }

    /// Where the directory being listed sits inside the repository.
    pub fn with_prefix(mut self, prefix: impl Into<PathBuf>) -> GitStatus {
        self.prefix = prefix.into();
        self
    }

    /// The status of `key`, a path relative to the listed directory.
    pub fn xy_for(&self, key: &Path) -> Option<Xy> {
        self.map.get(&self.prefix.join(key)).copied()
    }

    pub fn dirty(&self) -> bool {
        !self.map.is_empty()
    }

    /// `main*↑1↓2`: the branch, `*` when anything changed, then ahead/behind.
    pub fn summary(&self) -> String {
        let Some(b) = &self.branch else { return "?".into() };
        let mut s = b.name.clone().unwrap_or_else(|| "detached".into());
        if self.dirty() {
            s.push('*');
        }
        if b.ahead > 0 {
            s.push_str(&format!("↑{}", b.ahead));
        }
        if b.behind > 0 {
            s.push_str(&format!("↓{}", b.behind));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(s: &str) -> GitStatus {
        GitStatus::parse(s.as_bytes())
    }

    fn xy(g: &GitStatus, p: &str) -> Option<String> {
        g.xy_for(Path::new(p)).map(|x| x.text())
    }

    #[test]
    fn real_git_output_is_parsed_including_renames_and_untracked_dirs() {
        // Captured from git 2.51: modified, staged add, staged rename, untracked file and dir.
        let g = st("## master\0 M a.txt\0A  b.txt\0R  e.txt\0d.txt\0?? c.txt\0?? newdir/\0");
        assert_eq!(xy(&g, "a.txt").as_deref(), Some(" M"));
        assert_eq!(xy(&g, "b.txt").as_deref(), Some("A "));
        assert_eq!(xy(&g, "e.txt").as_deref(), Some("R "));
        assert_eq!(xy(&g, "d.txt"), None, "the old name of a rename is not an entry of its own");
        assert_eq!(xy(&g, "c.txt").as_deref(), Some("??"));
        assert_eq!(xy(&g, "newdir").as_deref(), Some("??"), "an untracked directory");
        assert_eq!(xy(&g, "clean.txt"), None);
        assert!(g.dirty());
        assert_eq!(g.summary(), "master*");
    }

    #[test]
    fn names_with_spaces_and_newlines_survive() {
        let g = st("## main\0?? sp ace.txt\0 M we\nird\0");
        assert_eq!(xy(&g, "sp ace.txt").as_deref(), Some("??"));
        assert_eq!(xy(&g, "we\nird").as_deref(), Some(" M"));
    }

    #[test]
    fn copies_also_carry_an_old_name_record() {
        let g = st("## main\0C  copy.txt\0orig.txt\0 M z\0");
        assert_eq!(xy(&g, "copy.txt").as_deref(), Some("C "));
        assert_eq!(xy(&g, "orig.txt"), None);
        assert_eq!(xy(&g, "z").as_deref(), Some(" M"), "parsing continues after the extra record");
    }

    #[test]
    fn branch_lines() {
        let cases = [
            ("## main", "main", 0, 0),
            ("## main...origin/main", "main", 0, 0),
            ("## main...origin/main [ahead 1]", "main", 1, 0),
            ("## main...origin/main [behind 2]", "main", 0, 2),
            ("## main...origin/main [ahead 3, behind 4]", "main", 3, 4),
            ("## feature/x...origin/feature/x [gone]", "feature/x", 0, 0),
            ("## No commits yet on trunk", "trunk", 0, 0),
            ("## Initial commit on trunk", "trunk", 0, 0),
        ];
        for (line, name, ahead, behind) in cases {
            let b = st(&format!("{line}\0")).branch.unwrap_or_else(|| panic!("{line}"));
            assert_eq!((b.name.as_deref(), b.ahead, b.behind), (Some(name), ahead, behind), "{line}");
        }
        let b = st("## HEAD (no branch)\0").branch.unwrap();
        assert_eq!((b.name, b.ahead, b.behind), (None, 0, 0));
    }

    #[test]
    fn the_summary_is_what_the_mode_line_shows() {
        assert_eq!(st("## main...o/main\0").summary(), "main");
        assert_eq!(st("## main...o/main [ahead 1]\0 M x\0").summary(), "main*↑1");
        assert_eq!(st("## main...o/main [ahead 1, behind 2]\0").summary(), "main↑1↓2");
        assert_eq!(st("## HEAD (no branch)\0?? x\0").summary(), "detached*");
        assert_eq!(st("").summary(), "?", "no branch line at all");
    }

    #[test]
    fn directories_roll_up_what_is_inside() {
        let g = st("## m\0 M src/deep/x.rs\0A  src/new.rs\0?? other/t.rs\0M  both/a\0 M both/b\0");
        assert_eq!(xy(&g, "src").as_deref(), Some("AM"), "a staged add and an unstaged change inside");
        assert_eq!(xy(&g, "src/deep").as_deref(), Some(" M"));
        assert_eq!(xy(&g, "other").as_deref(), Some(" ?"), "untracked inside shows only in the second column");
        assert_eq!(xy(&g, "both").as_deref(), Some("MM"), "staged and unstaged changes both show");
        assert_eq!(xy(&g, "nothing-here"), None);
    }

    #[test]
    fn rollup_picks_the_most_important_letter_per_column() {
        let g = st("## m\0M  d/a\0A  d/b\0R  d/c\0 M d/e\0 D d/f\0?? d/g\0");
        assert_eq!(xy(&g, "d").as_deref(), Some("RD"), "staged: R > A > M; unstaged: D > M > ?");
        let g = st("## m\0M  d/a\0 M d/b\0");
        assert_eq!(xy(&g, "d").as_deref(), Some("MM"));
    }

    #[test]
    fn every_unmerged_state_shows_as_uu() {
        for code in ["DD", "AU", "UD", "UA", "DU", "AA", "UU"] {
            let g = st(&format!("## m\0{code} d/c\0"));
            assert_eq!(xy(&g, "d/c").as_deref(), Some("UU"), "{code}");
            assert_eq!(xy(&g, "d").as_deref(), Some("UU"), "{code} rolls up");
        }
    }

    #[test]
    fn an_explicit_directory_entry_is_not_overwritten_by_rollup() {
        let g = st("## m\0?? a/b/\0 M a/c\0");
        assert_eq!(xy(&g, "a/b").as_deref(), Some("??"));
        assert_eq!(xy(&g, "a").as_deref(), Some(" M"), "the parent rolls up both children");
    }

    #[test]
    fn the_prefix_places_the_buffer_inside_the_repository() {
        let g = st("## m\0 M src/a.rs\0").with_prefix("src/");
        assert_eq!(xy(&g, "a.rs").as_deref(), Some(" M"));
        assert_eq!(xy(&g, "src/a.rs"), None, "keys are relative to the buffer, not the repository");
        let top = st("## m\0 M a.rs\0").with_prefix("");
        assert_eq!(xy(&top, "a.rs").as_deref(), Some(" M"));
    }

    #[test]
    fn empty_and_garbage_input_is_harmless() {
        let g = st("");
        assert!(!g.dirty() && g.branch.is_none());
        let g = st("\0\0x\0??\0 \0##\0");
        assert!(!g.dirty());
    }
}
