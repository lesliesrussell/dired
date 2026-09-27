// dired-7qs
//! Background tasks. Tasks never touch UI state: they send events tagged
//! with buffer id and generation so stale results are discarded.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dired_domain::journal::UndoStep;
use dired_domain::plan::{build_delete_plan, build_transfer_plan, resolve_targets, DestFact, OpKind, OpPlan, OpResult, SourceFact};
use dired_domain::shell::Invocation;
use dired_domain::EntryMetadata;
use dired_fs::{FileSystem, RawDirEntry, RenameOptions, RemoveOptions, TrashBackend};
use dired_process::{ProcessEvent, ProcessRunner, ProcessSpec};
use tokio::sync::mpsc::UnboundedSender;

use crate::buffers::BufferId;

const META_BATCH: usize = 256;

#[derive(Debug)]
pub enum AppEvent {
    ScanBatch { buf: BufferId, dir: PathBuf, gen: u64, entries: Vec<RawDirEntry> },
    ScanDone { buf: BufferId, dir: PathBuf, gen: u64, error: Option<String> },
    MetaBatch { buf: BufferId, dir: PathBuf, gen: u64, metas: Vec<(PathBuf, Result<EntryMetadata, String>)> },
    MetaDone { buf: BufferId, dir: PathBuf, gen: u64 },
    PlanReady { origin: BufferId, plan: Result<OpPlan, String>, delete_facts: Option<Vec<SourceFact>>, trash_supported: bool },
    OpProgress { buf: BufferId, done: usize, total: usize },
    OpDone { buf: BufferId, result: OpResult },
    ProcLine { buf: BufferId, line: String, stderr: bool },
    ProcExit { buf: BufferId, code: Option<i32>, error: Option<String> },
    Completions { gen: u64, items: Vec<String> },
    UndoDone { op_id: u64, errors: Vec<String>, touched: Vec<PathBuf> },
    PathResolved { path: PathBuf, is_dir: Result<bool, String>, visit: bool },
    TaskFinished,
}

pub type Tx = UnboundedSender<AppEvent>;

pub async fn scan(fs: Arc<dyn FileSystem>, tx: Tx, buf: BufferId, dir_key: PathBuf, abs: PathBuf, gen: u64, cancel: Arc<AtomicBool>) {
    let mut rx = match fs.read_dir(&abs).await {
        Ok(rx) => rx,
        Err(e) => {
            let _ = tx.send(AppEvent::ScanDone { buf, dir: dir_key, gen, error: Some(e.to_string()) });
            return;
        }
    };
    let mut keys: Vec<PathBuf> = Vec::new();
    let mut error = None;
    while let Some(batch) = rx.recv().await {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        match batch {
            Ok(entries) => {
                keys.extend(entries.iter().map(|e| dir_key.join(&e.name)));
                let _ = tx.send(AppEvent::ScanBatch { buf, dir: dir_key.clone(), gen, entries });
            }
            Err(e) => error = Some(e.to_string()),
        }
    }
    let _ = tx.send(AppEvent::ScanDone { buf, dir: dir_key.clone(), gen, error });
    // Enrich metadata in bounded batches after names are on screen.
    for chunk in keys.chunks(META_BATCH) {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut metas = Vec::with_capacity(chunk.len());
        for key in chunk {
            let p = abs.join(key.file_name().unwrap_or_default());
            metas.push((key.clone(), fs.metadata(&p, false).await.map_err(|e| e.to_string())));
        }
        let _ = tx.send(AppEvent::MetaBatch { buf, dir: dir_key.clone(), gen, metas });
    }
    let _ = tx.send(AppEvent::MetaDone { buf, dir: dir_key, gen });
}

pub async fn source_facts(fs: &dyn FileSystem, sources: &[(PathBuf, Option<PathBuf>)]) -> Vec<SourceFact> {
    let mut v = Vec::with_capacity(sources.len());
    for (path, key) in sources {
        v.push(SourceFact {
            path: path.clone(),
            key: key.clone(),
            meta: fs.metadata(path, false).await.map_err(|e| e.to_string()),
        });
    }
    v
}

