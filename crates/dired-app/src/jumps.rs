// dired-s23
//! Bookmarks and frecency-ranked directories for the jump picker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Most automatically tracked directories kept; the lowest-ranked go first.
pub const MAX_VISITS: usize = 500;

const FILE: &str = "jumps.json";

/// How often and how recently a directory was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visit {
    pub count: u32,
    /// Unix seconds of the latest visit.
    pub last: u64,
}

/// zoxide-style frecency: the visit count weighted by how recent the last
/// visit was (the last hour 4x, the last day 2x, the last week 1/2x, older 1/4x).
pub fn score(v: &Visit, now: u64) -> f64 {
    let age = now.saturating_sub(v.last);
    let weight = match age {
        a if a < 3_600 => 4.0,
        a if a < 86_400 => 2.0,
        a if a < 604_800 => 0.5,
        _ => 0.25,
    };
    v.count as f64 * weight
}

#[derive(Debug, Clone, Default)]
pub struct Jumps {
    /// `(name, directory)` in the order they were first set.
    pub bookmarks: Vec<(String, PathBuf)>,
    pub visits: BTreeMap<PathBuf, Visit>,
}

/// The on-disk form: plain strings, so unusual paths cannot break saving.
#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    bookmarks: Vec<(String, String)>,
    #[serde(default)]
    visits: BTreeMap<String, Visit>,
}

