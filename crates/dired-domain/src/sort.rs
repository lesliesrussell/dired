// dired-7qs
//! Entry ordering.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::entry::Entry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortKey {
    Name,
    Natural,
    Time,
    Size,
    Extension,
}

impl SortKey {
    pub const ALL: [SortKey; 5] = [
        SortKey::Name,
        SortKey::Natural,
        SortKey::Time,
        SortKey::Size,
        SortKey::Extension,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Natural => "natural",
            SortKey::Time => "time",
            SortKey::Size => "size",
            SortKey::Extension => "extension",
        }
    }

    pub fn parse(s: &str) -> Option<SortKey> {
        SortKey::ALL.into_iter().find(|k| k.label() == s)
    }

    pub fn next(self) -> SortKey {
        let i = SortKey::ALL.iter().position(|k| *k == self).unwrap_or(0);
        SortKey::ALL[(i + 1) % SortKey::ALL.len()]
    }

    /// Whether ordering depends on metadata that arrives after enumeration.
    pub fn needs_metadata(self) -> bool {
        matches!(self, SortKey::Time | SortKey::Size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortSpec {
    pub key: SortKey,
    pub dirs_first: bool,
    pub reverse: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        SortSpec {
            key: SortKey::Name,
            dirs_first: true,
            reverse: false,
        }
    }
}

impl SortSpec {
    pub fn label(&self) -> String {
        let mut s = self.key.label().to_string();
        if self.reverse {
            s.push_str(" rev");
        }
        s
    }

    pub fn compare(&self, a: &Entry, b: &Entry) -> Ordering {
        if a.special != b.special {
            return if a.special { Ordering::Less } else { Ordering::Greater };
        }
        if self.dirs_first {
            let (ad, bd) = (a.is_dir_like(), b.is_dir_like());
            if ad != bd {
                return if ad { Ordering::Less } else { Ordering::Greater };
            }
        }
        let ord = match self.key {
            SortKey::Name => a.name.cmp(&b.name),
            SortKey::Natural => natural_cmp(&a.name, &b.name),
            // Newest first, like `ls -t`.
            SortKey::Time => {
                let at = a.meta.as_ref().and_then(|m| m.modified);
                let bt = b.meta.as_ref().and_then(|m| m.modified);
                bt.cmp(&at)
            }
            // Largest first, like `ls -S`.
            SortKey::Size => {
                let asz = a.meta.as_ref().map(|m| m.size).unwrap_or(0);
                let bsz = b.meta.as_ref().map(|m| m.size).unwrap_or(0);
                bsz.cmp(&asz)
            }
            SortKey::Extension => a.extension().cmp(b.extension()),
        }
        .then_with(|| a.name.cmp(&b.name));
        if self.reverse {
            ord.reverse()
        } else {
            ord
        }
    }
}

/// Natural ("version") ordering: digit runs compare numerically, other text
/// compares case-insensitively.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) if ac.is_ascii_digit() && bc.is_ascii_digit() => {
                let mut an = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    an.push(c);
                    ai.next();
                }
                let mut bn = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    bn.push(c);
                    bi.next();
                }
                let at = an.trim_start_matches('0');
                let bt = bn.trim_start_matches('0');
                let ord = at.len().cmp(&bt.len()).then_with(|| at.cmp(bt));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(ac), Some(bc)) => {
                let ord = ac.to_lowercase().cmp(bc.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// Merge a sorted batch into a sorted vector in O(n + m).
pub fn merge_sorted(existing: &mut Vec<Entry>, mut batch: Vec<Entry>, spec: &SortSpec) {
    batch.sort_by(|a, b| spec.compare(a, b));
    if existing.is_empty() {
        *existing = batch;
        return;
    }
    let old = std::mem::take(existing);
    let mut out = Vec::with_capacity(old.len() + batch.len());
    let mut a = old.into_iter().peekable();
    let mut b = batch.into_iter().peekable();
    loop {
        match (a.peek(), b.peek()) {
            (Some(x), Some(y)) => {
                if spec.compare(x, y) != Ordering::Greater {
                    out.push(a.next().unwrap());
                } else {
                    out.push(b.next().unwrap());
                }
            }
            (Some(_), None) => out.push(a.next().unwrap()),
            (None, Some(_)) => out.push(b.next().unwrap()),
            (None, None) => break,
        }
    }
    *existing = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["file10", "file2", "File1", "file02b"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["File1", "file2", "file02b", "file10"]);
    }
}
