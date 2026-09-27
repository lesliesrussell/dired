// dired-7qs
//! Trash abstraction. Failures are reported per path so one failure never
//! aborts unrelated deletions.

use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrashReport {
    pub results: Vec<(PathBuf, Result<(), String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashError(pub String);

impl std::fmt::Display for TrashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TrashError {}

pub trait TrashBackend: Send + Sync {
    fn supported(&self) -> bool;
    fn name(&self) -> String {
        "trash".into()
    }
    fn trash(&self, paths: &[PathBuf]) -> Result<TrashReport, TrashError>;
}

/// The platform trash / recycle bin.
#[derive(Debug, Default)]
pub struct SystemTrash;

impl TrashBackend for SystemTrash {
    fn supported(&self) -> bool {
        cfg!(all(
            feature = "system-trash",
            any(target_os = "macos", target_os = "linux", target_os = "windows", target_os = "freebsd")
        ))
    }

    fn name(&self) -> String {
        if cfg!(target_os = "macos") {
            "macOS Trash".into()
        } else if cfg!(windows) {
            "Recycle Bin".into()
        } else {
            "freedesktop.org Trash".into()
        }
    }

    fn trash(&self, paths: &[PathBuf]) -> Result<TrashReport, TrashError> {
        if !self.supported() {
            return Err(TrashError("trash is not supported on this platform".into()));
        }
        let mut report = TrashReport::default();
        for p in paths {
            #[cfg(feature = "system-trash")]
            let r = trash::delete(p).map_err(|e| format!("trash {}: {e}", p.display()));
            #[cfg(not(feature = "system-trash"))]
            let r: Result<(), String> = Err("trash support not compiled in".into());
            report.results.push((p.clone(), r));
        }
        Ok(report)
    }
}

/// Fallback used when trash is unavailable: always unsupported, so plans
/// require an explicit permanent deletion instead.
#[derive(Debug, Default)]
pub struct NoTrash;

impl TrashBackend for NoTrash {
    fn supported(&self) -> bool {
        false
    }
    fn name(&self) -> String {
        "none".into()
    }
    fn trash(&self, _paths: &[PathBuf]) -> Result<TrashReport, TrashError> {
        Err(TrashError("no trash backend available".into()))
    }
}
