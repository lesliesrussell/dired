// dired-7qs
//! Pure operation planning: every mutating operation is expressed as an
//! [`OpPlan`] that can be previewed, validated, executed, and journaled.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::entry::EntryMetadata;

pub type OpId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpKind {
    Copy,
    Move,
    Trash,
    DeletePermanent,
    /// Batch rename from Wdired or `% r`.
    Rename,
    Chmod,
    Mkdir,
}

impl OpKind {
    pub fn label(self) -> &'static str {
        match self {
            OpKind::Copy => "Copy",
            OpKind::Move => "Move/Rename",
            OpKind::Trash => "Delete (trash)",
            OpKind::DeletePermanent => "Delete PERMANENTLY",
            OpKind::Rename => "Rename",
            OpKind::Chmod => "Change mode",
            OpKind::Mkdir => "Create directory",
        }
    }

    pub fn has_destination(self) -> bool {
        matches!(self, OpKind::Copy | OpKind::Move | OpKind::Rename)
    }
}

/// A blocking per-item problem; execution cannot start while any conflict
/// has an unresolved (`Ask`) resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Conflict {
    DestinationExists,
    DuplicateDestination,
    SelfRecursive,
    /// The destination is an ancestor of the source; overwriting it would
    /// destroy the source.
    DestinationContainsSource,
    SameFile,
    Inaccessible(String),
    InvalidName(String),
    /// A Wdired target collides with an existing, unrenamed entry.
    Collision(String),
}

impl Conflict {
    pub fn describe(&self) -> String {
        match self {
            Conflict::DestinationExists => "destination exists".into(),
            Conflict::DuplicateDestination => "duplicate destination".into(),
            Conflict::SelfRecursive => "destination is inside source".into(),
            Conflict::DestinationContainsSource => "destination contains the source".into(),
            Conflict::SameFile => "source and destination are the same".into(),
            Conflict::Inaccessible(e) => format!("inaccessible: {e}"),
            Conflict::InvalidName(e) => format!("invalid name: {e}"),
            Conflict::Collision(e) => format!("collision: {e}"),
        }
    }

    pub fn valid_resolutions(&self) -> &'static [ResolutionKind] {
        match self {
            Conflict::DestinationExists => &[
                ResolutionKind::Overwrite,
                ResolutionKind::Skip,
                ResolutionKind::Rename,
            ],
            Conflict::DuplicateDestination => &[ResolutionKind::Skip, ResolutionKind::Rename],
            Conflict::SelfRecursive
            | Conflict::DestinationContainsSource
            | Conflict::SameFile
            | Conflict::Inaccessible(_) => &[ResolutionKind::Skip],
            // Wdired problems must be fixed by editing names.
            Conflict::InvalidName(_) | Conflict::Collision(_) => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionKind {
    Overwrite,
    Skip,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    Ask,
    Overwrite,
    Skip,
    RenameTo(PathBuf),
}

impl Resolution {
    fn kind(&self) -> Option<ResolutionKind> {
        match self {
            Resolution::Ask => None,
            Resolution::Overwrite => Some(ResolutionKind::Overwrite),
            Resolution::Skip => Some(ResolutionKind::Skip),
            Resolution::RenameTo(_) => Some(ResolutionKind::Rename),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Resolution::Ask => "ask".into(),
            Resolution::Overwrite => "overwrite".into(),
            Resolution::Skip => "skip".into(),
            Resolution::RenameTo(p) => format!("rename → {}", p.display()),
        }
    }
}

/// Pre-operation metadata captured for journaling and best-effort undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreMeta {
    pub is_dir: bool,
    pub size: u64,
    pub mode: Option<u32>,
    pub modified_unix: Option<i64>,
}

impl PreMeta {
    pub fn from_meta(m: &EntryMetadata) -> Self {
        PreMeta {
            is_dir: m.kind == crate::FileKind::Dir,
            size: m.size,
            mode: m.mode,
            modified_unix: m.modified.and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs() as i64)
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    /// Buffer identity of the source, for mark remapping.
    pub key: Option<PathBuf>,
    pub is_dir: bool,
    pub conflict: Option<Conflict>,
    pub resolution: Resolution,
    pub cross_device: bool,
    pub case_only: bool,
    pub dst_exists: bool,
    pub pre: Option<PreMeta>,
}

impl PlanItem {
    pub fn new(src: PathBuf, dst: Option<PathBuf>) -> Self {
        PlanItem {
            src,
            dst,
            key: None,
            is_dir: false,
            conflict: None,
            resolution: Resolution::Ask,
            cross_device: false,
            case_only: false,
            dst_exists: false,
            pre: None,
        }
    }

