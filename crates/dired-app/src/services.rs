// dired-7qs
//! Injected adapters and effects the terminal driver must perform.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dired_config::{OpenWithEntry, OpenerRule};
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
    // dired-0cz
    /// Open with an explicitly chosen candidate (the "open with" picker). One
    /// result per launch: `{files}` commands launch once, `{file}` commands
    /// once per path, and one failed launch does not stop the others.
    fn open_with(&self, entry: &OpenWithEntry, paths: &[PathBuf]) -> Vec<Result<OpenOutcome, String>>;
}

// dired-0cz
/// Built-in "open with" candidates, offered after the configured ones:
/// the platform opener and, when set, `$VISUAL`/`$EDITOR`.
pub fn builtin_open_with() -> Vec<OpenWithEntry> {
    let prog = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let mut v = vec![OpenWithEntry {
        name: "System default".into(),
        extensions: vec![],
        command: vec![prog.into(), "{file}".into()],
        terminal: false,
    }];
    let editor = std::env::var("VISUAL")
        .ok()
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|e| !e.trim().is_empty());
    if let Some(ed) = editor {
        let mut command = shlex::split(&ed).filter(|w| !w.is_empty()).unwrap_or_else(|| vec![ed.clone()]);
        command.push("{files}".into()); // dired-vmx: editors take many files
        v.push(OpenWithEntry { name: "$EDITOR".into(), extensions: vec![], command, terminal: true });
    }
    v
}

// dired-vmx
/// The argv lists to launch for `command` over `paths`: a command containing
/// `{files}` runs once with every path in its place (`{file}` then means the
/// first path); otherwise it runs once per path.
pub fn plan_open_with(command: &[String], paths: &[PathBuf]) -> Vec<Vec<OsString>> {
    let Some(first) = paths.first() else { return Vec::new() };
    if command.iter().any(|a| a == "{files}") {
        let req = OpenRequest { path: first.clone(), line: None };
        let argv = command
            .iter()
            .flat_map(|a| {
                if a == "{files}" {
                    paths.iter().map(|p| p.as_os_str().to_os_string()).collect()
                } else {
                    expand(std::slice::from_ref(a), &req)
                }
            })
            .collect();
        return vec![argv];
    }
    paths.iter().map(|p| expand(command, &OpenRequest { path: p.clone(), line: None })).collect()
}

// dired-0cz
/// Run `command` (expanded for `req`) detached or, with `terminal`, in the foreground.
fn run_command(command: &[String], terminal: bool, req: &OpenRequest) -> Result<OpenOutcome, String> {
    let cwd = req.path.parent().unwrap_or(Path::new("/")).to_path_buf();
    let argv = expand(command, req);
    if terminal {
        let mut it = argv.into_iter();
        let prog = it.next().ok_or("empty opener command")?;
        return Ok(OpenOutcome::Foreground(ProcessSpec::new(prog, it.collect(), cwd)));
    }
    spawn_detached(argv, &cwd)?;
    Ok(OpenOutcome::Detached(command.join(" ")))
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
            // dired-0cz
            return run_command(&rule.command, rule.terminal, req);
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

    // dired-0cz
    fn open_with(&self, entry: &OpenWithEntry, paths: &[PathBuf]) -> Vec<Result<OpenOutcome, String>> {
        // dired-vmx
        let single = entry.command.iter().any(|a| a == "{files}");
        plan_open_with(&entry.command, paths)
            .into_iter()
            .enumerate()
            .map(|(i, argv)| {
                let anchor = &paths[if single { 0 } else { i }];
                let cwd = anchor.parent().unwrap_or(Path::new("/")).to_path_buf();
                if entry.terminal {
                    let mut it = argv.into_iter();
                    let prog = it.next().ok_or("empty opener command")?;
                    return Ok(OpenOutcome::Foreground(ProcessSpec::new(prog, it.collect(), cwd)));
                }
                spawn_detached(argv, &cwd)?;
                Ok(OpenOutcome::Detached(entry.command.join(" ")))
            })
            .collect()
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
    // dired-vhh
    /// Put this text on the system clipboard.
    Clipboard(String),
}

// dired-vmx
#[cfg(test)]
mod tests {
    use super::*;

    fn paths(p: &[&str]) -> Vec<PathBuf> {
        p.iter().map(PathBuf::from).collect()
    }

    fn strs(v: &[Vec<OsString>]) -> Vec<Vec<String>> {
        v.iter().map(|a| a.iter().map(|s| s.to_string_lossy().into_owned()).collect()).collect()
    }

    fn cmd(c: &[&str]) -> Vec<String> {
        c.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn file_runs_once_per_path() {
        let v = plan_open_with(&cmd(&["open", "-a", "Preview", "{file}"]), &paths(&["/w/a", "/w/b"]));
        assert_eq!(strs(&v), [["open", "-a", "Preview", "/w/a"], ["open", "-a", "Preview", "/w/b"]]);
    }

    #[test]
    fn files_runs_once_with_every_path_in_place() {
        let v = plan_open_with(&cmd(&["nvim", "-p", "{files}", "+1"]), &paths(&["/w/a", "/w/b", "/w/c"]));
        assert_eq!(strs(&v), [["nvim", "-p", "/w/a", "/w/b", "/w/c", "+1"]]);
    }

    #[test]
    fn nothing_selected_runs_nothing() {
        assert!(plan_open_with(&cmd(&["x", "{file}"]), &[]).is_empty());
        assert!(plan_open_with(&cmd(&["x", "{files}"]), &[]).is_empty());
    }

    #[test]
    fn terminal_commands_become_ordered_foreground_specs() {
        let e = OpenWithEntry { name: "Ed".into(), extensions: vec![], command: cmd(&["ed", "{file}"]), terminal: true };
        let out = SystemOpener { rules: vec![] }.open_with(&e, &paths(&["/w/a", "/w/b"]));
        let specs: Vec<_> = out
            .into_iter()
            .map(|r| match r.unwrap() {
                OpenOutcome::Foreground(s) => (s.program, s.args, s.cwd),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0], ("ed".into(), vec![OsString::from("/w/a")], PathBuf::from("/w")));
        assert_eq!(specs[1], ("ed".into(), vec![OsString::from("/w/b")], PathBuf::from("/w")));
        let e = OpenWithEntry { command: cmd(&["ed", "{files}"]), ..e };
        let out = SystemOpener { rules: vec![] }.open_with(&e, &paths(&["/w/a", "/w/b"]));
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn one_failed_launch_does_not_stop_the_others() {
        let dir = std::env::temp_dir();
        let good = dir.join("dired-vmx-good");
        let bad = dir.join("dired-vmx-no-such-dir").join("bad"); // its working directory does not exist
        let e = OpenWithEntry {
            name: "T".into(),
            extensions: vec![],
            command: cmd(&["/bin/sh", "-c", "exit 0", "sh", "{file}"]),
            terminal: false,
        };
        let out = SystemOpener { rules: vec![] }.open_with(&e, &[bad, good]);
        assert_eq!(out.len(), 2);
        assert!(out[0].is_err(), "{:?}", out[0]);
        assert!(out[1].is_ok(), "{:?}", out[1]);
    }
}
