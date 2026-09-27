// dired-7qs
//! Deterministic test doubles: in-memory filesystem with device IDs and
//! failure injection, trash backend, process runner, and clock.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dired_domain::clock::Clock;
use dired_domain::{EntryMetadata, FileKind};
use dired_fs::{
    CopyOptions, DirectoryStream, FileSystem, FsError, FsErrorKind, RawDirEntry, RemoveOptions,
    RenameOptions, TrashBackend, TrashError, TrashReport,
};
use dired_process::{KillSwitch, ProcessError, ProcessEvent, ProcessHandle, ProcessRunner, ProcessSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeKind {
    File(Vec<u8>),
    Dir,
    Symlink(PathBuf),
}

#[derive(Debug, Clone)]
struct Node {
    kind: NodeKind,
    mtime: SystemTime,
    ino: u64,
    mode: u32,
}

/// A recorded mutating filesystem call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsOp {
    Copy { from: PathBuf, to: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    Remove(PathBuf),
    Mkdir(PathBuf),
    Chmod(PathBuf, u32),
}

#[derive(Default)]
struct State {
    nodes: BTreeMap<PathBuf, Node>,
    ops: Vec<FsOp>,
    fail: HashMap<PathBuf, String>,
    mounts: Vec<(PathBuf, u64)>,
    next_ino: u64,
}

impl State {
    fn dev_of(&self, p: &Path) -> u64 {
        self.mounts
            .iter()
            .filter(|(m, _)| p.starts_with(m))
            .max_by_key(|(m, _)| m.as_os_str().len())
            .map(|(_, d)| *d)
            .unwrap_or(1)
    }

    fn ino(&mut self) -> u64 {
        self.next_ino += 1;
        self.next_ino
    }

    fn insert(&mut self, p: &Path, kind: NodeKind) {
        let ino = self.ino();
        let mode = if kind == NodeKind::Dir { 0o40755 } else { 0o100644 };
        self.nodes.insert(p.to_path_buf(), Node { kind, mtime: UNIX_EPOCH + Duration::from_secs(1_700_000_000 + ino), ino, mode });
    }

    fn descendants(&self, p: &Path) -> Vec<PathBuf> {
        self.nodes
            .range(p.to_path_buf()..)
            .take_while(|(k, _)| k.starts_with(p))
            .map(|(k, _)| k.clone())
            .collect()
    }

    fn check_fail(&self, op: &'static str, p: &Path) -> Result<(), FsError> {
        match self.fail.get(p) {
            Some(m) => Err(FsError::new(op, p, FsErrorKind::PermissionDenied, m.clone())),
            None => Ok(()),
        }
    }

    fn meta(&self, p: &Path, follow: bool) -> Result<EntryMetadata, FsError> {
        let node = self
            .nodes
            .get(p)
            .ok_or_else(|| FsError::new("stat", p, FsErrorKind::NotFound, "No such file or directory"))?;
        let (kind, size, target) = match &node.kind {
            NodeKind::File(d) => (FileKind::File, d.len() as u64, None),
            NodeKind::Dir => (FileKind::Dir, 0, None),
            NodeKind::Symlink(t) => {
                if follow {
                    let abs = p.parent().unwrap_or(Path::new("/")).join(t);
                    return self.meta(&abs, true);
                }
                (FileKind::Symlink, t.as_os_str().len() as u64, Some(t.clone()))
            }
        };
        let target_is_dir = target
            .as_ref()
            .map(|t| {
                let abs = p.parent().unwrap_or(Path::new("/")).join(t);
                matches!(self.nodes.get(&abs).map(|n| &n.kind), Some(NodeKind::Dir))
            })
            .unwrap_or(false);
        Ok(EntryMetadata {
            kind,
            size,
            modified: Some(node.mtime),
            mode: Some(node.mode),
            readonly: false,
            dev: Some(self.dev_of(p)),
            ino: Some(node.ino),
            symlink_target: target,
            target_is_dir,
        })
    }
}

/// In-memory filesystem.
pub struct MockFs {
    state: Mutex<State>,
    batch: usize,
}

impl Default for MockFs {
    fn default() -> Self {
        Self::new()
    }
}

impl MockFs {
    pub fn new() -> Self {
        let fs = MockFs { state: Mutex::new(State::default()), batch: 256 };
        fs.mkdir_p("/");
        fs
    }

    pub fn with_batch(mut self, n: usize) -> Self {
        self.batch = n;
        self
    }

    pub fn mkdir_p(&self, p: impl AsRef<Path>) {
        let mut st = self.state.lock().unwrap();
        let mut cur = PathBuf::new();
        for c in p.as_ref().components() {
            cur.push(c);
            if !st.nodes.contains_key(&cur) {
                st.insert(&cur.clone(), NodeKind::Dir);
            }
        }
    }

    pub fn write(&self, p: impl AsRef<Path>, data: &str) {
        let p = p.as_ref();
        if let Some(parent) = p.parent() {
            self.mkdir_p(parent);
        }
        let mut st = self.state.lock().unwrap();
        st.insert(p, NodeKind::File(data.as_bytes().to_vec()));
    }

    pub fn symlink(&self, p: impl AsRef<Path>, target: impl AsRef<Path>) {
        self.state.lock().unwrap().insert(p.as_ref(), NodeKind::Symlink(target.as_ref().to_path_buf()));
    }

    /// Add `n` synthetic files named `file-000000...` to `dir`.
    pub fn add_many(&self, dir: impl AsRef<Path>, n: usize) {
        self.mkdir_p(dir.as_ref());
        let mut st = self.state.lock().unwrap();
        for i in 0..n {
            let p = dir.as_ref().join(format!("file-{i:06}"));
            st.insert(&p, NodeKind::File(Vec::new()));
        }
    }

    pub fn read(&self, p: impl AsRef<Path>) -> Option<String> {
        match &self.state.lock().unwrap().nodes.get(p.as_ref())?.kind {
            NodeKind::File(d) => Some(String::from_utf8_lossy(d).into_owned()),
            _ => None,
        }
    }

    pub fn exists_now(&self, p: impl AsRef<Path>) -> bool {
        self.state.lock().unwrap().nodes.contains_key(p.as_ref())
    }

    /// Mount point prefix → device id (default device is 1).
    pub fn mount(&self, prefix: impl AsRef<Path>, dev: u64) {
        self.state.lock().unwrap().mounts.push((prefix.as_ref().to_path_buf(), dev));
    }

    pub fn fail_on(&self, p: impl AsRef<Path>, msg: &str) {
        self.state.lock().unwrap().fail.insert(p.as_ref().to_path_buf(), msg.into());
    }

    pub fn ops(&self) -> Vec<FsOp> {
        self.state.lock().unwrap().ops.clone()
    }

    pub fn clear_ops(&self) {
        self.state.lock().unwrap().ops.clear();
    }
}

#[async_trait::async_trait]
impl FileSystem for MockFs {
    async fn read_dir(&self, path: &Path) -> Result<DirectoryStream, FsError> {
        let names: Vec<RawDirEntry> = {
            let st = self.state.lock().unwrap();
            st.check_fail("read_dir", path)?;
            match st.nodes.get(path).map(|n| &n.kind) {
                Some(NodeKind::Dir) => {}
                Some(_) => return Err(FsError::new("read_dir", path, FsErrorKind::Other, "Not a directory")),
                None => return Err(FsError::new("read_dir", path, FsErrorKind::NotFound, "No such file or directory")),
            }
            st.nodes
                .range(path.to_path_buf()..)
                .skip(1)
                .take_while(|(k, _)| k.starts_with(path))
                .filter(|(k, _)| k.parent() == Some(path))
                .map(|(k, n)| RawDirEntry {
                    name: k.file_name().unwrap().to_os_string(),
                    kind: match n.kind {
                        NodeKind::File(_) => FileKind::File,
                        NodeKind::Dir => FileKind::Dir,
                        NodeKind::Symlink(_) => FileKind::Symlink,
                    },
                })
                .collect()
        };
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let batch = self.batch;
        tokio::spawn(async move {
            for chunk in names.chunks(batch) {
                if tx.send(Ok(chunk.to_vec())).await.is_err() {
                    return;
                }
                tokio::task::yield_now().await;
            }
        });
        Ok(rx)
    }

    async fn metadata(&self, path: &Path, follow_symlinks: bool) -> Result<EntryMetadata, FsError> {
        let st = self.state.lock().unwrap();
        st.meta(path, follow_symlinks)
    }

    async fn copy(&self, from: &Path, to: &Path, options: CopyOptions) -> Result<(), FsError> {
        let mut st = self.state.lock().unwrap();
        st.check_fail("copy", from)?;
        st.check_fail("copy", to)?;
        if !st.nodes.contains_key(from) {
            return Err(FsError::new("copy", from, FsErrorKind::NotFound, "No such file or directory"));
        }
        if !options.overwrite && st.nodes.contains_key(to) {
            return Err(FsError::new("copy", to, FsErrorKind::AlreadyExists, "File exists"));
        }
        if !to.parent().map(|p| st.nodes.contains_key(p)).unwrap_or(true) {
            return Err(FsError::new("copy", to, FsErrorKind::NotFound, "parent directory missing"));
        }
        let srcs = if options.recursive { st.descendants(from) } else { vec![from.to_path_buf()] };
        for s in srcs {
            let rel = s.strip_prefix(from).unwrap();
            let d = if rel.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rel) };
            let kind = st.nodes[&s].kind.clone();
            st.insert(&d, kind);
        }
        st.ops.push(FsOp::Copy { from: from.into(), to: to.into() });
        Ok(())
    }

    async fn rename(&self, from: &Path, to: &Path, options: RenameOptions) -> Result<(), FsError> {
        let mut st = self.state.lock().unwrap();
        st.check_fail("rename", from)?;
        if !st.nodes.contains_key(from) {
            return Err(FsError::new("rename", from, FsErrorKind::NotFound, "No such file or directory"));
        }
        if st.dev_of(from) != st.dev_of(to) {
            return Err(FsError { op: "rename", path: from.into(), kind: FsErrorKind::CrossDevice, message: "Cross-device link".into(), code: Some(18) });
        }
        if st.nodes.contains_key(to) {
            if !options.overwrite {
                return Err(FsError::new("rename", to, FsErrorKind::AlreadyExists, "File exists"));
            }
            for d in st.descendants(to) {
                st.nodes.remove(&d);
            }
        }
        for s in st.descendants(from) {
            let rel = s.strip_prefix(from).unwrap().to_path_buf();
            let node = st.nodes.remove(&s).unwrap();
            let d = if rel.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rel) };
            st.nodes.insert(d, node);
        }
        st.ops.push(FsOp::Rename { from: from.into(), to: to.into() });
        Ok(())
    }

    async fn remove(&self, path: &Path, options: RemoveOptions) -> Result<(), FsError> {
        let mut st = self.state.lock().unwrap();
        st.check_fail("remove", path)?;
        let d = st.descendants(path);
        if d.is_empty() {
            return Err(FsError::new("remove", path, FsErrorKind::NotFound, "No such file or directory"));
        }
        if d.len() > 1 && !options.recursive {
            return Err(FsError::new("remove", path, FsErrorKind::NotEmpty, "Directory not empty"));
        }
        for p in d {
            st.nodes.remove(&p);
        }
        st.ops.push(FsOp::Remove(path.into()));
        Ok(())
    }

    async fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        let mut st = self.state.lock().unwrap();
        st.check_fail("mkdir", path)?;
        if st.nodes.contains_key(path) {
            return Err(FsError::new("mkdir", path, FsErrorKind::AlreadyExists, "File exists"));
        }
        st.insert(path, NodeKind::Dir);
        st.ops.push(FsOp::Mkdir(path.into()));
        Ok(())
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<(), FsError> {
        let mut st = self.state.lock().unwrap();
        st.check_fail("chmod", path)?;
        let node = st
            .nodes
            .get_mut(path)
            .ok_or_else(|| FsError::new("chmod", path, FsErrorKind::NotFound, "No such file or directory"))?;
        node.mode = (node.mode & !0o7777) | (mode & 0o7777);
        st.ops.push(FsOp::Chmod(path.into(), mode));
        Ok(())
    }
}

