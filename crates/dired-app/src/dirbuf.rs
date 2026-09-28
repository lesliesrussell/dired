// dired-7qs
//! Directory buffer state: listings (root plus inserted subdirectories),
//! per-entry marks keyed by stable identity, and a virtualized view.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dired_domain::sort::merge_sorted;
use dired_domain::{Entry, EntryMetadata, EntryState, FileKind, SortSpec};
use dired_fs::RawDirEntry;

/// Entries of one directory (the root or an inserted subdirectory).
#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Receives a refresh scan; swapped in when the scan completes.
    pub staging: Option<Vec<Entry>>,
    pub gen: u64,
    pub scanning: bool,
    pub meta_pending: bool,
    pub meta_done: usize,
    pub error: Option<String>,
    pub cancel: Arc<AtomicBool>,
    index: Option<HashMap<PathBuf, usize>>,
}

impl Listing {
    fn index(&mut self) -> &HashMap<PathBuf, usize> {
        if self.index.is_none() {
            self.index = Some(
                self.entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (e.key.clone(), i))
                    .collect(),
            );
        }
        self.index.as_ref().unwrap()
    }
}

/// One visible line: an entry in a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub dir: PathBuf,
    pub idx: usize,
    pub depth: usize,
}

/// Wdired editing state.
#[derive(Debug, Default, Clone)]
pub struct Wdired {
    pub edits: HashMap<PathBuf, String>,
    /// Cursor position (in chars) within the name at point.
    pub col: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub marked: usize,
    pub flagged: usize,
    pub entries: usize,
}

pub struct DirBuffer {
    /// User-visible absolute path.
    pub root: PathBuf,
    pub listings: HashMap<PathBuf, Listing>,
    pub states: HashMap<PathBuf, EntryState>,
    pub annotations: HashMap<PathBuf, String>,
    pub sort: SortSpec,
    pub filter: Option<String>,
    pub show_hidden: bool,
    pub point: usize,
    point_key: Option<PathBuf>,
    pub top: usize,
    view: Vec<Row>,
    view_dirty: bool,
    resort: bool,
    counts: Counts,
    pub wdired: Option<Wdired>,
    gen_counter: u64,
}

impl DirBuffer {
    pub fn new(root: PathBuf, sort: SortSpec, show_hidden: bool) -> Self {
        let mut listings = HashMap::new();
        listings.insert(
            PathBuf::new(),
            Listing { entries: vec![Entry::parent_link()], ..Default::default() },
        );
        DirBuffer {
            root,
            listings,
            states: HashMap::new(),
            annotations: HashMap::new(),
            sort,
            filter: None,
            show_hidden,
            point: 0,
            point_key: None,
            top: 0,
            view: Vec::new(),
            view_dirty: true,
            resort: false,
            counts: Counts::default(),
            wdired: None,
            gen_counter: 0,
        }
    }

    pub fn abs(&self, key: &Path) -> PathBuf {
        if key == Path::new("..") {
            return self.root.parent().unwrap_or(&self.root).to_path_buf();
        }
        self.root.join(key)
    }

    // ----- scanning -----

    /// Begin (re)scanning a listing. Returns (absolute dir, generation, cancel flag).
    pub fn begin_scan(&mut self, dir: &Path) -> (PathBuf, u64, Arc<AtomicBool>) {
        self.gen_counter += 1;
        let gen = self.gen_counter;
        let abs = self.abs(dir);
        let l = self.listings.entry(dir.to_path_buf()).or_default();
        l.cancel.store(true, Ordering::Relaxed);
        l.cancel = Arc::new(AtomicBool::new(false));
        l.gen = gen;
        l.scanning = true;
        l.meta_pending = true;
        l.meta_done = 0;
        l.error = None;
        let fresh = l.entries.iter().all(|e| e.special);
        l.staging = if fresh { None } else { Some(Vec::new()) };
        (abs, gen, l.cancel.clone())
    }

    pub fn is_scanning(&self) -> bool {
        self.listings.values().any(|l| l.scanning || l.meta_pending)
    }