impl Jumps {
    pub fn record_visit(&mut self, path: &Path, now: u64) {
        let v = self.visits.entry(path.to_path_buf()).or_insert(Visit { count: 0, last: now });
        v.count = v.count.saturating_add(1);
        v.last = now;
        if self.visits.len() > MAX_VISITS {
            let worst = self
                .visits
                .iter()
                .min_by(|a, b| {
                    score(a.1, now)
                        .partial_cmp(&score(b.1, now))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.1.last.cmp(&b.1.last))
                        .then(a.0.cmp(b.0))
                })
                .map(|(p, _)| p.clone());
            if let Some(w) = worst {
                self.visits.remove(&w);
            }
        }
    }

    pub fn forget(&mut self, path: &Path) {
        self.visits.remove(path);
    }

    /// Returns whether an existing bookmark was updated (rather than added).
    pub fn set_bookmark(&mut self, name: &str, path: PathBuf) -> bool {
        match self.bookmarks.iter_mut().find(|(n, _)| n == name) {
            Some(b) => {
                b.1 = path;
                true
            }
            None => {
                self.bookmarks.push((name.to_string(), path));
                false
            }
        }
    }

    pub fn delete_bookmark(&mut self, name: &str) -> bool {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|(n, _)| n != name);
        self.bookmarks.len() != before
    }

    /// Visited directories, best first: score, then most recent, then path.
    pub fn ranked(&self, now: u64, exclude: Option<&Path>) -> Vec<PathBuf> {
        let mut v: Vec<(&PathBuf, &Visit)> =
            self.visits.iter().filter(|(p, _)| Some(p.as_path()) != exclude).collect();
        v.sort_by(|a, b| {
            score(b.1, now)
                .partial_cmp(&score(a.1, now))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.1.last.cmp(&a.1.last))
                .then(a.0.cmp(b.0))
        });
        v.into_iter().map(|(p, _)| p.clone()).collect()
    }

    /// The picker's lines with the directory each opens: bookmarks first, then
    /// ranked directories that are not already bookmarked.
    pub fn entries(&self, now: u64, exclude: Option<&Path>) -> Vec<(String, PathBuf)> {
        let mut out: Vec<(String, PathBuf)> = self
            .bookmarks
            .iter()
            .map(|(n, p)| (format!("★ {n}  {}", p.display()), p.clone()))
            .collect();
        for p in self.ranked(now, exclude) {
            if !self.bookmarks.iter().any(|(_, b)| *b == p) {
                out.push((format!("  {}", p.display()), p));
            }
        }
        out
    }

    /// Read `jumps.json` from `dir`; anything unreadable is simply empty.
    /// Visits are only read when `with_visits`.
    pub fn load(dir: &Path, with_visits: bool) -> Jumps {
        let stored: Stored = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Jumps {
            bookmarks: stored.bookmarks.into_iter().map(|(n, p)| (n, PathBuf::from(p))).collect(),
            visits: if with_visits {
                stored.visits.into_iter().map(|(p, v)| (PathBuf::from(p), v)).collect()
            } else {
                BTreeMap::new()
            },
        }
    }

    /// Write `jumps.json`; visits only when `with_visits`. Failures are ignored.
    pub fn save(&self, dir: &Path, with_visits: bool) {
        let stored = Stored {
            bookmarks: self
                .bookmarks
                .iter()
                .filter_map(|(n, p)| p.to_str().map(|p| (n.clone(), p.to_string())))
                .collect(),
            visits: if with_visits {
                self.visits.iter().filter_map(|(p, v)| p.to_str().map(|p| (p.to_string(), *v))).collect()
            } else {
                BTreeMap::new()
            },
        };
        if let Ok(text) = serde_json::to_string(&stored) {
            let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(dir.join(FILE), text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_750_000_000;
    const HOUR: u64 = 3_600;
    const DAY: u64 = 86_400;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn visit(j: &mut Jumps, path: &str, n: u32, ago: u64) {
        for _ in 0..n {
            j.record_visit(&p(path), NOW - ago);
        }
    }

    #[test]
    fn recent_visits_weigh_more_than_old_ones() {
        let v = |count, ago| Visit { count, last: NOW - ago };
        assert_eq!(score(&v(1, 0), NOW), 4.0);
        assert_eq!(score(&v(1, HOUR - 1), NOW), 4.0);
        assert_eq!(score(&v(1, HOUR), NOW), 2.0);
        assert_eq!(score(&v(1, DAY), NOW), 0.5);
        assert_eq!(score(&v(1, 7 * DAY), NOW), 0.25);
        assert_eq!(score(&v(10, 30 * DAY), NOW), 2.5);
        // A last-visit time in the future (clock change) counts as just now.
        assert_eq!(score(&Visit { count: 1, last: NOW + 99 }, NOW), 4.0);
    }

    #[test]
    fn ranking_prefers_frequent_then_recent_and_excludes_the_current_directory() {
        let mut j = Jumps::default();
        visit(&mut j, "/old-favourite", 20, 30 * DAY); // 20 * 0.25 = 5
        visit(&mut j, "/just-now", 1, 10); // 4
        visit(&mut j, "/today", 1, 2 * HOUR); // 2
        visit(&mut j, "/here", 50, 0);
        let ranked = j.ranked(NOW, Some(&p("/here")));
        assert_eq!(ranked, [p("/old-favourite"), p("/just-now"), p("/today")]);
        // Equal scores: the more recent visit first, then path order.
        let mut t = Jumps::default();
        visit(&mut t, "/b", 1, 10);
        visit(&mut t, "/a", 1, 20);
        visit(&mut t, "/c", 1, 10);
        assert_eq!(t.ranked(NOW, None), [p("/b"), p("/c"), p("/a")]);
    }

    #[test]
    fn visits_are_capped_dropping_the_lowest_ranked() {
        let mut j = Jumps::default();
        visit(&mut j, "/precious", 3, 30 * DAY);
        for i in 0..MAX_VISITS + 20 {
            j.record_visit(&p(&format!("/d{i}")), NOW);
        }
        assert_eq!(j.visits.len(), MAX_VISITS);
        assert!(j.visits.contains_key(&p(&format!("/d{}", MAX_VISITS + 19))), "the newest survives");
        assert!(!j.visits.contains_key(&p("/precious")), "the old, low-scoring entry went first");
    }

    #[test]
    fn bookmarks_are_ordered_unique_and_updatable() {
        let mut j = Jumps::default();
        assert!(!j.set_bookmark("proj", p("/a")), "new");
        assert!(!j.set_bookmark("docs", p("/b")), "new");
        assert!(j.set_bookmark("proj", p("/c")), "updated");
        assert_eq!(j.bookmarks, [("proj".to_string(), p("/c")), ("docs".to_string(), p("/b"))]);
        assert!(j.delete_bookmark("proj"));
        assert!(!j.delete_bookmark("proj"));
        assert_eq!(j.bookmarks.len(), 1);
    }

    #[test]
    fn entries_list_bookmarks_first_and_do_not_repeat_bookmarked_directories() {
        let mut j = Jumps::default();
        j.set_bookmark("proj", p("/work/proj"));
        visit(&mut j, "/work/proj", 5, 0); // bookmarked: shown once, as a bookmark
        visit(&mut j, "/tmp/x", 1, 0);
        visit(&mut j, "/here", 9, 0);
        let lines: Vec<String> = j.entries(NOW, Some(&p("/here"))).into_iter().map(|(l, _)| l).collect();
        assert_eq!(lines, ["★ proj  /work/proj", "  /tmp/x"]);
        let e = j.entries(NOW, Some(&p("/here")));
        assert_eq!(e[0].1, p("/work/proj"));
        assert_eq!(e[1].1, p("/tmp/x"));
    }

    #[test]
    fn forget_removes_a_visit_but_keeps_bookmarks() {
        let mut j = Jumps::default();
        j.set_bookmark("proj", p("/gone"));
        visit(&mut j, "/gone", 1, 0);
        j.forget(&p("/gone"));
        assert!(j.visits.is_empty());
        assert_eq!(j.bookmarks.len(), 1);
    }

    #[test]
    fn save_and_load_round_trip_with_or_without_visits() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Jumps::default();
        j.set_bookmark("proj", p("/a"));
        visit(&mut j, "/b", 2, 0);
        j.save(dir.path(), true);
        let back = Jumps::load(dir.path(), true);
        assert_eq!(back.bookmarks, j.bookmarks);
        assert_eq!(back.visits, j.visits);

        // Visits are not stored (or read) when persistence of automatic data is off.
        j.save(dir.path(), false);
        let back = Jumps::load(dir.path(), false);
        assert_eq!(back.bookmarks, j.bookmarks);
        assert!(back.visits.is_empty());
        j.save(dir.path(), true);
        assert!(Jumps::load(dir.path(), false).visits.is_empty(), "ignored on load too");
    }

    #[test]
    fn missing_and_corrupt_files_load_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Jumps::load(dir.path(), true).bookmarks.is_empty());
        std::fs::write(dir.path().join("jumps.json"), "{ not json").unwrap();
        let j = Jumps::load(dir.path(), true);
        assert!(j.bookmarks.is_empty() && j.visits.is_empty());
        assert!(Jumps::load(Path::new("/definitely/not/a/dir"), true).visits.is_empty());
    }
}