    pub fn needs_decision(&self) -> bool {
        self.conflict.is_some() && self.resolution == Resolution::Ask
    }

    pub fn skipped(&self) -> bool {
        self.resolution == Resolution::Skip
    }

    /// Where the item will actually be written.
    pub fn effective_dst(&self) -> Option<&Path> {
        match &self.resolution {
            Resolution::RenameTo(p) => Some(p),
            _ => self.dst.as_deref(),
        }
    }

    pub fn state_label(&self) -> String {
        match (&self.conflict, &self.resolution) {
            (None, Resolution::Skip) => "skip".into(),
            (None, _) => "ok".into(),
            (Some(c), Resolution::Ask) => format!("CONFLICT: {} [ask]", c.describe()),
            (Some(c), r) => format!("{} [{}]", c.describe(), r.label()),
        }
    }
}

/// One primitive rename step in a Wdired schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameStep {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpPlan {
    pub id: OpId,
    pub kind: OpKind,
    pub items: Vec<PlanItem>,
    /// Non-blocking plan-level warnings.
    pub warnings: Vec<String>,
    /// Blocking plan-level errors; a plan with errors can never execute.
    pub errors: Vec<String>,
    /// Ordered rename schedule (Wdired / regexp rename).
    pub steps: Vec<RenameStep>,
    /// Human description of the resolved backend (e.g. deletion backend).
    pub backend: String,
    pub mode: Option<u32>,
}

impl OpPlan {
    pub fn new(id: OpId, kind: OpKind) -> Self {
        OpPlan {
            id,
            kind,
            items: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            steps: Vec::new(),
            backend: String::new(),
            mode: None,
        }
    }

    pub fn unresolved(&self) -> usize {
        self.items.iter().filter(|i| i.needs_decision()).count()
    }

    pub fn conflicts(&self) -> usize {
        self.items.iter().filter(|i| i.conflict.is_some()).count()
    }

    pub fn active_items(&self) -> usize {
        self.items.iter().filter(|i| !i.skipped()).count()
    }

    pub fn overwrite_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.resolution == Resolution::Overwrite)
            .count()
    }

    pub fn can_execute(&self) -> Result<(), String> {
        if let Some(e) = self.errors.first() {
            return Err(format!("plan has errors: {e}"));
        }
        let n = self.unresolved();
        if n > 0 {
            return Err(format!(
                "{n} conflict(s) need a policy (o overwrite, s skip, r rename, O overwrite all, S skip all)"
            ));
        }
        if self.active_items() == 0 {
            return Err("nothing to do: every item is skipped".into());
        }
        Ok(())
    }

    pub fn set_resolution(&mut self, idx: usize, res: Resolution) -> Result<(), String> {
        let others: HashSet<PathBuf> = self
            .items
            .iter()
            .enumerate()
            .filter(|(i, it)| *i != idx && !it.skipped())
            .filter_map(|(_, it)| it.effective_dst().map(Path::to_path_buf))
            .collect();
        if self.kind == OpKind::Rename && res != Resolution::Ask {
            return Err("batch renames are all-or-nothing: edit the names instead".into());
        }
        let item = self.items.get_mut(idx).ok_or("no item at point")?;
        if res == Resolution::Ask {
            item.resolution = Resolution::Ask;
            return Ok(());
        }
        let kind = res.kind().expect("non-ask resolution");
        match &item.conflict {
            Some(c) if !c.valid_resolutions().contains(&kind) => {
                return Err(format!("{:?} is not valid for: {}", kind, c.describe()));
            }
            None if kind != ResolutionKind::Skip => {
                return Err("item has no conflict; only skip applies".into());
            }
            _ => {}
        }
        if let Resolution::RenameTo(p) = &res {
            if others.contains(p) {
                return Err(format!("{} is already a destination", p.display()));
            }
            if *p == item.src || (item.is_dir && p.starts_with(&item.src)) {
                return Err("new destination is inside the source".into());
            }
            if item.src.starts_with(p) {
                return Err("new destination contains the source".into());
            }
            if item.dst.as_deref() == Some(p.as_path()) {
                return Err("that is the conflicting destination; choose a new name".into());
            }
        }
        item.resolution = res;
        Ok(())
    }

    /// Apply `kind` to every still-undecided conflict where it is valid.
    pub fn resolve_all(&mut self, kind: ResolutionKind) -> usize {
        let mut n = 0;
        for item in &mut self.items {
            if let Some(c) = &item.conflict {
                if item.resolution == Resolution::Ask && c.valid_resolutions().contains(&kind) {
                    item.resolution = match kind {
                        ResolutionKind::Overwrite => Resolution::Overwrite,
                        ResolutionKind::Skip => Resolution::Skip,
                        ResolutionKind::Rename => continue,
                    };
                    n += 1;
                }
            }
        }
        n
    }
}

