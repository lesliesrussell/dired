// dired-7qs
//! Injected adapters and effects the terminal driver must perform.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dired_config::OpenerRule;
use dired_domain::clock::Clock;
use dired_fs::{FileSystem, TrashBackend};
use dired_process::{ProcessRunner, ProcessSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    pub path: PathBuf,
    pub line: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    /// Opened outside the terminal (e.g. `open`, `xdg-open`).
    Detached(String),
    /// Must run in the terminal; the driver suspends the UI.
    Foreground(ProcessSpec),
}

pub trait Opener: Send + Sync {
    fn open(&self, req: &OpenRequest) -> Result<OpenOutcome, String>;
}

/// Opener driven by config rules, then `$VISUAL`/`$EDITOR`, then the
/// platform opener.
pub struct SystemOpener {
    pub rules: Vec<OpenerRule>,
}

fn expand(argv: &[String], req: &OpenRequest) -> Vec<OsString> {
    argv.iter()
        .flat_map(|a| {
            if a == "{file}" {
                vec![req.path.as_os_str().to_os_string()]
            } else if a.contains("{line}") {
                match req.line {
                    Some(l) => vec![OsString::from(a.replace("{line}", &l.to_string()))],
                    None => vec![],
                }
            } else {
                vec![OsString::from(a)]
            }
        })
        .collect()
}

fn spawn_detached(argv: Vec<OsString>, cwd: &Path) -> Result<(), String> {
    let mut it = argv.into_iter();
    let prog = it.next().ok_or("empty opener command")?;
    let mut child = std::process::Command::new(&prog)
        .args(it)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", prog.to_string_lossy()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

impl Opener for SystemOpener {
    fn open(&self, req: &OpenRequest) -> Result<OpenOutcome, String> {
        let ext = req
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let cwd = req.path.parent().unwrap_or(Path::new("/")).to_path_buf();
        if let Some(rule) = self.rules.iter().find(|r| {
            r.extensions.is_empty() || r.extensions.iter().any(|e| e.to_lowercase() == ext)
        }) {
            let argv = expand(&rule.command, req);
            if rule.terminal {
                let mut it = argv.into_iter();
                let prog = it.next().ok_or("empty opener command")?;
                return Ok(OpenOutcome::Foreground(ProcessSpec::new(prog, it.collect(), cwd)));
            }
            let desc = rule.command.join(" ");
            spawn_detached(argv, &cwd)?;
            return Ok(OpenOutcome::Detached(desc));
        }
        let editor = std::env::var("VISUAL")
            .ok()
            .or_else(|| std::env::var("EDITOR").ok())
            .filter(|e| !e.trim().is_empty());
        if let Some(ed) = editor {
            // The editor variable may carry flags ("code -w"); split it as words.
            let mut words = shlex::split(&ed).unwrap_or_else(|| vec![ed.clone()]);
            let prog = words.remove(0);
            let mut args: Vec<OsString> = words.into_iter().map(OsString::from).collect();
            if let Some(l) = req.line {
                args.push(OsString::from(format!("+{l}")));
            }
            args.push(req.path.as_os_str().to_os_string());
            return Ok(OpenOutcome::Foreground(ProcessSpec::new(prog, args, cwd)));
        }
        let prog = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        spawn_detached(vec![prog.into(), req.path.as_os_str().to_os_string()], &cwd)?;
        Ok(OpenOutcome::Detached(prog.into()))
    }
}

/// Everything the application needs from the outside world.
#[derive(Clone)]
pub struct Services {
    pub fs: Arc<dyn FileSystem>,
    pub trash: Arc<dyn TrashBackend>,
    pub runner: Arc<dyn ProcessRunner>,
    pub opener: Arc<dyn Opener>,
    pub clock: Arc<dyn Clock>,
    /// Where history and the journal persist; `None` disables persistence.
    pub state_dir: Option<PathBuf>,
}

/// Side effects the terminal driver performs on the app's behalf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Foreground(ProcessSpec),
}