    pub fn cancel_scans(&mut self) -> bool {
        let mut any = false;
        for l in self.listings.values_mut() {
            if l.scanning || l.meta_pending {
                l.cancel.store(true, Ordering::Relaxed);
                l.scanning = false;
                l.meta_pending = false;
                if let Some(st) = l.staging.take() {
                    if l.entries.len() <= 1 {
                        l.entries.extend(st);
                    }
                }
                any = true;
            }
        }
        self.view_dirty = true;
        any
    }

    pub fn cancel_all(&mut self) {
        for l in self.listings.values() {
            l.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn scan_batch(&mut self, dir: &Path, gen: u64, raw: Vec<RawDirEntry>) {
        let spec = self.sort;
        let Some(l) = self.listings.get_mut(dir) else { return };
        if l.gen != gen {
            return;
        }
        let batch: Vec<Entry> = raw
            .into_iter()
            .map(|r| Entry::new(r.name, dir, r.kind))
            .collect();
        match &mut l.staging {
            Some(st) => st.extend(batch),
            None => {
                merge_sorted(&mut l.entries, batch, &spec);
                l.index = None;
                self.view_dirty = true;
            }
        }
    }

    pub fn scan_done(&mut self, dir: &Path, gen: u64, error: Option<String>) {
        let spec = self.sort;
        let Some(l) = self.listings.get_mut(dir) else { return };
        if l.gen != gen {
            return;
        }
        l.scanning = false;
        if error.is_some() {
            l.meta_pending = false;
        }
        l.error = error;
        if let Some(mut st) = l.staging.take() {
            // Keep known metadata so columns don't flicker while re-enriching.
            let old: HashMap<PathBuf, Option<EntryMetadata>> =
                l.entries.drain(..).map(|e| (e.key, e.meta)).collect();
            for e in &mut st {
                if let Some(Some(m)) = old.get(&e.key) {
                    e.meta = Some(m.clone());
                    e.kind = m.kind;
                }
            }
            if dir.as_os_str().is_empty() {
                st.push(Entry::parent_link());
            }
            st.sort_by(|a, b| spec.compare(a, b));
            l.entries = st;
            l.index = None;
        }
        self.view_dirty = true;
    }

    pub fn meta_batch(&mut self, dir: &Path, gen: u64, metas: Vec<(PathBuf, Result<EntryMetadata, String>)>) {
        let needs = self.sort.key.needs_metadata();
        let Some(l) = self.listings.get_mut(dir) else { return };
        if l.gen != gen {
            return;
        }
        let mut reorder = needs;
        for (key, m) in metas {
            l.meta_done += 1;
            let Some(&i) = l.index().get(&key) else { continue };
            let e = &mut l.entries[i];
            match m {
                Ok(m) => {
                    if e.kind != m.kind || m.target_is_dir {
                        reorder = true;
                    }
                    e.kind = m.kind;
                    e.meta = Some(m);
                }
                Err(err) => {
                    self.annotations.insert(key, err);
                }
            }
        }
        if reorder {
            self.resort = true;
        }
        self.view_dirty = true;
    }

    pub fn meta_done(&mut self, dir: &Path, gen: u64) {
        if let Some(l) = self.listings.get_mut(dir) {
            if l.gen == gen {
                l.meta_pending = false;
            }
        }
    }

    // ----- view -----

    pub fn invalidate(&mut self) {
        self.view_dirty = true;
    }

    pub fn set_sort(&mut self, sort: SortSpec) {
        self.sort = sort;
        self.resort = true;
        self.view_dirty = true;
    }

    fn visible(&self, e: &Entry) -> bool {
        if e.special {
            return true;
        }
        if !self.show_hidden && e.is_hidden() {
            return false;
        }
        match &self.filter {
            Some(f) => e.name.to_lowercase().contains(&f.to_lowercase()),
            None => true,
        }
    }

    fn emit(&self, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        let Some(l) = self.listings.get(dir) else { return };
        for (idx, e) in l.entries.iter().enumerate() {
            if depth > 0 && e.special {
                continue;
            }
            if !self.visible(e) {
                continue;
            }
            out.push(Row { dir: dir.to_path_buf(), idx, depth });
            if !e.special && self.listings.contains_key(&e.key) {
                self.emit(&e.key, depth + 1, out);
            }
        }
    }

    /// Rebuild the flattened view if needed. Cheap when clean.
    pub fn ensure_view(&mut self) {
        if !self.view_dirty {
            return;
        }
        if self.resort {
            let spec = self.sort;
            for l in self.listings.values_mut() {
                l.entries.sort_by(|a, b| spec.compare(a, b));
                l.index = None;
            }
            self.resort = false;
        }
        let mut view = Vec::with_capacity(self.view.len().max(16));
        self.emit(Path::new(""), 0, &mut view);
        self.view = view;
        self.view_dirty = false;
        let mut c = Counts::default();
        for l in self.listings.values() {
            c.entries += l.entries.iter().filter(|e| !e.special).count();
            if !self.states.is_empty() {
                for e in &l.entries {
                    match self.states.get(&e.key) {
                        Some(EntryState::Marked) => c.marked += 1,
                        Some(EntryState::DeleteFlagged) => c.flagged += 1,
                        _ => {}
                    }
                }
            }
        }
        self.counts = c;
        // Relocate point by identity; keep a not-yet-seen key while scanning.
        let scanning = self.is_scanning();
        match self.point_key.clone().map(|k| self.find_row(&k)) {
            Some(Some(i)) => self.point = i,
            Some(None) if scanning => {
                self.point = self.point.min(self.view.len().saturating_sub(1))
            }
            _ => {
                self.point = self.point.min(self.view.len().saturating_sub(1));
                self.point_key = self.view.get(self.point).map(|r| self.entry(r).key.clone());
            }
        }
    }

    // dired-ta2
    /// Point, scroll offset, and point identity, for per-window views.
    pub fn view_state(&self) -> (usize, usize, Option<PathBuf>) {
        (self.point, self.top, self.point_key.clone())
    }

    // dired-ta2
    pub fn set_view_state(&mut self, point: usize, top: usize, key: Option<PathBuf>) {
        self.point = point;
        self.top = top;
        if key.is_some() {
            self.point_key = key;
        }
        self.view_dirty = true;
    }

    pub fn find_row(&self, key: &Path) -> Option<usize> {
        self.view.iter().position(|r| self.entry(r).key == key)
    }

    pub fn view(&mut self) -> &[Row] {
        self.ensure_view();
        &self.view
    }

    /// View without rebuilding (for rendering after `ensure_view`).
    pub fn rows(&self) -> &[Row] {
        &self.view
    }

    pub fn counts(&mut self) -> Counts {
        self.ensure_view();
        self.counts
    }

    pub fn entry(&self, r: &Row) -> &Entry {
        &self.listings[&r.dir].entries[r.idx]
    }

    pub fn len(&mut self) -> usize {
        self.view().len()
    }

    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }

    pub fn set_point(&mut self, i: usize) {
        self.ensure_view();
        if self.view.is_empty() {
            self.point = 0;
            return;
        }
        self.point = i.min(self.view.len() - 1);
        self.point_key = Some(self.entry(&self.view[self.point]).key.clone());
        if let Some(w) = &mut self.wdired {
            w.col = usize::MAX;
        }
    }

    pub fn goto_key(&mut self, key: &Path) -> bool {
        self.ensure_view();
        match self.find_row(key) {
            Some(i) => {
                self.set_point(i);
                true
            }
            None => {
                self.point_key = Some(key.to_path_buf());
                false
            }
        }
    }

    pub fn point_row(&mut self) -> Option<Row> {
        self.ensure_view();
        self.view.get(self.point).cloned()
    }

    pub fn point_entry(&mut self) -> Option<Entry> {
        let r = self.point_row()?;
        Some(self.entry(&r).clone())
    }

    pub fn state(&self, key: &Path) -> EntryState {
        self.states.get(key).copied().unwrap_or(EntryState::Unmarked)
    }

    pub fn set_state(&mut self, key: &Path, s: EntryState) {
        if s == EntryState::Unmarked {
            self.states.remove(key);
        } else {
            self.states.insert(key.to_path_buf(), s);
        }
        self.view_dirty = true;
    }

    /// Visible actionable entries (not `..`).
    pub fn visible_entries(&mut self) -> Vec<Entry> {
        self.ensure_view();
        self.view
            .iter()
            .map(|r| self.entry(r))
            .filter(|e| !e.special)
            .cloned()
            .collect()
    }

    /// Batch operands: visible marked entries, else the entry at point.
    pub fn operands(&mut self) -> Vec<Entry> {
        let marked: Vec<Entry> = self
            .visible_entries()
            .into_iter()
            .filter(|e| self.state(&e.key) == EntryState::Marked)
            .collect();
        if !marked.is_empty() {
            return marked;
        }
        match self.point_entry() {
            Some(e) if !e.special => vec![e],
            _ => vec![],
        }
    }

    /// Every flagged entry present in the buffer, including hidden or
    /// filtered-out ones, so the preview matches the status-line count.
    pub fn flagged(&mut self) -> Vec<Entry> {
        self.ensure_view();
        let mut v: Vec<Entry> = self
            .listings
            .values()
            .flat_map(|l| l.entries.iter())
            .filter(|e| !e.special && self.state(&e.key) == EntryState::DeleteFlagged)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.key.cmp(&b.key));
        v
    }

