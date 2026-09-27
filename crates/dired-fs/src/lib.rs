// dired-7qs
//! Filesystem, trash, and plan-execution adapters. The UI never calls
//! filesystem APIs directly; the application layer goes through these traits.

mod exec;
mod local;
mod trash_backend;

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use dired_domain::{EntryMetadata, FileKind};

pub use exec::execute_plan;
pub use local::LocalFs;
pub use trash_backend::{NoTrash, SystemTrash, TrashBackend, TrashError, TrashReport};

/// A name produced by directory enumeration, before any stat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDirEntry {
    pub name: OsString,
    /// File type from the directory entry when cheaply available.
    pub kind: FileKind,
}

/// Batches of names, streamed as enumeration proceeds.
pub type DirectoryStream = tokio::sync::mpsc::Receiver<Result<Vec<RawDirEntry>, FsError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsErrorKind {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    CrossDevice,
    NotEmpty,
    Unsupported,
    Other,
}

/// A filesystem error that always names the path and operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsError {
    pub op: &'static str,
    pub path: PathBuf,
    pub kind: FsErrorKind,
    pub message: String,
    pub code: Option<i32>,
}

impl FsError {
    pub fn new(op: &'static str, path: &Path, kind: FsErrorKind, message: impl Into<String>) -> Self {
        FsError {
            op,
            path: path.to_path_buf(),
            kind,
            message: message.into(),
            code: None,
        }
    }

    pub fn from_io(op: &'static str, path: &Path, e: &std::io::Error) -> Self {
        let code = e.raw_os_error();
        let kind = match e.kind() {
            std::io::ErrorKind::NotFound => FsErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
            std::io::ErrorKind::AlreadyExists => FsErrorKind::AlreadyExists,
            std::io::ErrorKind::Unsupported => FsErrorKind::Unsupported,
            _ if code == Some(18) => FsErrorKind::CrossDevice, // EXDEV on Linux and macOS
            _ if code == Some(39) || code == Some(66) => FsErrorKind::NotEmpty, // ENOTEMPTY
            _ => FsErrorKind::Other,
        };
        FsError {
            op,
            path: path.to_path_buf(),
            kind,
            message: e.to_string(),
            code,
        }
    }

    pub fn hint(&self) -> Option<&'static str> {
        match self.kind {
            FsErrorKind::NotFound => Some("refresh the buffer (g); the file may have been moved"),
            FsErrorKind::PermissionDenied => Some("check ownership and permissions (M changes mode)"),
            FsErrorKind::AlreadyExists => Some("choose overwrite, skip, or rename in the preview"),
            FsErrorKind::CrossDevice => Some("the move will be performed as copy-then-delete"),
            FsErrorKind::NotEmpty => Some("the directory is not empty"),
            _ => None,
        }
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.op, self.path.display(), self.message)?;
        if let Some(c) = self.code {
            if !self.message.contains("os error") {
                write!(f, " (os error {c})")?;
            }
        }
        if let Some(h) = self.hint() {
            write!(f, " — {h}")?;
        }
        Ok(())
    }
}

impl std::error::Error for FsError {}

#[derive(Debug, Clone, Copy, Default)]
pub struct CopyOptions {
    pub recursive: bool,
    pub overwrite: bool,
    /// Follow directory symlinks while copying recursively (off by default).
    pub follow_symlinks: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenameOptions {
    pub overwrite: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RemoveOptions {
    pub recursive: bool,
}

#[async_trait::async_trait]
pub trait FileSystem: Send + Sync {
    async fn read_dir(&self, path: &Path) -> Result<DirectoryStream, FsError>;
    async fn metadata(&self, path: &Path, follow_symlinks: bool) -> Result<EntryMetadata, FsError>;
    async fn copy(&self, from: &Path, to: &Path, options: CopyOptions) -> Result<(), FsError>;
    async fn rename(&self, from: &Path, to: &Path, options: RenameOptions) -> Result<(), FsError>;
    async fn remove(&self, path: &Path, options: RemoveOptions) -> Result<(), FsError>;
    async fn create_dir(&self, path: &Path) -> Result<(), FsError>;
    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<(), FsError>;

    /// Resolve symlinks, for alias detection. Adapters without a notion of
    /// links may return the path unchanged.
    async fn canonicalize(&self, path: &Path) -> Result<PathBuf, FsError> {
        Ok(path.to_path_buf())
    }

    /// Whether `path` exists (without following a final symlink).
    async fn exists(&self, path: &Path) -> bool {
        self.metadata(path, false).await.is_ok()
    }
}
