// dired-7qs
//! The application reducer: one state machine handles keys and task
//! events deterministically. Filesystem work happens only in tasks.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Component, Path, PathBuf};

use dired_config::{format_seq, Config, Diagnostic, Key, KeyCode};
use dired_domain::journal::JournalEntry;
use dired_domain::{OpPlan, SortKey, SortSpec};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::buffers::{Buffer, BufferId, BufferKind, LineStyle, PreviewBuffer, PreviewPhase, TextBuffer, TextLine, TextRole};
use crate::dirbuf::DirBuffer;
use crate::keymap::{Binding, Keymaps, Lookup, Mode};
use crate::minibuffer::{common_prefix, fuzzy, Completion, EditResult, Prompt, PromptAction};
use crate::registry;
use crate::services::{Effect, Services};
use crate::tasks::{self, AppEvent, Tx};

/// Expand `~` and resolve relative input against `base`, lexically
/// (symlinks are not resolved, so user-visible paths are preserved).
pub fn expand_path(input: &str, base: &Path) -> PathBuf {
    // dired-ta2: as in Emacs, typing `~/` or `/` after a pre-filled path
    // starts a new path (`/src/~/backup` is `~/backup`, `/src//tmp` is `/tmp`).
    let slash = input.rfind("//").map(|i| i + 1);
    let tilde = input
        .match_indices("/~")
        .filter(|(i, _)| matches!(input.as_bytes().get(i + 2), None | Some(b'/')))
        .map(|(i, _)| i + 1)
        .last();
    let input = match slash.max(tilde) {
        Some(i) => &input[i..],
        None => input,
    };
    let p = if input == "~" || input.starts_with("~/") {
        let home = dirs_home();
        home.join(input.trim_start_matches('~').trim_start_matches('/'))
    } else {
        let p = Path::new(input);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        }
    };
    normalize(&p)
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

pub struct App {
    pub(crate) svc: Services,
    pub config: Config,
    pub(crate) keymaps: Keymaps,
    pub(crate) buffers: Vec<Buffer>,
    pub(crate) active: BufferId,
    pub(crate) mru: Vec<BufferId>,
    // dired-ta2
    pub(crate) windows: Vec<crate::windows::Window>,
    pub(crate) layout: crate::windows::Layout,
    pub(crate) selected: crate::windows::WindowId,
    pub(crate) next_win: crate::windows::WindowId,
    pub(crate) area: crate::windows::Rect,
    next_id: BufferId,
    pub(crate) tx: Tx,
    rx: Option<UnboundedReceiver<AppEvent>>,
    pending_tasks: usize,
    pub(crate) prompt: Option<Prompt>,
    pending_keys: Vec<Key>,
    esc_meta: bool,
    message: Option<(String, bool)>,
    pub(crate) messages: Vec<String>,
    pub(crate) history: HashMap<String, Vec<String>>,
    pub(crate) journal: Vec<JournalEntry>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) viewport: usize,
    pub(crate) quit: bool,
    pub(crate) effects: Vec<Effect>,
    next_op: u64,
    pub(crate) pending_operands: Vec<(PathBuf, Option<PathBuf>)>,
    pub(crate) inflight: HashMap<u64, (OpPlan, BufferId)>,
    // dired-7yu
    pub(crate) config_gen: u64,
}

impl App {
    pub fn new(config: Config, config_diags: Vec<Diagnostic>, svc: Services) -> App {
        let (tx, rx) = unbounded_channel();
        let (keymaps, key_diags) = Keymaps::with_user(&config.keymap, "config keymap");
        let mut app = App {
            svc,
            config,
            keymaps,
            buffers: Vec::new(),
            active: 0,
            mru: Vec::new(),
            windows: Vec::new(),
            layout: crate::windows::Layout::Leaf(1),
            selected: 1,
            next_win: 1,
            area: crate::windows::Rect { x: 0, y: 0, w: 80, h: 23 },
            next_id: 1,
            tx,
            rx: Some(rx),
            pending_tasks: 0,
            prompt: None,
            pending_keys: Vec::new(),
            esc_meta: false,
            message: None,
            messages: Vec::new(),
            history: HashMap::new(),
            journal: Vec::new(),
            diagnostics: Vec::new(),
            viewport: 20,
            quit: false,
            effects: Vec::new(),
            next_op: 1,
            pending_operands: Vec::new(),
            inflight: HashMap::new(),
            config_gen: 0,
        };
        app.load_history();
        app.set_diagnostics(config_diags.into_iter().chain(key_diags).collect());
        app
    }