    /// Remove an inserted subtree and everything below it.
    pub fn collapse(&mut self, key: &Path) {
        let keys: Vec<PathBuf> = self
            .listings
            .keys()
            .filter(|k| !k.as_os_str().is_empty() && k.starts_with(key))
            .cloned()
            .collect();
        for k in keys {
            if let Some(l) = self.listings.remove(&k) {
                l.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.view_dirty = true;
    }

    /// Re-key marks and annotations after successful renames. Applied as
    /// one batch so swaps (`a → b`, `b → a`) don't clobber each other.
    pub fn remap(&mut self, moves: &[(PathBuf, PathBuf)]) {
        let taken: Vec<Option<EntryState>> = moves.iter().map(|(from, _)| self.states.remove(from)).collect();
        for (from, _) in moves {
            self.annotations.remove(from);
        }
        for ((_, to), s) in moves.iter().zip(taken) {
            if let Some(s) = s {
                self.states.insert(to.clone(), s);
            }
        }
        if let Some((_, to)) = moves.iter().find(|(from, _)| self.point_key.as_deref() == Some(from.as_path())) {
            self.point_key = Some(to.clone());
        }
        self.view_dirty = true;
    }

    pub fn key_for(&self, abs: &Path) -> Option<PathBuf> {
        abs.strip_prefix(&self.root).ok().map(Path::to_path_buf).filter(|k| !k.as_os_str().is_empty())
    }

    /// Names present in the listing that contains `abs_dir` (for rename validation).
    pub fn names_in(&self, abs_dir: &Path) -> Option<std::collections::HashSet<std::ffi::OsString>> {
        let key = if abs_dir == self.root { PathBuf::new() } else { self.key_for(abs_dir)? };
        self.listings.get(&key).map(|l| {
            l.entries.iter().filter(|e| !e.special).map(|e| e.os_name.clone()).collect()
        })
    }

    pub fn has_kind(&self, key: &Path, kind: FileKind) -> bool {
        self.listings.values().any(|l| l.entries.iter().any(|e| e.key == key && e.kind == kind))
    }

    /// Display name at a row, honoring Wdired edits.
    pub fn display_name(&self, r: &Row) -> String {
        let e = self.entry(r);
        self.wdired
            .as_ref()
            .and_then(|w| w.edits.get(&e.key).cloned())
            .unwrap_or_else(|| e.name.clone())
    }

    pub fn adjust_top(&mut self, height: usize) {
        let h = height.max(1);
        if self.point < self.top {
            self.top = self.point;
        } else if self.point >= self.top + h {
            self.top = self.point + 1 - h;
        }
    }

    pub fn scan_summary(&self) -> String {
        let root = &self.listings[Path::new("")];
        let scanning = self.listings.values().any(|l| l.scanning);
        let meta = self.listings.values().any(|l| l.meta_pending);
        let n = root.entries.len().saturating_sub(1)
            + root.staging.as_ref().map(|s| s.len()).unwrap_or(0);
        if let Some(e) = &root.error {
            format!("error: {e}")
        } else if scanning {
            format!("scanning {n}")
        } else if meta {
            format!("stat {}/{}", root.meta_done, n)
        } else {
            "done".into()
        }
    }
}