/// Facts about a source gathered by the application layer.
#[derive(Debug, Clone)]
pub struct SourceFact {
    pub path: PathBuf,
    pub key: Option<PathBuf>,
    pub meta: Result<EntryMetadata, String>,
}

/// Facts about the destination the user typed.
#[derive(Debug, Clone)]
pub struct DestFact {
    pub path: PathBuf,
    /// Metadata of `path` (followed), `None` if it does not exist.
    pub meta: Option<EntryMetadata>,
    /// Device of the directory that will receive the items.
    pub dev: Option<u64>,
}

/// Compute destination paths for a copy/move, following Dired semantics:
/// an existing directory receives the items; otherwise a single source is
/// renamed to the destination path.
pub fn resolve_targets(sources: &[PathBuf], dest: &DestFact) -> Result<Vec<PathBuf>, String> {
    let dest_is_dir = dest
        .meta
        .as_ref()
        .map(|m| m.kind == crate::FileKind::Dir || m.target_is_dir)
        .unwrap_or(false);
    if dest_is_dir {
        sources
            .iter()
            .map(|s| {
                s.file_name()
                    .map(|n| dest.path.join(n))
                    .ok_or_else(|| format!("{} has no file name", s.display()))
            })
            .collect()
    } else if sources.len() == 1 {
        Ok(vec![dest.path.clone()])
    } else {
        Err(format!(
            "destination {} must be an existing directory for {} items",
            dest.path.display(),
            sources.len()
        ))
    }
}

fn same_file(a: &EntryMetadata, b: &EntryMetadata) -> bool {
    matches!((a.dev, a.ino, b.dev, b.ino), (Some(ad), Some(ai), Some(bd), Some(bi)) if ad == bd && ai == bi)
}

fn case_only(src: &Path, dst: &Path) -> bool {
    let (Some(sn), Some(dn)) = (src.file_name(), dst.file_name()) else {
        return false;
    };
    src.parent() == dst.parent()
        && sn != dn
        && sn.to_string_lossy().to_lowercase() == dn.to_string_lossy().to_lowercase()
}

