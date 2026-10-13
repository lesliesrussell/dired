// dired-7qs
//! Buffer model: every view is a buffer.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use dired_domain::plan::{Conflict, ItemOutcome, OpKind, OpPlan, OpResult, SourceFact};
use dired_process::KillSwitch;

use crate::dirbuf::DirBuffer;
use crate::keymap::Mode;

pub type BufferId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Normal,
    Header,
    Dim,
    Warning,
    Error,
    Conflict,
    Success,
    Match,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLine {
    pub text: String,
    pub style: LineStyle,
}

impl TextLine {
    pub fn new(text: impl Into<String>, style: LineStyle) -> Self {
        TextLine { text: text.into(), style }
    }
    pub fn normal(text: impl Into<String>) -> Self {
        Self::new(text, LineStyle::Normal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    Help,
    Messages,
    Diagnostics,
    Process,
    Search,
    // dired-6xv
    View,
}

/// A location a text line can visit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    pub line: Option<u64>,
}

#[derive(Debug)]
pub struct ProcState {
    pub label: String,
    pub running: bool,
    pub exit: Option<Option<i32>>,
    pub kill: Option<KillSwitch>,
    pub shell_mode: bool,
}

#[derive(Debug)]
pub struct TextBuffer {
    pub role: TextRole,
    pub lines: Vec<TextLine>,
    pub targets: Vec<Option<Target>>,
    pub point: usize,
    pub top: usize,
    pub proc: Option<ProcState>,
    /// Keep point on the last line as output arrives.
    pub follow: bool,
    pub cwd: PathBuf,
}

impl TextBuffer {
    pub fn new(role: TextRole, lines: Vec<TextLine>) -> Self {
        let n = lines.len();
        TextBuffer {
            role,
            lines,
            targets: vec![None; n],
            point: 0,
            top: 0,
            proc: None,
            follow: false,
            cwd: PathBuf::new(),
        }
    }

