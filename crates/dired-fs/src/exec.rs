// dired-7qs
//! Plan execution. Each item is attempted independently; failures are
//! recorded and never abort unrelated items.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dired_domain::{ItemOutcome, ItemResult, OpKind, OpPlan, OpResult, PlanItem, Resolution};

use crate::{
    CopyOptions, FileSystem, FsError, FsErrorKind, RemoveOptions, RenameOptions, TrashBackend,
};

fn result(index: usize, item: &PlanItem, outcome: ItemOutcome) -> ItemResult {
    ItemResult {
        index,
        src: item.src.clone(),
        dst: item.effective_dst().map(Path::to_path_buf),
        key: item.key.clone(),
        outcome,
    }
}

fn temp_sibling(p: &Path) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    p.with_file_name(format!(".dired-tmp-case-{}-{name}", std::process::id()))
}

fn backup_sibling(p: &Path) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    p.with_file_name(format!(".dired-backup-{}-{name}", std::process::id()))
}

/// Run `op` against `dst`. With an overwrite resolution the existing
/// destination is first set aside, removed only after `op` succeeds, and
/// restored if it fails, so a failed overwrite never loses the old data.
async fn with_overwrite<'a, F, Fut>(fs: &'a dyn FileSystem, item: &PlanItem, dst: &'a Path, op: F) -> Result<(), FsError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), FsError>> + 'a,
{
    let exists = fs.exists(dst).await;
    if !exists {
        return op().await;
    }
    if item.resolution != Resolution::Overwrite {
        return Err(FsError::new("write", dst, FsErrorKind::AlreadyExists, "destination appeared after planning"));
    }
    if item.src.starts_with(dst) {
        return Err(FsError::new("overwrite", dst, FsErrorKind::Other, "refusing to replace a directory that contains the source"));
    }
    let backup = backup_sibling(dst);
    fs.rename(dst, &backup, RenameOptions { overwrite: false }).await?;
    match op().await {
        Ok(()) => {
            let _ = fs.remove(&backup, RemoveOptions { recursive: true }).await;
            Ok(())
        }
        Err(e) => {
            if fs.exists(dst).await {
                let _ = fs.remove(dst, RemoveOptions { recursive: true }).await;
            }
            let _ = fs.rename(&backup, dst, RenameOptions { overwrite: false }).await;
            Err(e)
        }
    }
}

async fn copy_item(fs: &dyn FileSystem, item: &PlanItem, dst: &Path) -> Result<(), FsError> {
    with_overwrite(fs, item, dst, || {
        fs.copy(&item.src, dst, CopyOptions { recursive: true, overwrite: false, follow_symlinks: false })
    })
    .await
}

/// Copy then delete; the source is removed only after the copy succeeded.
async fn cross_device_move(fs: &dyn FileSystem, item: &PlanItem, dst: &Path) -> Result<(), FsError> {
    copy_item(fs, item, dst).await?;
    fs.remove(&item.src, RemoveOptions { recursive: true }).await
}

async fn move_item(fs: &dyn FileSystem, item: &PlanItem, dst: &Path) -> Result<(), FsError> {
    if item.cross_device {
        return cross_device_move(fs, item, dst).await;
    }
    if item.case_only {
        let tmp = temp_sibling(&item.src);
        fs.rename(&item.src, &tmp, RenameOptions { overwrite: false }).await?;
        return match fs.rename(&tmp, dst, RenameOptions { overwrite: false }).await {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs.rename(&tmp, &item.src, RenameOptions::default()).await;
                Err(e)
            }
        };
    }
    let r = with_overwrite(fs, item, dst, || fs.rename(&item.src, dst, RenameOptions { overwrite: false })).await;
    match r {
        Err(e) if e.kind == FsErrorKind::CrossDevice => cross_device_move(fs, item, dst).await,
        r => r,
    }
}