/// Records every path it is asked to trash; paths in `fail` fail.
/// When constructed with a [`MockFs`], trashed paths are removed from it.
#[derive(Default)]
pub struct MockTrash {
    pub received: Mutex<Vec<PathBuf>>,
    pub fail: Mutex<HashSet<PathBuf>>,
    pub unsupported: bool,
    pub fs: Option<Arc<MockFs>>,
}

impl MockTrash {
    pub fn with_fs(fs: Arc<MockFs>) -> Self {
        MockTrash { fs: Some(fs), ..Default::default() }
    }
    pub fn fail_on(&self, p: impl AsRef<Path>) {
        self.fail.lock().unwrap().insert(p.as_ref().to_path_buf());
    }
    pub fn received(&self) -> Vec<PathBuf> {
        self.received.lock().unwrap().clone()
    }
}

impl TrashBackend for MockTrash {
    fn supported(&self) -> bool {
        !self.unsupported
    }
    fn name(&self) -> String {
        "mock trash".into()
    }
    fn trash(&self, paths: &[PathBuf]) -> Result<TrashReport, TrashError> {
        let mut rep = TrashReport::default();
        let fail = self.fail.lock().unwrap().clone();
        for p in paths {
            self.received.lock().unwrap().push(p.clone());
            if fail.contains(p) {
                rep.results.push((p.clone(), Err(format!("trash {}: permission denied", p.display()))));
            } else {
                if let Some(fs) = &self.fs {
                    let mut st = fs.state.lock().unwrap();
                    for d in st.descendants(p) {
                        st.nodes.remove(&d);
                    }
                }
                rep.results.push((p.clone(), Ok(())));
            }
        }
        Ok(rep)
    }
}