    pub fn push(&mut self, line: TextLine, target: Option<Target>) {
        self.lines.push(line);
        self.targets.push(target);
        if self.follow {
            self.point = self.lines.len() - 1;
        }
    }
}

#[derive(Debug)]
pub enum PreviewPhase {
    Pending,
    Running { done: usize, total: usize },
    Done(OpResult),
}

#[derive(Debug)]
pub struct PreviewBuffer {
    pub plan: OpPlan,
    pub origin: BufferId,
    pub point: usize,
    pub top: usize,
    pub phase: PreviewPhase,
    pub cancel: Arc<AtomicBool>,
    /// Kept so a deletion preview can switch between trash and permanent.
    pub delete_facts: Option<Vec<SourceFact>>,
    pub trash_supported: bool,
}

impl PreviewBuffer {
    /// Header lines, then one line per item. Returns (lines, index of first item line).
    pub fn lines(&self) -> (Vec<TextLine>, usize) {
        let p = &self.plan;
        let mut v = Vec::new();
        let title = match &self.phase {
            PreviewPhase::Pending => format!("Operation preview: {}", p.kind.label()),
            PreviewPhase::Running { done, total } => {
                format!("Running: {} ({done}/{total})", p.kind.label())
            }
            PreviewPhase::Done(_) => format!("Result: {}", p.kind.label()),
        };
        v.push(TextLine::new(title, LineStyle::Header));
        if !p.backend.is_empty() {
            let style = if p.kind == OpKind::DeletePermanent { LineStyle::Error } else { LineStyle::Normal };
            v.push(TextLine::new(format!("Backend: {}", p.backend), style));
        }
        if let Some(m) = p.mode {
            v.push(TextLine::normal(format!("Mode: {:o}", m)));
        }
        let dirs = p.items.iter().filter(|i| i.is_dir).count();
        v.push(TextLine::normal(format!(
            "Items: {} ({} directories) · conflicts: {} · undecided: {} · skipped: {}",
            p.items.len(),
            dirs,
            p.conflicts(),
            p.unresolved(),
            p.items.iter().filter(|i| i.skipped()).count()
        )));
        for w in &p.warnings {
            v.push(TextLine::new(format!("warning: {w}"), LineStyle::Warning));
        }
        for e in &p.errors {
            v.push(TextLine::new(format!("error: {e}"), LineStyle::Error));
        }
        match &self.phase {
            PreviewPhase::Pending => {
                let mut help = String::from("x/C-c C-c execute · q/C-g cancel");
                if p.kind.has_destination() && p.kind != OpKind::Rename {
                    help.push_str(" · o overwrite · s skip · r rename · a ask · O overwrite all · S skip all");
                }
                if matches!(p.kind, OpKind::Trash | OpKind::DeletePermanent) {
                    help.push_str(" · D toggle permanent · s skip");
                }
                v.push(TextLine::new(help, LineStyle::Dim));
                match p.can_execute() {
                    Ok(()) => v.push(TextLine::new("Ready to execute.", LineStyle::Success)),
                    Err(e) => v.push(TextLine::new(format!("Blocked: {e}"), LineStyle::Conflict)),
                }
            }
            PreviewPhase::Running { .. } => {
                v.push(TextLine::new("C-g cancels remaining items", LineStyle::Dim));
            }
            PreviewPhase::Done(r) => {
                v.push(TextLine::new(
                    format!(
                        "Done: {} succeeded, {} failed, {} skipped{}",
                        r.succeeded(),
                        r.failed(),
                        r.count(|o| *o == ItemOutcome::Skipped),
                        if r.cancelled { " (cancelled)" } else { "" }
                    ),
                    if r.failed() > 0 { LineStyle::Error } else { LineStyle::Success },
                ));
            }
        }
        v.push(TextLine::normal(""));
        let first = v.len();
        for (i, item) in p.items.iter().enumerate() {
            let arrow = match item.effective_dst() {
                Some(d) => format!("{} → {}", item.src.display(), d.display()),
                None => item.src.display().to_string(),
            };
            let (state, style) = match &self.phase {
                PreviewPhase::Done(r) => match r.results.iter().find(|x| x.index == i).map(|x| &x.outcome) {
                    Some(ItemOutcome::Done) => ("done".to_string(), LineStyle::Success),
                    Some(ItemOutcome::Skipped) => ("skipped".to_string(), LineStyle::Dim),
                    Some(ItemOutcome::Failed(e)) => (format!("FAILED: {e}"), LineStyle::Error),
                    None => ("?".to_string(), LineStyle::Dim),
                },
                _ => {
                    let style = match &item.conflict {
                        Some(_) if item.needs_decision() => LineStyle::Conflict,
                        Some(Conflict::DestinationExists) => LineStyle::Warning,
                        Some(_) => LineStyle::Warning,
                        None if item.skipped() => LineStyle::Dim,
                        None => LineStyle::Normal,
                    };
                    (item.state_label(), style)
                }
            };
            let mut flags = String::new();
            if item.cross_device {
                flags.push_str(" [cross-device: non-atomic]");
            }
            if item.case_only {
                flags.push_str(" [case-only]");
            }
            if item.is_dir {
                flags.push_str(" [dir]");
            }
            v.push(TextLine::new(format!("{arrow}  ({state}){flags}"), style));
        }
        (v, first)
    }
}

pub enum BufferKind {
    Dir(Box<DirBuffer>),
    Text(TextBuffer),
    Preview(PreviewBuffer),
}

pub struct Buffer {
    pub id: BufferId,
    pub name: String,
    pub kind: BufferKind,
}

impl Buffer {
    pub fn mode(&self) -> Mode {
        match &self.kind {
            BufferKind::Dir(d) if d.wdired.is_some() => Mode::Wdired,
            BufferKind::Dir(_) => Mode::Dired,
            BufferKind::Text(_) => Mode::Text,
            BufferKind::Preview(_) => Mode::Preview,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match &self.kind {
            BufferKind::Dir(_) => "dir",
            BufferKind::Text(t) => match t.role {
                TextRole::Help => "help",
                TextRole::Messages => "messages",
                TextRole::Diagnostics => "diagnostics",
                TextRole::Process => "process",
                TextRole::Search => "search",
                TextRole::View => "view", // dired-6xv
            },
            BufferKind::Preview(_) => "operation",
        }
    }

    pub fn dir(&self) -> Option<&DirBuffer> {
        match &self.kind {
            BufferKind::Dir(d) => Some(d),
            _ => None,
        }
    }

    pub fn dir_mut(&mut self) -> Option<&mut DirBuffer> {
        match &mut self.kind {
            BufferKind::Dir(d) => Some(d),
            _ => None,
        }
    }

    /// Everything except the automatic *Diagnostics* / *Messages* buffers.
    pub fn is_user_buffer(&self) -> bool {
        !matches!(&self.kind, BufferKind::Text(t) if matches!(t.role, TextRole::Diagnostics | TextRole::Messages))
    }

    /// Unfinished work that makes closing worth confirming.
    pub fn has_unfinished_work(&self) -> bool {
        match &self.kind {
            BufferKind::Dir(d) => d
                .wdired
                .as_ref()
                .map(|w| !w.edits.is_empty())
                .unwrap_or(false),
            BufferKind::Preview(p) => !matches!(p.phase, PreviewPhase::Done(_)),
            BufferKind::Text(t) => t.proc.as_ref().map(|p| p.running).unwrap_or(false),
        }
    }
}