pub async fn execute_plan(
    fs: Arc<dyn FileSystem>,
    trash: Arc<dyn TrashBackend>,
    plan: OpPlan,
    cancel: Arc<AtomicBool>,
    progress: impl Fn(usize, usize) + Send,
) -> OpResult {
    let total = plan.items.len();
    let mut results: Vec<ItemResult> = Vec::with_capacity(total);
    let mut cancelled = false;
    let fs = fs.as_ref();

    if let Err(e) = plan.can_execute() {
        let results = plan
            .items
            .iter()
            .enumerate()
            .map(|(i, it)| result(i, it, ItemOutcome::Failed(e.clone())))
            .collect();
        return OpResult { op_id: plan.id, kind: plan.kind, results, cancelled: false };
    }

    match plan.kind {
        OpKind::Trash => {
            let targets: Vec<(usize, &PlanItem)> =
                plan.items.iter().enumerate().filter(|(_, i)| !i.skipped()).collect();
            let paths: Vec<PathBuf> = targets.iter().map(|(_, i)| i.src.clone()).collect();
            let trash = trash.clone();
            let report = tokio::task::spawn_blocking(move || trash.trash(&paths))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            for (i, item) in plan.items.iter().enumerate() {
                let outcome = if item.skipped() {
                    ItemOutcome::Skipped
                } else {
                    match &report {
                        Err(e) => ItemOutcome::Failed(format!("trash {}: {e}", item.src.display())),
                        Ok(rep) => match rep.results.iter().find(|(p, _)| *p == item.src) {
                            Some((_, Ok(()))) => ItemOutcome::Done,
                            Some((_, Err(e))) => ItemOutcome::Failed(e.clone()),
                            None => ItemOutcome::Failed("trash backend did not report this path".into()),
                        },
                    }
                };
                results.push(result(i, item, outcome));
            }
            progress(total, total);
        }
        OpKind::Rename => {
            // Execute the schedule; an item is done when its final step ran.
            let mut done_targets = std::collections::HashSet::new();
            let mut failure: Option<String> = None;
            // Renames are quick and interdependent (temp names for cycles), so
            // the schedule is not cancellable midway.
            for (n, step) in plan.steps.iter().enumerate() {
                match fs.rename(&step.from, &step.to, RenameOptions { overwrite: false }).await {
                    Ok(()) => {
                        done_targets.insert(step.to.clone());
                    }
                    Err(e) => {
                        let parked: Vec<String> = plan.steps[..n]
                            .iter()
                            .filter(|s| s.to.file_name().map(|f| f.to_string_lossy().starts_with(".dired-tmp-")).unwrap_or(false))
                            .filter(|s| !plan.steps[..n].iter().any(|t| t.from == s.to))
                            .map(|s| format!("{} parked at {}", s.from.display(), s.to.display()))
                            .collect();
                        let mut msg = e.to_string();
                        if !parked.is_empty() {
                            msg.push_str(&format!("; recovery: {}", parked.join(", ")));
                        }
                        failure = Some(msg);
                        break;
                    }
                }
                progress(n + 1, plan.steps.len());
            }
            for (i, item) in plan.items.iter().enumerate() {
                let outcome = match item.effective_dst() {
                    _ if item.skipped() => ItemOutcome::Skipped,
                    Some(d) if done_targets.contains(d) => ItemOutcome::Done,
                    _ => ItemOutcome::Failed(
                        failure.clone().unwrap_or_else(|| "not executed (cancelled)".into()),
                    ),
                };
                results.push(result(i, item, outcome));
            }
        }
        _ => {
            for (i, item) in plan.items.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    results.push(result(i, item, ItemOutcome::Skipped));
                    continue;
                }
                if item.skipped() {
                    results.push(result(i, item, ItemOutcome::Skipped));
                    continue;
                }
                let r = match plan.kind {
                    OpKind::Copy => copy_item(fs, item, item.effective_dst().unwrap()).await,
                    OpKind::Move => move_item(fs, item, item.effective_dst().unwrap()).await,
                    OpKind::DeletePermanent => {
                        fs.remove(&item.src, RemoveOptions { recursive: item.is_dir }).await
                    }
                    OpKind::Chmod => fs.set_permissions(&item.src, plan.mode.unwrap_or(0o644)).await,
                    OpKind::Mkdir => fs.create_dir(&item.src).await,
                    OpKind::Trash | OpKind::Rename => unreachable!(),
                };
                let outcome = match r {
                    Ok(()) => ItemOutcome::Done,
                    Err(e) => ItemOutcome::Failed(e.to_string()),
                };
                results.push(result(i, item, outcome));
                progress(i + 1, total);
            }
        }
    }
    OpResult { op_id: plan.id, kind: plan.kind, results, cancelled }
}