/// Build a copy or move plan from gathered facts. `targets` pairs each
/// computed destination with its current metadata (if it exists).
pub fn build_transfer_plan(
    id: OpId,
    kind: OpKind,
    sources: Vec<SourceFact>,
    targets: Vec<(PathBuf, Option<EntryMetadata>)>,
    dest_dev: Option<u64>,
) -> OpPlan {
    assert!(matches!(kind, OpKind::Copy | OpKind::Move | OpKind::Rename));
    let mut plan = OpPlan::new(id, kind);
    let mut seen: HashMap<PathBuf, usize> = HashMap::new();
    for (src, (dst, dst_meta)) in sources.into_iter().zip(targets) {
        let mut item = PlanItem::new(src.path.clone(), Some(dst.clone()));
        item.key = src.key.clone();
        item.dst_exists = dst_meta.is_some();
        match &src.meta {
            Err(e) => item.conflict = Some(Conflict::Inaccessible(e.clone())),
            Ok(m) => {
                item.is_dir = m.kind == crate::FileKind::Dir;
                item.pre = Some(PreMeta::from_meta(m));
                item.case_only = case_only(&src.path, &dst);
                if kind != OpKind::Copy {
                    if let (Some(sd), Some(dd)) = (m.dev, dest_dev) {
                        item.cross_device = sd != dd;
                    }
                }
                if dst == src.path {
                    item.conflict = Some(Conflict::SameFile);
                } else if item.is_dir && dst.starts_with(&src.path) {
                    item.conflict = Some(Conflict::SelfRecursive);
                } else if src.path.starts_with(&dst) {
                    item.conflict = Some(Conflict::DestinationContainsSource);
                } else if let Some(dm) = &dst_meta {
                    let same = same_file(m, dm);
                    if same && item.case_only && kind != OpKind::Copy {
                        // Case-only rename on a case-insensitive filesystem:
                        // the "existing" destination is the source itself.
                        item.dst_exists = false;
                    } else if same {
                        item.conflict = Some(Conflict::SameFile);
                    } else {
                        item.conflict = Some(Conflict::DestinationExists);
                    }
                }
            }
        }
        if let Some(&prev) = seen.get(&dst) {
            item.conflict = Some(Conflict::DuplicateDestination);
            if plan.items[prev].conflict.is_none() {
                plan.items[prev].conflict = Some(Conflict::DuplicateDestination);
            }
        } else {
            seen.insert(dst, plan.items.len());
        }
        plan.items.push(item);
    }
    let cross = plan.items.iter().filter(|i| i.cross_device).count();
    if cross > 0 {
        plan.warnings.push(format!(
            "{cross} item(s) cross devices: moved by copy-then-delete (non-atomic); a source is deleted only after its copy succeeds"
        ));
    }
    let case = plan.items.iter().filter(|i| i.case_only).count();
    if case > 0 {
        plan.warnings.push(format!(
            "{case} case-only rename(s): performed via a temporary name for case-insensitive filesystems"
        ));
    }
    plan
}

/// Build a deletion plan (trash or permanent).
pub fn build_delete_plan(
    id: OpId,
    kind: OpKind,
    sources: Vec<SourceFact>,
    trash_supported: bool,
    backend: &str,
) -> OpPlan {
    assert!(matches!(kind, OpKind::Trash | OpKind::DeletePermanent));
    let mut plan = OpPlan::new(id, kind);
    plan.backend = backend.to_string();
    for src in sources {
        let mut item = PlanItem::new(src.path, None);
        item.key = src.key;
        match &src.meta {
            Err(e) => item.conflict = Some(Conflict::Inaccessible(e.clone())),
            Ok(m) => {
                item.is_dir = m.kind == crate::FileKind::Dir;
                item.pre = Some(PreMeta::from_meta(m));
            }
        }
        plan.items.push(item);
    }
    let dirs = plan.items.iter().filter(|i| i.is_dir).count();
    if kind == OpKind::Trash && !trash_supported {
        plan.errors.push(
            "trash is not supported on this platform/filesystem; use permanent deletion (D) explicitly"
                .into(),
        );
    }
    if kind == OpKind::DeletePermanent {
        plan.warnings
            .push("PERMANENT deletion: files will be unlinked and cannot be restored".into());
        if dirs > 0 {
            plan.warnings.push(format!(
                "{dirs} directory(ies) will be removed RECURSIVELY (symlinks are not followed)"
            ));
        }
    } else if dirs > 0 {
        plan.warnings
            .push(format!("{dirs} directory(ies) will be moved to trash with their contents"));
    }
    plan
}

/// Build a chmod plan.
pub fn build_chmod_plan(id: OpId, sources: Vec<SourceFact>, mode: u32) -> OpPlan {
    let mut plan = OpPlan::new(id, OpKind::Chmod);
    plan.mode = Some(mode);
    for src in sources {
        let mut item = PlanItem::new(src.path, None);
        item.key = src.key;
        match &src.meta {
            Err(e) => item.conflict = Some(Conflict::Inaccessible(e.clone())),
            Ok(m) => {
                item.is_dir = m.kind == crate::FileKind::Dir;
                item.pre = Some(PreMeta::from_meta(m));
                if m.mode.is_none() {
                    item.conflict = Some(Conflict::Inaccessible(
                        "filesystem does not expose Unix permissions".into(),
                    ));
                }
            }
        }
        plan.items.push(item);
    }
    plan
}