pub async fn plan_transfer(fs: Arc<dyn FileSystem>, tx: Tx, origin: BufferId, id: u64, kind: OpKind, sources: Vec<(PathBuf, Option<PathBuf>)>, dest: PathBuf) {
    let facts = source_facts(fs.as_ref(), &sources).await;
    let dest_meta = fs.metadata(&dest, true).await.ok();
    let dest_is_dir = dest_meta.as_ref().map(|m| m.kind == dired_domain::FileKind::Dir).unwrap_or(false);
    let dev_dir = if dest_is_dir { dest.clone() } else { dest.parent().unwrap_or(Path::new("/")).to_path_buf() };
    let dev = fs.metadata(&dev_dir, true).await.ok().and_then(|m| m.dev);
    let fact = DestFact { path: dest.clone(), meta: dest_meta, dev };
    let paths: Vec<PathBuf> = sources.iter().map(|(p, _)| p.clone()).collect();
    let plan = match resolve_targets(&paths, &fact) {
        Err(e) => Err(e),
        Ok(targets) => {
            if !dest_is_dir && !fs.exists(&dev_dir).await {
                Err(format!("destination directory {} does not exist", dev_dir.display()))
            } else {
                let mut t = Vec::with_capacity(targets.len());
                for target in targets {
                    let m = fs.metadata(&target, false).await.ok();
                    t.push((target, m));
                }
                let mut plan = build_transfer_plan(id, kind, facts, t, dev);
                flag_aliased_recursion(fs.as_ref(), &mut plan).await;
                Ok(plan)
            }
        }
    };
    let _ = tx.send(AppEvent::PlanReady { origin, plan, delete_facts: None, trash_supported: true });
}

/// Detect self-recursive copies/moves hidden behind symlinks or other
/// aliases by comparing canonical paths.
async fn flag_aliased_recursion(fs: &dyn FileSystem, plan: &mut OpPlan) {
    use dired_domain::plan::Conflict;
    for item in &mut plan.items {
        // Recursion outranks a plain "exists" conflict.
        let weaker = matches!(item.conflict, None | Some(Conflict::DestinationExists));
        if !weaker || !item.is_dir {
            continue;
        }
        let Some(dst) = item.dst.clone() else { continue };
        let (Some(parent), Some(name)) = (dst.parent(), dst.file_name()) else { continue };
        let (Ok(src), Ok(parent)) = (fs.canonicalize(&item.src).await, fs.canonicalize(parent).await) else { continue };
        let dst = parent.join(name);
        if dst.starts_with(&src) {
            item.conflict = Some(Conflict::SelfRecursive);
        } else if src.starts_with(&dst) {
            item.conflict = Some(Conflict::DestinationContainsSource);
        }
    }
}

pub async fn plan_delete(fs: Arc<dyn FileSystem>, trash: Arc<dyn TrashBackend>, tx: Tx, origin: BufferId, id: u64, permanent: bool, sources: Vec<(PathBuf, Option<PathBuf>)>) {
    let facts = source_facts(fs.as_ref(), &sources).await;
    let supported = trash.supported();
    let kind = if permanent { OpKind::DeletePermanent } else { OpKind::Trash };
    let backend = if permanent { "permanent unlink (irreversible)".to_string() } else { trash.name() };
    let plan = build_delete_plan(id, kind, facts.clone(), supported, &backend);
    let _ = tx.send(AppEvent::PlanReady { origin, plan: Ok(plan), delete_facts: Some(facts), trash_supported: supported });
}

pub async fn plan_chmod(fs: Arc<dyn FileSystem>, tx: Tx, origin: BufferId, id: u64, mode: u32, sources: Vec<(PathBuf, Option<PathBuf>)>) {
    let facts = source_facts(fs.as_ref(), &sources).await;
    let plan = dired_domain::plan::build_chmod_plan(id, facts, mode);
    let _ = tx.send(AppEvent::PlanReady { origin, plan: Ok(plan), delete_facts: None, trash_supported: true });
}

pub async fn execute(fs: Arc<dyn FileSystem>, trash: Arc<dyn TrashBackend>, tx: Tx, buf: BufferId, plan: OpPlan, cancel: Arc<AtomicBool>) {
    let tx2 = tx.clone();
    let result = dired_fs::execute_plan(fs, trash, plan, cancel, move |done, total| {
        let _ = tx2.send(AppEvent::OpProgress { buf, done, total });
    })
    .await;
    let _ = tx.send(AppEvent::OpDone { buf, result });
}

