// dired-7qs
//! Operation journal records and best-effort undo derivation.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use crate::plan::{schedule_renames, ItemOutcome, OpKind, OpPlan, OpResult, PlanItem, PreMeta};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalItem {
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    pub pre: Option<PreMeta>,
    pub dst_existed: bool,
    pub outcome: ItemOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub op_id: u64,
    pub time_unix: u64,
    pub kind: OpKind,
    pub backend: String,
    pub mode: Option<u32>,
    pub items: Vec<JournalItem>,
    pub undone: bool,
}

impl JournalEntry {
    pub fn from_result(plan: &OpPlan, result: &OpResult, time_unix: u64) -> Self {
        let items = result
            .results
            .iter()
            .map(|r| {
                let item = plan.items.get(r.index);
                JournalItem {
                    src: r.src.clone(),
                    dst: r.dst.clone(),
                    pre: item.and_then(|i| i.pre.clone()),
                    // With a RenameTo resolution the written target is new.
                    dst_existed: item
                        .map(|i| i.dst_exists && !matches!(i.resolution, crate::plan::Resolution::RenameTo(_)))
                        .unwrap_or(false),
                    outcome: r.outcome.clone(),
                }
            })
            .collect();
        JournalEntry {
            op_id: plan.id,
            time_unix,
            kind: plan.kind,
            backend: plan.backend.clone(),
            mode: plan.mode,
            items,
            undone: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoStep {
    Rename { from: PathBuf, to: PathBuf },
    Trash(PathBuf),
    RemoveEmptyDir(PathBuf),
    Chmod { path: PathBuf, mode: u32 },
}

/// Derive best-effort undo steps. Errors explain why an entry cannot be undone.
pub fn undo_steps(entry: &JournalEntry) -> Result<Vec<UndoStep>, String> {
    if entry.undone {
        return Err("operation was already undone".into());
    }
    let done = entry
        .items
        .iter()
        .filter(|i| i.outcome == ItemOutcome::Done);
    let steps: Vec<UndoStep> = match entry.kind {
        OpKind::Move | OpKind::Rename => {
            // Reverse every rename, ordered safely (swaps need a temp name).
            let items: Vec<PlanItem> = done
                .filter_map(|i| i.dst.as_ref().map(|d| PlanItem::new(d.clone(), Some(i.src.clone()))))
                .collect();
            schedule_renames(&items, &HashMap::new())
                .into_iter()
                .map(|s| UndoStep::Rename { from: s.from, to: s.to })
                .collect()
        }
        OpKind::Copy => {
            if entry.items.iter().any(|i| i.dst_existed && i.outcome == ItemOutcome::Done) {
                return Err("copy overwrote existing files; their previous contents cannot be restored".into());
            }
            done.filter_map(|i| i.dst.clone().map(UndoStep::Trash)).collect()
        }
        OpKind::Mkdir => done.map(|i| UndoStep::RemoveEmptyDir(i.src.clone())).collect(),
        OpKind::Chmod => done
            .filter_map(|i| {
                i.pre.as_ref().and_then(|p| p.mode).map(|m| UndoStep::Chmod {
                    path: i.src.clone(),
                    mode: m & 0o7777,
                })
            })
            .collect(),
        OpKind::Trash => {
            return Err("restore trashed files from the system trash".into());
        }
        OpKind::DeletePermanent => return Err("permanent deletion cannot be undone".into()),
    };
    if steps.is_empty() {
        return Err("no completed items to undo".into());
    }
    Ok(steps)
}