/// A requested batch rename (from Wdired or `% r`).
#[derive(Debug, Clone)]
pub struct RenameRequest {
    pub src: PathBuf,
    pub key: Option<PathBuf>,
    pub new_name: String,
    pub is_dir: bool,
}

/// Validate a new file name.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("empty name".into());
    }
    if name == "." || name == ".." {
        return Err(format!("`{name}` is reserved"));
    }
    if name.contains('/') || (cfg!(windows) && name.contains('\\')) {
        return Err("name contains a path separator".into());
    }
    if name.contains('\0') {
        return Err("name contains NUL".into());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("name contains control characters".into());
    }
    Ok(())
}

/// Build a batch rename plan. `existing` lists the current entry names of
/// every directory involved. Validation problems become conflicts plus
/// plan errors, so the plan can be previewed but never executed.
pub fn build_rename_plan(
    id: OpId,
    requests: Vec<RenameRequest>,
    existing: &HashMap<PathBuf, HashSet<OsString>>,
) -> OpPlan {
    let mut plan = OpPlan::new(id, OpKind::Rename);
    let requests: Vec<RenameRequest> = requests
        .into_iter()
        .filter(|r| r.src.file_name() != Some(OsStr::new(&r.new_name)))
        .collect();
    let sources: HashSet<PathBuf> = requests.iter().map(|r| r.src.clone()).collect();
    let mut targets: HashMap<PathBuf, usize> = HashMap::new();
    for r in &requests {
        let parent = r.src.parent().unwrap_or(Path::new("")).to_path_buf();
        let dst = parent.join(&r.new_name);
        let mut item = PlanItem::new(r.src.clone(), Some(dst.clone()));
        item.key = r.key.clone();
        item.is_dir = r.is_dir;
        item.case_only = case_only(&r.src, &dst);
        if let Err(e) = validate_name(&r.new_name) {
            item.conflict = Some(Conflict::InvalidName(format!("{:?}: {e}", r.new_name)));
        } else if let Some(&prev) = targets.get(&dst) {
            let msg = format!(
                "{} and {} both renamed to {:?}",
                display_file_name(&plan.items[prev].src),
                display_file_name(&r.src),
                r.new_name
            );
            plan.items[prev].conflict = Some(Conflict::Collision(msg.clone()));
            item.conflict = Some(Conflict::Collision(msg));
        } else {
            let occupied = existing
                .get(&parent)
                .map(|names| names.contains(OsStr::new(&r.new_name)))
                .unwrap_or(false);
            // A distinct entry with the exact new name exists (also for
            // case-only renames: readdir listed both spellings).
            if occupied && !sources.contains(&dst) {
                item.conflict = Some(Conflict::Collision(format!(
                    "{:?} already exists and is not being renamed",
                    r.new_name
                )));
            }
            targets.insert(dst, plan.items.len());
        }
        plan.items.push(item);
    }
    for item in &plan.items {
        if let Some(c) = &item.conflict {
            plan.errors
                .push(format!("{}: {}", display_file_name(&item.src), c.describe()));
        }
    }
    if plan.items.is_empty() {
        plan.errors.push("no names were changed".into());
    }
    if plan.errors.is_empty() {
        plan.steps = schedule_renames(&plan.items, existing);
        if plan.steps.len() > plan.items.len() {
            plan.warnings
                .push("rename cycle(s) broken with temporary names in the same directory".into());
        }
    }
    plan
}

