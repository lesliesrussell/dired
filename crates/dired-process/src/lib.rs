// dired-7qs
//! Argv-based process execution with streamed output. No API here ever
//! builds a shell command line from strings.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Notify};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: Vec<(OsString, OsString)>,
}

impl ProcessSpec {
    pub fn new(program: impl Into<OsString>, args: Vec<OsString>, cwd: PathBuf) -> Self {
        ProcessSpec { program: program.into(), args, cwd, env: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Stdout(String),
    Stderr(String),
    /// Process ended; `None` when killed by a signal.
    Exited(Option<i32>),
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("cannot run {program}: {message}")]
pub struct ProcessError {
    pub program: String,
    pub message: String,
}

/// Cancels a running process.
#[derive(Debug, Clone, Default)]
pub struct KillSwitch(Arc<Notify>);

impl KillSwitch {
    pub fn kill(&self) {
        self.0.notify_one();
    }
    pub async fn wait(&self) {
        self.0.notified().await
    }
}

#[derive(Debug)]
pub struct ProcessHandle {
    pub pid: Option<u32>,
    pub events: mpsc::UnboundedReceiver<ProcessEvent>,
    pub kill: KillSwitch,
}

pub trait ProcessRunner: Send + Sync {
    fn spawn(&self, spec: ProcessSpec) -> Result<ProcessHandle, ProcessError>;
}

/// Runs real processes with `tokio::process::Command` (argv only).
#[derive(Debug, Default)]
pub struct LocalRunner;

impl ProcessRunner for LocalRunner {
    fn spawn(&self, spec: ProcessSpec) -> Result<ProcessHandle, ProcessError> {
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| ProcessError {
            program: spec.program.to_string_lossy().into_owned(),
            message: e.to_string(),
        })?;
        let pid = child.id();
        let (tx, rx) = mpsc::unbounded_channel();
        let kill = KillSwitch::default();
        let k2 = kill.clone();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        tokio::spawn(async move {
            let mut out = stdout.map(|s| BufReader::new(s).lines());
            let mut err = stderr.map(|s| BufReader::new(s).lines());
            let mut killed = false;
            loop {
                tokio::select! {
                    l = async { out.as_mut().unwrap().next_line().await }, if out.is_some() => match l {
                        Ok(Some(l)) => { let _ = tx.send(ProcessEvent::Stdout(l)); }
                        _ => out = None,
                    },
                    l = async { err.as_mut().unwrap().next_line().await }, if err.is_some() => match l {
                        Ok(Some(l)) => { let _ = tx.send(ProcessEvent::Stderr(l)); }
                        _ => err = None,
                    },
                    _ = k2.wait(), if !killed && (out.is_some() || err.is_some()) => {
                        let _ = child.start_kill();
                        killed = true;
                    }
                    else => break,
                }
            }
            let status = child.wait().await.ok().and_then(|s| s.code());
            let _ = tx.send(ProcessEvent::Exited(status));
        });
        Ok(ProcessHandle { pid, events: rx, kill })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn streams_output_and_exit() {
        let r = LocalRunner;
        let mut h = r
            .spawn(ProcessSpec::new("printf", vec!["%s\n".into(), "a b".into(), "$HOME".into()], std::env::temp_dir()))
            .unwrap();
        let mut lines = vec![];
        while let Some(ev) = h.events.recv().await {
            match ev {
                ProcessEvent::Stdout(l) => lines.push(l),
                ProcessEvent::Exited(c) => {
                    assert_eq!(c, Some(0));
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(lines, vec!["a b", "$HOME"]);
    }
}