/// Records spawned specs and replays scripted output.
#[derive(Default)]
pub struct MockRunner {
    pub spawned: Mutex<Vec<ProcessSpec>>,
    pub output: Mutex<Vec<String>>,
    pub exit_code: Mutex<Option<i32>>,
}

impl MockRunner {
    pub fn set_output(&self, lines: Vec<String>) {
        *self.output.lock().unwrap() = lines;
    }
    pub fn spawned(&self) -> Vec<ProcessSpec> {
        self.spawned.lock().unwrap().clone()
    }
    pub fn argv(&self) -> Vec<Vec<OsString>> {
        self.spawned()
            .into_iter()
            .map(|s| std::iter::once(s.program).chain(s.args).collect())
            .collect()
    }
}

impl ProcessRunner for MockRunner {
    fn spawn(&self, spec: ProcessSpec) -> Result<ProcessHandle, ProcessError> {
        self.spawned.lock().unwrap().push(spec);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for l in self.output.lock().unwrap().iter() {
            let _ = tx.send(ProcessEvent::Stdout(l.clone()));
        }
        let _ = tx.send(ProcessEvent::Exited(Some(self.exit_code.lock().unwrap().unwrap_or(0))));
        Ok(ProcessHandle { pid: None, events: rx, kill: KillSwitch::default() })
    }
}

/// A settable clock.
pub struct MockClock(pub Mutex<SystemTime>);

impl Default for MockClock {
    fn default() -> Self {
        MockClock(Mutex::new(UNIX_EPOCH + Duration::from_secs(1_750_000_000)))
    }
}

impl Clock for MockClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}