    // ----- driver API -----

    pub fn take_events(&mut self) -> UnboundedReceiver<AppEvent> {
        self.rx.take().expect("events already taken")
    }

    pub fn pending_tasks(&self) -> usize {
        self.pending_tasks
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    pub fn set_viewport(&mut self, rows: usize) {
        self.viewport = rows.max(1);
    }

    // dired-7yu
    /// Bumped whenever configuration (e.g. the theme) changes.
    pub fn config_generation(&self) -> u64 {
        self.config_gen
    }

    pub fn buffers(&self) -> &[Buffer] {
        &self.buffers
    }

    pub fn active_id(&self) -> BufferId {
        self.active
    }

    pub fn active(&self) -> &Buffer {
        self.buffer(self.active).expect("active buffer")
    }

    pub fn active_mut(&mut self) -> &mut Buffer {
        let id = self.active;
        self.buffer_mut(id).expect("active buffer")
    }

    pub fn buffer(&self, id: BufferId) -> Option<&Buffer> {
        self.buffers.iter().find(|b| b.id == id)
    }

    pub fn buffer_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.buffers.iter_mut().find(|b| b.id == id)
    }

    pub fn find_buffer(&self, name: &str) -> Option<BufferId> {
        self.buffers.iter().find(|b| b.name == name).map(|b| b.id)
    }

    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    pub fn echo(&self) -> Option<(&str, bool)> {
        self.message.as_ref().map(|(m, e)| (m.as_str(), *e))
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn keymaps(&self) -> &Keymaps {
        &self.keymaps
    }

    /// Transient hint for a pending prefix key.
    pub fn key_hint(&self) -> Option<String> {
        if self.esc_meta {
            return Some("ESC- (meta)".into());
        }
        if self.pending_keys.is_empty() {
            return None;
        }
        let modes = self.modes();
        let conts = self.keymaps.continuations(&modes, &self.pending_keys);
        let body: Vec<String> = conts
            .iter()
            .take(10)
            .map(|(k, c)| format!("{k}:{}", c.trim_start_matches("dired-")))
            .collect();
        Some(format!("{}- {}", format_seq(&self.pending_keys), body.join("  ")))
    }

    /// Per-frame preparation: rebuild the active view and keep point visible.
    pub fn prepare_frame(&mut self) {
        if self.buffers.is_empty() {
            return;
        }
        // dired-ta2: directory buffers shown in other windows need rows too.
        let shown: Vec<BufferId> = self.windows.iter().map(|w| w.buffer).collect();
        for id in shown {
            if let Some(d) = self.dir_of(id) {
                d.ensure_view();
            }
        }
        let h = self.viewport;
        match &mut self.active_mut().kind {
            BufferKind::Dir(d) => {
                d.ensure_view();
                d.adjust_top(h.saturating_sub(1));
            }
            BufferKind::Text(t) => {
                t.point = t.point.min(t.lines.len().saturating_sub(1));
                adjust(&mut t.top, t.point, h);
            }
            BufferKind::Preview(p) => {
                let (_, first) = p.lines();
                if p.point == 0 {
                    p.top = 0; // keep the summary header visible
                }
                adjust(&mut p.top, first + p.point, h);
            }
        }
    }

    pub fn status_line(&mut self) -> String {
        self.status_for(self.active)
    }

    // dired-ta2
    /// Mode-line text for any buffer.
    pub fn status_for(&mut self, id: BufferId) -> String {
        let Some(b) = self.buffer_mut(id) else { return String::new() };
        let name = b.name.clone();
        match &mut b.kind {
            BufferKind::Dir(d) => {
                let c = d.counts();
                let mut s = format!(
                    "{name}  marked:{} flagged:{}  [{}]  filter:{}  sort:{}{}{}",
                    c.marked,
                    c.flagged,
                    d.scan_summary(),
                    d.filter.as_deref().unwrap_or("-"),
                    d.sort.label(),
                    if d.sort.dirs_first { "" } else { " (mixed)" },
                    if d.show_hidden { "" } else { "  hidden:off" },
                );
                if let Some(w) = &d.wdired {
                    s.push_str(&format!("  WDIRED ({} edited)", w.edits.len()));
                }
                s
            }
            BufferKind::Text(t) => {
                let state = match &t.proc {
                    Some(p) if p.running => format!(" running: {}", p.label),
                    Some(p) => match p.exit.flatten() {
                        Some(c) => format!(" exited {c}"),
                        None => " killed".to_string(),
                    },
                    None => String::new(),
                };
                format!("{name}  [{} lines]{state}", t.lines.len())
            }
            BufferKind::Preview(p) => {
                let phase = match &p.phase {
                    PreviewPhase::Pending => "pending".to_string(),
                    PreviewPhase::Running { done, total } => format!("running {done}/{total}"),
                    PreviewPhase::Done(_) => "done".into(),
                };
                format!("{name}  [{phase}]  undecided:{}", p.plan.unresolved())
            }
        }
    }

    // ----- infrastructure -----

    pub(crate) fn spawn<F>(&mut self, f: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.pending_tasks += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            f.await;
            let _ = tx.send(AppEvent::TaskFinished);
        });
    }

    pub(crate) fn op_id(&mut self) -> u64 {
        let id = self.next_op;
        self.next_op += 1;
        id
    }

    pub fn message(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.messages.push(t.clone());
        self.message = Some((t, false));
    }

    pub fn error(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.messages.push(format!("error: {t}"));
        self.message = Some((t, true));
    }

    pub(crate) fn add_buffer(&mut self, name: &str, kind: BufferKind) -> BufferId {
        let id = self.next_id;
        self.next_id += 1;
        let mut unique = name.to_string();
        let mut n = 2;
        while self.buffers.iter().any(|b| b.name == unique) {
            unique = format!("{name}<{n}>");
            n += 1;
        }
        self.buffers.push(Buffer { id, name: unique, kind });
        id
    }

    /// Show a buffer in the selected window.
    pub fn focus(&mut self, id: BufferId) {
        if self.buffer(id).is_none() {
            return;
        }
        // dired-ta2
        self.show_in_selected(id);
    }

    pub(crate) fn remove_buffer(&mut self, id: BufferId) {
        if let Some(b) = self.buffer_mut(id) {
            match &mut b.kind {
                BufferKind::Dir(d) => d.cancel_all(),
                BufferKind::Text(t) => {
                    if let Some(k) = t.proc.as_ref().and_then(|p| p.kill.clone()) {
                        k.kill();
                    }
                }
                BufferKind::Preview(p) => p.cancel.store(true, std::sync::atomic::Ordering::Relaxed),
            }
        }
        self.buffers.retain(|b| b.id != id);
        self.mru.retain(|b| *b != id);
        if self.buffers.is_empty() {
            self.quit = true;
            self.windows.clear();
            return;
        }
        // dired-ta2: popped-up windows close with their buffer; other windows
        // showing it switch to the most recent buffer not already visible.
        let showing: Vec<crate::windows::WindowId> =
            self.windows.iter().filter(|w| w.buffer == id).map(|w| w.id).collect();
        for w in showing {
            let popup = self.window(w).map(|x| x.popup).unwrap_or(false);
            if popup && self.windows.len() > 1 {
                self.delete_window_raw(w);
                continue;
            }
            let visible: Vec<BufferId> = self.windows.iter().map(|x| x.buffer).collect();
            let rep = self
                .mru
                .iter()
                .copied()
                .find(|b| !visible.contains(b))
                .or_else(|| self.mru.first().copied())
                .unwrap_or(self.buffers[0].id);
            let v = self.snapshot(rep);
            if let Some(win) = self.window_mut(w) {
                win.buffer = rep;
                win.popup = false;
                win.view = v;
            }
        }
        self.mru.retain(|b| *b != id);
        if let Some(buf) = self.window(self.selected).map(|w| w.buffer) {
            self.active = buf;
            self.mru.retain(|b| *b != buf);
            self.mru.insert(0, buf);
        }
    }

    pub(crate) fn modes(&self) -> Vec<Mode> {
        match self.buffer(self.active) {
            Some(b) => vec![b.mode(), Mode::Global],
            None => vec![Mode::Global],
        }
    }

    pub(crate) fn dir(&mut self) -> Option<&mut DirBuffer> {
        match &mut self.active_mut().kind {
            BufferKind::Dir(d) => Some(d),
            _ => None,
        }
    }

    pub(crate) fn dir_of(&mut self, id: BufferId) -> Option<&mut DirBuffer> {
        self.buffer_mut(id).and_then(|b| b.dir_mut())
    }

    // ----- directories -----

    /// Open or focus a directory buffer.
    pub fn open_dir(&mut self, path: impl AsRef<Path>) -> BufferId {
        let path = normalize(path.as_ref());
        if let Some(id) = self
            .buffers
            .iter()
            .find(|b| b.dir().map(|d| d.root == path).unwrap_or(false))
            .map(|b| b.id)
        {
            self.focus(id);
            return id;
        }
        let sort = SortSpec {
            key: SortKey::parse(&self.config.display.sort).unwrap_or(SortKey::Name),
            dirs_first: self.config.display.dirs_first,
            reverse: false,
        };
        let d = DirBuffer::new(path.clone(), sort, self.config.display.show_hidden);
        let name = path
            .file_name()
            .map(|n| format!("{}/", n.to_string_lossy()))
            .unwrap_or_else(|| path.display().to_string());
        let id = self.add_buffer(&name, BufferKind::Dir(Box::new(d)));
        self.focus(id);
        self.scan_listing(id, PathBuf::new());
        id
    }

    pub(crate) fn scan_listing(&mut self, id: BufferId, key: PathBuf) {
        let Some(d) = self.dir_of(id) else { return };
        let (abs, gen, cancel) = d.begin_scan(&key);
        let fs = self.svc.fs.clone();
        let tx = self.tx.clone();
        self.spawn(tasks::scan(fs, tx, id, key, abs, gen, cancel));
    }

    /// Reload a directory buffer (root and inserted subdirectories).
    pub fn revert(&mut self, id: BufferId) {
        let Some(d) = self.dir_of(id) else { return };
        let mut keys: Vec<PathBuf> = d.listings.keys().cloned().collect();
        keys.sort();
        for k in keys {
            self.scan_listing(id, k);
        }
    }

    /// Open a path from the command line: directories open, files open their
    /// parent with point on the file.
    pub fn open_path(&mut self, path: impl AsRef<Path>, visit: bool) {
        self.open_path_in(path, visit, false)
    }

    // dired-ta2
    pub(crate) fn open_path_in(&mut self, path: impl AsRef<Path>, visit: bool, other: bool) {
        let path = normalize(path.as_ref());
        let fs = self.svc.fs.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            let is_dir = fs
                .metadata(&path, true)
                .await
                .map(|m| m.kind == dired_domain::FileKind::Dir)
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::PathResolved { path, is_dir, visit, other });
        });
    }

    // ----- keys -----

    pub fn handle_key(&mut self, mut key: Key) {
        if self.buffers.is_empty() {
            return; // still loading the first buffer
        }
        if self.esc_meta {
            self.esc_meta = false;
            if key.code == KeyCode::Esc {
                return self.keyboard_quit();
            }
            key.meta = true;
        } else if key.code == KeyCode::Esc && !key.ctrl && !key.meta && self.pending_keys.is_empty() {
            self.esc_meta = true;
            return;
        }
        if key == Key::ctrl('g') {
            return self.keyboard_quit();
        }
        if self.prompt.is_some() {
            return self.prompt_key(key);
        }
        if self.pending_keys.is_empty() && self.active().mode() == Mode::Wdired && self.wdired_key(key) {
            return;
        }
        let mut seq = std::mem::take(&mut self.pending_keys);
        seq.push(key);
        let modes = self.modes();
        let binding = match self.keymaps.lookup(&modes, &seq) {
            Lookup::Found(b) => b.clone(),
            Lookup::Prefix => {
                self.pending_keys = seq;
                return;
            }
            Lookup::None => {
                self.error(format!("{} is undefined", format_seq(&seq)));
                return;
            }
        };
        self.message = None;
        match binding {
            Binding::Command(c) => self.run_command(&c),
            Binding::Exec(argv) => self.run_exec(argv),
            Binding::Plugin(p) => self.error(format!("plugin command `{p}` is unavailable: no plugin runtime in this release")),
        }
    }

    /// Feed a key sequence in Emacs notation (tests and scripting).
    pub fn press(&mut self, seq: &str) {
        for k in dired_config::parse_seq(seq).expect("valid key sequence") {
            self.handle_key(k);
        }
    }

    pub fn type_text(&mut self, s: &str) {
        for c in s.chars() {
            self.handle_key(Key::ch(c));
        }
    }

    pub fn keyboard_quit(&mut self) {
        let had_prefix = !self.pending_keys.is_empty() || self.esc_meta;
        self.pending_keys.clear();
        self.esc_meta = false;
        // 1. prompt
        if let Some(p) = self.prompt.take() {
            if let PromptAction::Isearch { origin, .. } = p.action {
                self.set_point(origin);
            }
            self.message("Quit");
            return;
        }
        // 2. operation preview
        let active = self.active;
        if let BufferKind::Preview(p) = &self.active().kind {
            if matches!(p.phase, PreviewPhase::Pending) {
                let origin = p.origin;
                self.remove_buffer(active);
                self.pop_to(origin);
                self.message("Operation cancelled; nothing was changed");
                return;
            }
        }
        // 3. transient command state
        if had_prefix {
            self.message("Quit");
            return;
        }
        // 4. long-running task
        let cancelled = match &mut self.active_mut().kind {
            BufferKind::Dir(d) => d.cancel_scans(),
            BufferKind::Text(t) => match t.proc.as_ref() {
                Some(p) if p.running => {
                    if let Some(k) = &p.kill {
                        k.kill();
                    }
                    true
                }
                _ => false,
            },
            BufferKind::Preview(p) => {
                if matches!(p.phase, PreviewPhase::Running { .. }) {
                    p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    true
                } else {
                    false
                }
            }
        };
        self.message(if cancelled { "Cancelled running task" } else { "Quit" });
    }

    // ----- prompt -----

    pub(crate) fn open_prompt(&mut self, label: impl Into<String>, initial: impl Into<String>, action: PromptAction) {
        let base = self.base_dir();
        let mut p = Prompt::new(label, initial, action);
        p.base_dir = base;
        p.origin = self.active;
        self.prompt = Some(p);
        self.refresh_candidates();
    }

    pub(crate) fn base_dir(&self) -> PathBuf {
        match &self.active().kind {
            BufferKind::Dir(d) => d.root.clone(),
            BufferKind::Text(t) if !t.cwd.as_os_str().is_empty() => t.cwd.clone(),
            _ => self
                .buffers
                .iter()
                .find_map(|b| b.dir().map(|d| d.root.clone()))
                .unwrap_or_else(|| PathBuf::from("/")),
        }
    }

    fn refresh_candidates(&mut self) {
        let names: Vec<String> = match self.prompt.as_ref().map(|p| p.action.completion()) {
            Some(Completion::Command) => registry::COMMANDS
                .iter()
                .flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied()))
                .map(String::from)
                .collect(),
            Some(Completion::Buffer) => {
                let mut ids = self.mru.clone();
                ids.retain(|i| *i != self.active);
                ids.push(self.active);
                ids.iter().filter_map(|i| self.buffer(*i)).map(|b| b.name.clone()).collect()
            }
            // dired-0cz
            Some(Completion::OpenWith) => match self.prompt.as_ref().map(|p| &p.action) {
                Some(PromptAction::OpenWith { path }) => {
                    self.open_with_entries(path).into_iter().map(|e| e.name).collect()
                }
                _ => return,
            },
            _ => return,
        };
        let p = self.prompt.as_mut().unwrap();
        p.candidates = fuzzy(&p.input, names);
        p.selected = 0;
    }

    fn prompt_key(&mut self, key: Key) {
        let yn = self.prompt.as_ref().map(|p| p.action.yes_or_no_key()).unwrap_or(false);
        if yn {
            match key.printable() {
                Some('y') | Some('Y') => {
                    self.prompt.as_mut().unwrap().input = "y".into();
                    self.submit_prompt();
                }
                Some('n') | Some('N') => {
                    self.prompt = None;
                    self.message("Cancelled");
                }
                _ => self.error("Please answer y or n"),
            }
            return;
        }
        let r = self.prompt.as_mut().unwrap().edit(key);
        match r {
            EditResult::Changed => self.prompt_changed(),
            EditResult::Moved | EditResult::Unhandled => {}
            EditResult::Submit => self.submit_prompt(),
            EditResult::Complete => self.complete(),
            EditResult::HistoryPrev => self.history_step(-1),
            EditResult::HistoryNext => self.history_step(1),
            EditResult::IsearchNext => self.isearch_step(true),
            EditResult::IsearchPrev => self.isearch_step(false),
        }
    }

    fn prompt_changed(&mut self) {
        let p = self.prompt.as_mut().unwrap();
        p.completing_for = None;
        if p.action.completion() == Completion::Path {
            p.candidates.clear();
        }
        if let PromptAction::Isearch { forward, origin } = p.action {
            let q = p.input.clone();
            self.isearch_from(&q, origin, forward, false);
        }
        self.refresh_candidates();
    }

    fn history_step(&mut self, dir: i64) {
        let p = self.prompt.as_mut().unwrap();
        let hist = self.history.get(p.action.history_key()).cloned().unwrap_or_default();
        if hist.is_empty() {
            return;
        }
        let pos = p.hist_pos.map(|x| x as i64).unwrap_or(hist.len() as i64) + dir;
        if pos < 0 {
            return;
        }
        if pos as usize >= hist.len() {
            p.hist_pos = None;
            p.set_input(String::new());
        } else {
            p.hist_pos = Some(pos as usize);
            p.set_input(hist[pos as usize].clone());
        }
        self.refresh_candidates();
    }

    fn complete(&mut self) {
        let p = self.prompt.as_mut().unwrap();
        match p.action.completion() {
            Completion::Command | Completion::Buffer | Completion::OpenWith => {
                if let Some(c) = p.candidates.get(p.selected).cloned() {
                    if c == p.input && p.candidates.len() > 1 {
                        p.selected = (p.selected + 1) % p.candidates.len();
                        let n = p.candidates[p.selected].clone();
                        p.input = n;
                        p.cursor = p.input.chars().count();
                    } else {
                        p.set_input(c);
                    }
                }
            }
            Completion::Path => {
                let (gen, input, base) = (p.gen, p.input.clone(), p.base_dir.clone());
                p.completing_for = Some(input.clone());
                let fs = self.svc.fs.clone();
                let tx = self.tx.clone();
                self.spawn(tasks::complete_path(fs, tx, gen, input, base));
            }
            Completion::None => {}
        }
    }

    fn apply_completions(&mut self, gen: u64, items: Vec<String>) {
        let Some(p) = self.prompt.as_mut() else { return };
        if p.gen != gen {
            return; // stale
        }
        if items.is_empty() {
            self.message = Some(("[No match]".into(), true));
            return;
        }
        let pre = common_prefix(&items);
        if pre.chars().count() > p.input.chars().count() {
            p.set_input(pre);
        }
        p.candidates = if items.len() == 1 { Vec::new() } else { items };
        p.selected = 0;
    }

    fn push_history(&mut self, key: &str, value: &str) {
        if value.is_empty() || key == "confirm" {
            return;
        }
        let h = self.history.entry(key.to_string()).or_default();
        h.retain(|v| v != value);
        h.push(value.to_string());
        if h.len() > 100 {
            h.remove(0);
        }
        self.save_history();
    }

    fn submit_prompt(&mut self) {
        let Some(mut p) = self.prompt.take() else { return };
        // Completing prompts accept the selected candidate on inexact input.
        // (Empty input to kill-buffer means the current buffer.)
        let kill_default = p.action == PromptAction::KillBuffer && p.input.is_empty();
        if !kill_default
            && matches!(p.action.completion(), Completion::Command | Completion::Buffer | Completion::OpenWith)
        {
            let exact = match (p.action.completion(), &p.action) {
                (Completion::Command, _) => registry::lookup(&p.input).is_some(),
                // dired-0cz
                (Completion::OpenWith, PromptAction::OpenWith { path }) => {
                    self.open_with_entries(path).iter().any(|e| e.name == p.input)
                }
                _ => self.find_buffer(&p.input).is_some(),
            };
            if !exact {
                if let Some(c) = p.candidates.get(p.selected) {
                    p.input = c.clone();
                }
            }
        }
        // Act on the buffer the prompt belongs to, even if a background
        // event opened another buffer meanwhile.
        if self.buffer(p.origin).is_none() {
            return self.error("the buffer this prompt was for is gone");
        }
        self.pop_to(p.origin);
        self.push_history(p.action.history_key(), &p.input);
        let input = p.input.clone();
        let base = p.base_dir.clone();
        self.message = None;
        self.submit_action(p.action, input, base);
    }

    // ----- isearch -----

    pub(crate) fn search_lines(&mut self) -> Vec<String> {
        match &mut self.active_mut().kind {
            BufferKind::Dir(d) => {
                d.ensure_view();
                d.rows().iter().map(|r| d.display_name(r)).collect()
            }
            BufferKind::Text(t) => t.lines.iter().map(|l| l.text.clone()).collect(),
            BufferKind::Preview(p) => {
                let (lines, first) = p.lines();
                lines[first..].iter().map(|l| l.text.clone()).collect()
            }
        }
    }

    pub(crate) fn point(&mut self) -> usize {
        match &mut self.active_mut().kind {
            BufferKind::Dir(d) => {
                d.ensure_view();
                d.point
            }
            BufferKind::Text(t) => t.point,
            BufferKind::Preview(p) => p.point,
        }
    }

    pub(crate) fn buffer_len(&mut self) -> usize {
        match &mut self.active_mut().kind {
            BufferKind::Dir(d) => d.len(),
            BufferKind::Text(t) => t.lines.len(),
            BufferKind::Preview(p) => p.plan.items.len(),
        }
    }

    pub(crate) fn set_point(&mut self, i: usize) {
        let len = self.buffer_len();
        let i = i.min(len.saturating_sub(1));
        match &mut self.active_mut().kind {
            BufferKind::Dir(d) => d.set_point(i),
            BufferKind::Text(t) => {
                t.point = i;
                t.follow = i + 1 >= t.lines.len();
            }
            BufferKind::Preview(p) => p.point = i,
        }
    }

    fn isearch_from(&mut self, q: &str, start: usize, forward: bool, skip_current: bool) -> bool {
        if q.is_empty() {
            self.set_point(start);
            return true;
        }
        let lines = self.search_lines();
        let n = lines.len();
        if n == 0 {
            return false;
        }
        let ql = q.to_lowercase();
        let smart_case = q.chars().any(|c| c.is_uppercase());
        let hit = |s: &String| if smart_case { s.contains(q) } else { s.to_lowercase().contains(&ql) };
        for step in 0..n {
            let off = if skip_current { step + 1 } else { step };
            let i = if forward { (start + off) % n } else { (start + n * 2 - off % n) % n };
            if hit(&lines[i]) {
                self.set_point(i);
                self.message = None;
                return true;
            }
        }
        self.message = Some((format!("Failing I-search: {q}"), true));
        false
    }

    fn isearch_step(&mut self, forward: bool) {
        let Some(p) = self.prompt.as_mut() else { return };
        let PromptAction::Isearch { origin, .. } = p.action else { return };
        p.action = PromptAction::Isearch { forward, origin };
        p.label = if forward { "I-search: ".into() } else { "I-search backward: ".into() };
        if p.input.is_empty() {
            if let Some(last) = self.history.get("isearch").and_then(|h| h.last()).cloned() {
                self.prompt.as_mut().unwrap().set_input(last);
            }
        }
        let q = self.prompt.as_ref().unwrap().input.clone();
        let cur = self.point();
        self.isearch_from(&q, cur, forward, true);
    }

    // ----- events -----

    pub fn handle_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::TaskFinished => self.pending_tasks = self.pending_tasks.saturating_sub(1),
            AppEvent::ScanBatch { buf, dir, gen, entries } => {
                if let Some(d) = self.dir_of(buf) {
                    d.scan_batch(&dir, gen, entries);
                }
            }
            AppEvent::ScanDone { buf, dir, gen, error } => {
                if let Some(d) = self.dir_of(buf) {
                    d.scan_done(&dir, gen, error.clone());
                    if let Some(e) = error {
                        self.error(e);
                    }
                }
            }
            AppEvent::MetaBatch { buf, dir, gen, metas } => {
                if let Some(d) = self.dir_of(buf) {
                    d.meta_batch(&dir, gen, metas);
                }
            }
            AppEvent::MetaDone { buf, dir, gen } => {
                if let Some(d) = self.dir_of(buf) {
                    d.meta_done(&dir, gen);
                }
            }
            AppEvent::PlanReady { origin, plan, delete_facts, trash_supported } => match plan {
                Err(e) => self.error(e),
                Ok(plan) => self.open_preview(origin, plan, delete_facts, trash_supported),
            },
            AppEvent::OpProgress { buf, done, total } => {
                if let Some(BufferKind::Preview(p)) = self.buffer_mut(buf).map(|b| &mut b.kind) {
                    p.phase = PreviewPhase::Running { done, total };
                }
            }
            AppEvent::OpDone { buf, result } => self.op_done(buf, result),
            AppEvent::ProcLine { buf, line, stderr } => self.proc_line(buf, line, stderr),
            AppEvent::ProcExit { buf, code, error } => self.proc_exit(buf, code, error),
            AppEvent::Completions { gen, items } => self.apply_completions(gen, items),
            AppEvent::UndoDone { op_id, errors, touched } => self.undo_done(op_id, errors, touched),
            AppEvent::PathResolved { path, is_dir, visit, other } => self.path_resolved(path, is_dir, visit, other),
        }
    }

    fn path_resolved(&mut self, path: PathBuf, is_dir: Result<bool, String>, visit: bool, other: bool) {
        // dired-ta2: only directories go to the other window.
        if other && matches!(is_dir, Ok(true)) && !self.select_other_window() {
            return;
        }
        match is_dir {
            Err(e) if self.buffers.is_empty() => {
                // Startup path is unusable: show it as a buffer with the error.
                self.open_dir(&path);
                self.error(e);
            }
            Err(e) => self.error(e),
            Ok(true) => {
                self.open_dir(&path);
            }
            Ok(false) if visit => self.visit_file(&path, None),
            Ok(false) => {
                let parent = path.parent().unwrap_or(Path::new("/")).to_path_buf();
                let id = self.open_dir(&parent);
                if let (Some(d), Some(name)) = (self.dir_of(id), path.file_name()) {
                    d.goto_key(Path::new(name));
                }
            }
        }
    }

    pub(crate) fn open_preview(&mut self, origin: BufferId, plan: OpPlan, delete_facts: Option<Vec<dired_domain::SourceFact>>, trash_supported: bool) {
        let name = format!("*Preview: {}*", plan.kind.label());
        let first_conflict = plan.items.iter().position(|i| i.conflict.is_some()).unwrap_or(0);
        let pb = PreviewBuffer {
            plan,
            origin,
            point: first_conflict,
            top: 0,
            phase: PreviewPhase::Pending,
            cancel: Default::default(),
            delete_facts,
            trash_supported,
        };
        let id = self.add_buffer(&name, BufferKind::Preview(pb));
        if self.prompt.is_some() {
            // Don't yank focus from under an open prompt.
            let n = self.buffer(id).map(|b| b.name.clone()).unwrap_or_default();
            self.message(format!("Preview ready in {n} (b to switch)"));
            return;
        }
        // dired-ta2: pop the preview up below, keeping the source visible.
        self.pop_to(origin);
        match self.split_selected(crate::windows::SplitDir::Below, true) {
            Ok(w) => {
                self.select_window(w);
                self.focus(id);
                if let Some(win) = self.window_mut(w) {
                    win.popup = true;
                }
            }
            Err(_) => self.focus(id),
        }
    }

    pub(crate) fn text_buffer(&mut self, name: &str, role: TextRole, lines: Vec<TextLine>, focus: bool) -> BufferId {
        let id = match self.find_buffer(name) {
            Some(id) => {
                if let Some(BufferKind::Text(t)) = self.buffer_mut(id).map(|b| &mut b.kind) {
                    *t = TextBuffer::new(role, lines);
                }
                id
            }
            None => self.add_buffer(name, BufferKind::Text(TextBuffer::new(role, lines))),
        };
        if focus {
            self.focus(id);
        }
        id
    }

    pub(crate) fn set_diagnostics(&mut self, diags: Vec<Diagnostic>) {
        self.diagnostics = diags;
        if self.diagnostics.is_empty() {
            return;
        }
        let lines: Vec<TextLine> = std::iter::once(TextLine::new("Configuration diagnostics", LineStyle::Header))
            .chain(self.diagnostics.iter().map(|d| {
                TextLine::new(
                    d.to_string(),
                    match d.severity {
                        dired_config::Severity::Error => LineStyle::Error,
                        dired_config::Severity::Warning => LineStyle::Warning,
                    },
                )
            }))
            .collect();
        let had_active = !self.buffers.is_empty();
        let id = self.text_buffer("*Diagnostics*", TextRole::Diagnostics, lines, false);
        if !had_active {
            self.focus(id);
        }
        let n = self.diagnostics.len();
        self.error(format!("Configuration has {n} problem(s); see *Diagnostics* (C-x b)"));
        if let Some(dir) = &self.svc.state_dir {
            let text: String = self.diagnostics.iter().map(|d| format!("{d}\n")).collect();
            let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(dir.join("diagnostics.log"), text));
        }
    }

    // ----- persistence -----

    fn load_history(&mut self) {
        if !self.config.safety.persist_history {
            return;
        }
        if let Some(dir) = &self.svc.state_dir {
            if let Ok(text) = std::fs::read_to_string(dir.join("history.json")) {
                if let Ok(h) = serde_json::from_str(&text) {
                    self.history = h;
                }
            }
        }
    }

    fn save_history(&self) {
        if !self.config.safety.persist_history {
            return;
        }
        if let Some(dir) = &self.svc.state_dir {
            if let Ok(text) = serde_json::to_string(&self.history) {
                let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(dir.join("history.json"), text));
            }
        }
    }

    pub(crate) fn record_journal(&mut self, entry: JournalEntry) {
        if let Some(dir) = &self.svc.state_dir {
            if let Ok(line) = serde_json::to_string(&entry) {
                use std::io::Write;
                let _ = std::fs::create_dir_all(dir).and_then(|_| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(dir.join("journal.jsonl"))
                        .and_then(|mut f| writeln!(f, "{line}"))
                });
            }
        }
        self.journal.push(entry);
    }
}

fn adjust(top: &mut usize, point: usize, h: usize) {
    let h = h.max(1);
    if point < *top {
        *top = point;
    } else if point >= *top + h {
        *top = point + 1 - h;
    }
}