/// Run invocations sequentially, streaming output into buffer `buf`.
pub async fn run_processes(runner: Arc<dyn ProcessRunner>, tx: Tx, buf: BufferId, invocations: Vec<Invocation>, kill: dired_process::KillSwitch) {
    let mut last = None;
    for inv in invocations {
        let spec = ProcessSpec::new(inv.program.clone(), inv.args.clone(), inv.cwd.clone());
        let mut h = match runner.spawn(spec) {
            Ok(h) => h,
            Err(e) => {
                let _ = tx.send(AppEvent::ProcExit { buf, code: None, error: Some(e.to_string()) });
                return;
            }
        };
        let k = h.kill.clone();
        loop {
            tokio::select! {
                ev = h.events.recv() => match ev {
                    Some(ProcessEvent::Stdout(line)) => { let _ = tx.send(AppEvent::ProcLine { buf, line, stderr: false }); }
                    Some(ProcessEvent::Stderr(line)) => { let _ = tx.send(AppEvent::ProcLine { buf, line, stderr: true }); }
                    Some(ProcessEvent::Exited(code)) => { last = code; break; }
                    None => break,
                },
                _ = kill.wait() => { k.kill(); }
            }
        }
    }
    let _ = tx.send(AppEvent::ProcExit { buf, code: last, error: None });
}

pub async fn complete_path(fs: Arc<dyn FileSystem>, tx: Tx, gen: u64, input: String, base: PathBuf) {
    let expanded = crate::app::expand_path(&input, &base);
    let (dir, prefix) = if input.ends_with('/') || input.is_empty() {
        (expanded.clone(), String::new())
    } else {
        (
            expanded.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/")),
            expanded.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        )
    };
    let mut items = Vec::new();
    if let Ok(mut rx) = fs.read_dir(&dir).await {
        while let Some(batch) = rx.recv().await {
            for e in batch.into_iter().flatten() {
                let name = e.name.to_string_lossy().into_owned();
                if name.starts_with(&prefix) {
                    let is_dir = match e.kind {
                        dired_domain::FileKind::Dir => true,
                        dired_domain::FileKind::Symlink => fs.metadata(&dir.join(&e.name), true).await.map(|m| m.kind == dired_domain::FileKind::Dir).unwrap_or(false),
                        _ => false,
                    };
                    items.push(if is_dir { format!("{name}/") } else { name });
                }
            }
            if items.len() > 5000 {
                break;
            }
        }
    }
    items.sort();
    // Candidates are full inputs: the typed directory part plus each name.
    let head = match input.rfind('/') {
        Some(i) => input[..=i].to_string(),
        None => String::new(),
    };
    let items = items.into_iter().map(|n| format!("{head}{n}")).collect();
    let _ = tx.send(AppEvent::Completions { gen, items });
}

pub async fn undo(fs: Arc<dyn FileSystem>, trash: Arc<dyn TrashBackend>, tx: Tx, op_id: u64, steps: Vec<UndoStep>) {
    let mut errors = Vec::new();
    let mut touched = Vec::new();
    for s in steps {
        let r = match &s {
            UndoStep::Rename { from, to } => {
                touched.push(from.clone());
                touched.push(to.clone());
                fs.rename(from, to, RenameOptions { overwrite: false }).await.map_err(|e| e.to_string())
            }
            UndoStep::Trash(p) => {
                touched.push(p.clone());
                let t = trash.clone();
                let p2 = p.clone();
                match tokio::task::spawn_blocking(move || t.trash(&[p2])).await {
                    Ok(Ok(rep)) => rep.results.into_iter().next().map(|(_, r)| r).unwrap_or(Ok(())),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
            UndoStep::RemoveEmptyDir(p) => {
                touched.push(p.clone());
                fs.remove(p, RemoveOptions { recursive: false }).await.map_err(|e| e.to_string())
            }
            UndoStep::Chmod { path, mode } => {
                touched.push(path.clone());
                fs.set_permissions(path, *mode).await.map_err(|e| e.to_string())
            }
        };
        if let Err(e) = r {
            errors.push(e);
        }
    }
    let _ = tx.send(AppEvent::UndoDone { op_id, errors, touched });
}