fn display_file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// Order renames so no step overwrites a pending source; break cycles
/// (e.g. `a → b`, `b → a`) with a temporary name in the same directory.
/// Case-only renames always go through a temporary name.
pub fn schedule_renames(
    items: &[PlanItem],
    existing: &HashMap<PathBuf, HashSet<OsString>>,
) -> Vec<RenameStep> {
    // (current source, final destination)
    let mut pending: Vec<(PathBuf, PathBuf)> = items
        .iter()
        .filter(|i| !i.skipped())
        .filter_map(|i| i.effective_dst().map(|d| (i.src.clone(), d.to_path_buf())))
        .collect();
    let mut used: HashSet<PathBuf> = pending.iter().map(|(_, d)| d.clone()).collect();
    let mut steps = Vec::new();
    let mut counter = 0usize;
    let mut temp_for = |src: &Path, used: &mut HashSet<PathBuf>| -> PathBuf {
        let parent = src.parent().unwrap_or(Path::new(""));
        let names = existing.get(parent);
        loop {
            counter += 1;
            let name = format!(
                ".dired-tmp-{counter}-{}",
                src.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
            );
            let cand = parent.join(&name);
            let taken = names.map(|n| n.contains(OsStr::new(&name))).unwrap_or(false);
            if !taken && !used.contains(&cand) {
                used.insert(cand.clone());
                return cand;
            }
        }
    };
    // Case-only renames: move to a temp name first.
    for (src, dst) in pending.iter_mut() {
        if case_only(src, dst) {
            let tmp = temp_for(src, &mut used);
            steps.push(RenameStep {
                from: src.clone(),
                to: tmp.clone(),
            });
            *src = tmp;
        }
    }
    while !pending.is_empty() {
        let blocked: HashSet<PathBuf> = pending.iter().map(|(s, _)| s.clone()).collect();
        if let Some(i) = pending.iter().position(|(_, d)| !blocked.contains(d)) {
            let (s, d) = pending.remove(i);
            steps.push(RenameStep { from: s, to: d });
        } else {
            // Everything left is in cycles: park one source under a temp name.
            let (s, _) = &pending[0];
            let tmp = temp_for(s, &mut used);
            steps.push(RenameStep {
                from: s.clone(),
                to: tmp.clone(),
            });
            pending[0].0 = tmp;
        }
    }
    steps
}

/// Result of executing one plan item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemOutcome {
    Done,
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemResult {
    pub index: usize,
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    pub key: Option<PathBuf>,
    pub outcome: ItemOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpResult {
    pub op_id: OpId,
    pub kind: OpKind,
    pub results: Vec<ItemResult>,
    pub cancelled: bool,
}

impl OpResult {
    pub fn count(&self, pred: impl Fn(&ItemOutcome) -> bool) -> usize {
        self.results.iter().filter(|r| pred(&r.outcome)).count()
    }
    pub fn succeeded(&self) -> usize {
        self.count(|o| *o == ItemOutcome::Done)
    }
    pub fn failed(&self) -> usize {
        self.count(|o| matches!(o, ItemOutcome::Failed(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileKind;

    fn fact(p: &str, dev: u64, ino: u64, kind: FileKind) -> SourceFact {
        let mut m = EntryMetadata::simple(kind, 1);
        m.dev = Some(dev);
        m.ino = Some(ino);
        SourceFact {
            path: PathBuf::from(p),
            key: None,
            meta: Ok(m),
        }
    }

    #[test]
    fn detects_existing_and_self_recursive_and_cross_device() {
        let plan = build_transfer_plan(
            1,
            OpKind::Move,
            vec![
                fact("/a/x", 1, 1, FileKind::File),
                fact("/a/d", 1, 2, FileKind::Dir),
            ],
            vec![
                (
                    PathBuf::from("/b/x"),
                    Some(EntryMetadata::simple(FileKind::File, 3)),
                ),
                (PathBuf::from("/a/d/sub/d"), None),
            ],
            Some(2),
        );
        assert_eq!(plan.items[0].conflict, Some(Conflict::DestinationExists));
        assert_eq!(plan.items[1].conflict, Some(Conflict::SelfRecursive));
        assert!(plan.items[0].cross_device);
        assert!(plan.can_execute().is_err());
    }

    #[test]
    fn duplicate_destinations() {
        let plan = build_transfer_plan(
            1,
            OpKind::Copy,
            vec![
                fact("/a/x", 1, 1, FileKind::File),
                fact("/b/x", 1, 2, FileKind::File),
            ],
            vec![(PathBuf::from("/c/x"), None), (PathBuf::from("/c/x"), None)],
            Some(1),
        );
        assert!(plan
            .items
            .iter()
            .all(|i| i.conflict == Some(Conflict::DuplicateDestination)));
    }

    #[test]
    fn resolutions_validated() {
        let mut plan = build_transfer_plan(
            1,
            OpKind::Copy,
            vec![fact("/a/x", 1, 1, FileKind::File)],
            vec![(
                PathBuf::from("/b/x"),
                Some(EntryMetadata::simple(FileKind::File, 3)),
            )],
            Some(1),
        );
        assert!(plan.can_execute().is_err());
        assert_eq!(plan.resolve_all(ResolutionKind::Overwrite), 1);
        assert!(plan.can_execute().is_ok());
        assert_eq!(plan.overwrite_count(), 1);
    }

    fn names(dir: &str, n: &[&str]) -> HashMap<PathBuf, HashSet<OsString>> {
        let mut m = HashMap::new();
        m.insert(
            PathBuf::from(dir),
            n.iter().map(OsString::from).collect::<HashSet<_>>(),
        );
        m
    }

    fn req(src: &str, new: &str) -> RenameRequest {
        RenameRequest {
            src: PathBuf::from(src),
            key: None,
            new_name: new.into(),
            is_dir: false,
        }
    }

    #[test]
    fn swap_cycle_uses_temp() {
        let ex = names("/d", &["a", "b"]);
        let plan = build_rename_plan(1, vec![req("/d/a", "b"), req("/d/b", "a")], &ex);
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        assert_eq!(plan.steps.len(), 3);
        // Simulate.
        let mut fs: HashMap<PathBuf, &str> = HashMap::new();
        fs.insert("/d/a".into(), "A");
        fs.insert("/d/b".into(), "B");
        for s in &plan.steps {
            assert!(!fs.contains_key(&s.to), "step would overwrite {:?}", s.to);
            let v = fs.remove(&s.from).unwrap();
            fs.insert(s.to.clone(), v);
        }
        assert_eq!(fs[&PathBuf::from("/d/a")], "B");
        assert_eq!(fs[&PathBuf::from("/d/b")], "A");
    }

    #[test]
    fn collisions_block() {
        let ex = names("/d", &["a", "b", "c"]);
        let plan = build_rename_plan(1, vec![req("/d/a", "z"), req("/d/b", "z")], &ex);
        assert!(!plan.errors.is_empty());
        assert!(plan.can_execute().is_err());
        let plan = build_rename_plan(1, vec![req("/d/a", "c")], &ex);
        assert!(matches!(plan.items[0].conflict, Some(Conflict::Collision(_))));
        let plan = build_rename_plan(1, vec![req("/d/a", "x/y")], &ex);
        assert!(matches!(plan.items[0].conflict, Some(Conflict::InvalidName(_))));
    }

    #[test]
    fn chain_ordering() {
        let ex = names("/d", &["a", "b"]);
        // a -> b, b -> c : b must move first.
        let plan = build_rename_plan(1, vec![req("/d/a", "b"), req("/d/b", "c")], &ex);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].from, PathBuf::from("/d/b"));
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::FileKind;

    #[test]
    fn destination_containing_source_is_skip_only() {
        let mut m = EntryMetadata::simple(FileKind::Dir, 0);
        m.dev = Some(1);
        m.ino = Some(9);
        let mut d = EntryMetadata::simple(FileKind::Dir, 0);
        d.dev = Some(1);
        d.ino = Some(5);
        let mut plan = build_transfer_plan(
            1,
            OpKind::Move,
            vec![SourceFact { path: "/x/d/d".into(), key: None, meta: Ok(m) }],
            vec![("/x/d".into(), Some(d))],
            Some(1),
        );
        assert_eq!(plan.items[0].conflict, Some(Conflict::DestinationContainsSource));
        assert!(plan.set_resolution(0, Resolution::Overwrite).is_err());
        assert_eq!(plan.resolve_all(ResolutionKind::Overwrite), 0);
    }

    #[test]
    fn rename_plans_refuse_skip_and_case_collisions() {
        let mut ex = HashMap::new();
        ex.insert(PathBuf::from("/d"), ["a", "A"].iter().map(OsString::from).collect::<HashSet<_>>());
        let req = |s: &str, n: &str| RenameRequest { src: PathBuf::from(s), key: None, new_name: n.into(), is_dir: false };
        let plan = build_rename_plan(1, vec![req("/d/a", "A")], &ex);
        assert!(!plan.errors.is_empty(), "distinct `A` exists");
        let mut ex2 = HashMap::new();
        ex2.insert(PathBuf::from("/d"), ["a"].iter().map(OsString::from).collect::<HashSet<_>>());
        let mut plan = build_rename_plan(1, vec![req("/d/a", "A")], &ex2);
        assert!(plan.errors.is_empty());
        assert!(plan.set_resolution(0, Resolution::Skip).is_err());
    }
}
