// dired-7qs
//! Interactive command implementations.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use dired_config::{Key, KeyCode};
use dired_domain::journal::{undo_steps, JournalEntry};
use dired_domain::plan::{build_rename_plan, ItemOutcome, OpKind, OpPlan, OpResult, PlanItem, RenameRequest, Resolution, ResolutionKind};
use dired_domain::shell::{build_invocations, operand_arg, Invocation};
use dired_domain::{EntryState, SortKey};

use crate::app::{expand_path, App};
use crate::buffers::{BufferId, BufferKind, LineStyle, PreviewPhase, ProcState, Target, TextLine, TextRole};
use crate::dirbuf::Wdired;
use crate::keymap::Mode;
use crate::minibuffer::PromptAction;
use crate::registry;
use crate::services::{Effect, OpenOutcome, OpenRequest};
use crate::tasks;

impl App {
    pub fn run_command(&mut self, name: &str) {
        let Some(def) = registry::lookup(name) else {
            return self.error(format!("unknown command `{name}`"));
        };
        let in_dir = matches!(self.active().kind, BufferKind::Dir(_));
        let needs_dir = (def.name.starts_with("dired-") || def.name.starts_with("wdired-"))
            && !matches!(def.name, "dired-open-directory" | "dired-find-file" | "dired-undo" | "dired-reload-config" | "dired-other-window");
        if needs_dir && !in_dir {
            return self.error(format!("{} needs a directory buffer", def.name));
        }
        match def.name {
            "dired-open-directory" => {
                let init = format!("{}/", self.base_dir().display()).replace("//", "/");
                self.open_prompt("Dired (directory): ", init, PromptAction::OpenDirectory)
            }
            "dired-find-file" => {
                let init = format!("{}/", self.base_dir().display()).replace("//", "/");
                self.open_prompt("Find file: ", init, PromptAction::FindFile)
            }
            "wdired-toggle-edit" => self.wdired_enter(),
            "wdired-finish-edit" => self.wdired_finish(),
            "wdired-abort-changes" => self.wdired_abort(),
            "buffer-kill" => {
                let cur = self.active().name.clone();
                self.open_prompt(format!("Kill buffer (default {cur}): "), "", PromptAction::KillBuffer)
            }
            "next-line" => self.move_lines(1),
            "previous-line" => self.move_lines(-1),
            "beginning-of-buffer" => self.set_point(0),
            "end-of-buffer" => {
                let n = self.buffer_len();
                self.set_point(n.saturating_sub(1))
            }
            "scroll-up-command" => self.move_lines(self.page_lines() as i64),
            "scroll-down-command" => self.move_lines(-(self.page_lines() as i64)),
            "isearch-forward" | "isearch-backward" => {
                let forward = def.name == "isearch-forward";
                let origin = self.point();
                let label = if forward { "I-search: " } else { "I-search backward: " };
                self.open_prompt(label, "", PromptAction::Isearch { forward, origin })
            }
            "dired-goto-file" => self.open_prompt("Goto file: ", "", PromptAction::GotoFile),
            "dired-visit" => self.visit_at_point(),
            // dired-0cz
            "dired-open-with" => self.open_with_at_point(),
            // dired-vhh
            "dired-copy-name" => self.copy_operands(false),
            "dired-copy-path" => self.copy_operands(true),
            "dired-up-directory" => self.up_directory(),
            "revert-buffer" => {
                let id = self.active;
                self.revert(id);
                self.message("Reverting…");
            }
            "dired-mark" => self.mark_at_point(EntryState::Marked, 1),
            "dired-unmark" => self.mark_at_point(EntryState::Unmarked, 1),
            "dired-flag-file-deletion" => self.mark_at_point(EntryState::DeleteFlagged, 1),
            "dired-unmark-backward" => {
                self.move_lines(-1);
                self.mark_at_point(EntryState::Unmarked, 0)
            }
            "dired-unmark-all-marks" => {
                let d = self.dir().unwrap();
                let n = d.states.values().filter(|s| **s == EntryState::Marked).count();
                d.states.retain(|_, s| *s != EntryState::Marked);
                d.invalidate();
                self.message(format!("{n} mark(s) removed"));
            }
            "dired-toggle-marks" => {
                let d = self.dir().unwrap();
                for e in d.visible_entries() {
                    match d.state(&e.key) {
                        EntryState::Marked => d.set_state(&e.key, EntryState::Unmarked),
                        EntryState::Unmarked => d.set_state(&e.key, EntryState::Marked),
                        EntryState::DeleteFlagged => {}
                    }
                }
            }
            "dired-do-flagged-delete" | "dired-do-flagged-delete-permanently" => {
                let permanent = def.name.ends_with("permanently")
                    || (!self.svc.trash.supported() && self.config.safety.trash_fallback == "permanent");
                self.do_delete(permanent)
            }
            "dired-do-copy" => self.transfer_prompt(OpKind::Copy),
            "dired-do-rename" => self.transfer_prompt(OpKind::Move),
            "dired-create-directory" => {
                let init = format!("{}/", self.base_dir().display()).replace("//", "/");
                self.open_prompt("Create directory: ", init, PromptAction::Mkdir)
            }
            "dired-do-chmod" => {
                if self.capture_operands() {
                    let n = self.pending_operands.len();
                    self.open_prompt(format!("Change mode of {n} file(s) to (octal): "), "", PromptAction::Chmod)
                }
            }
            "dired-do-shell-command" | "dired-do-async-shell-command" | "dired-do-shell-command-in-shell" => {
                if !self.capture_operands() {
                    return;
                }
                let asynchronous = def.name == "dired-do-async-shell-command";
                let shell_mode = def.name == "dired-do-shell-command-in-shell";
                let n = self.pending_operands.len();
                let what = if n == 1 {
                    self.pending_operands[0].0.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
                } else {
                    format!("{n} files")
                };
                let label = match (shell_mode, asynchronous) {
                    (true, _) => format!("[SHELL MODE] command on {what}: "),
                    (false, true) => format!("& on {what}: "),
                    (false, false) => format!("! on {what}: "),
                };
                self.open_prompt(label, "", PromptAction::Shell { asynchronous, shell_mode })
            }
            "dired-maybe-insert-subdir" => self.insert_subdir(),
            "dired-hide-subdir" => self.hide_subdir(),
            "dired-mark-files-regexp" => self.open_prompt("Mark files (regexp): ", "", PromptAction::MarkRegexp(EntryState::Marked)),
            "dired-flag-files-regexp" => self.open_prompt("Flag for deletion (regexp): ", "", PromptAction::MarkRegexp(EntryState::DeleteFlagged)),
            "dired-unmark-files-regexp" => self.open_prompt("Unmark files (regexp): ", "", PromptAction::MarkRegexp(EntryState::Unmarked)),
            // dired-tlu
            "dired-mark-files-where" => {
                self.open_prompt("Mark files where: ", "", PromptAction::MarkWhere(EntryState::Marked))
            }
            "dired-flag-files-where" => {
                self.open_prompt("Flag for deletion where: ", "", PromptAction::MarkWhere(EntryState::DeleteFlagged))
            }
            "dired-unmark-files-where" => {
                self.open_prompt("Unmark files where: ", "", PromptAction::MarkWhere(EntryState::Unmarked))
            }
            "dired-do-rename-regexp" => {
                if self.capture_operands() {
                    self.open_prompt("Rename from (regexp): ", "", PromptAction::RenameRegexpFrom)
                }
            }
            "dired-search-files-content" => self.open_prompt("Search files (regexp): ", "", PromptAction::Search),
            "dired-sort-toggle-or-edit" => {
                let d = self.dir().unwrap();
                let mut s = d.sort;
                s.key = s.key.next();
                d.set_sort(s);
                let l = s.label();
                self.message(format!("Sorted by {l}"));
            }
            "dired-sort-by-name" | "dired-sort-by-natural" | "dired-sort-by-time" | "dired-sort-by-size" | "dired-sort-by-extension" => {
                let key = SortKey::parse(def.name.trim_start_matches("dired-sort-by-")).unwrap();
                let d = self.dir().unwrap();
                let mut s = d.sort;
                s.key = key;
                d.set_sort(s);
            }
            "dired-sort-reverse" => {
                let d = self.dir().unwrap();
                let mut s = d.sort;
                s.reverse = !s.reverse;
                d.set_sort(s);
            }
            "dired-sort-toggle-dirs-first" => {
                let d = self.dir().unwrap();
                let mut s = d.sort;
                s.dirs_first = !s.dirs_first;
                d.set_sort(s);
            }
            "dired-toggle-hidden" => {
                let d = self.dir().unwrap();
                d.show_hidden = !d.show_hidden;
                d.invalidate();
                let m = if d.show_hidden { "Showing hidden files" } else { "Hiding hidden files (marks kept)" };
                self.message(m);
            }
            "dired-filter" => {
                let cur = self.dir().unwrap().filter.clone().unwrap_or_default();
                self.open_prompt("Filter (substring, empty clears): ", cur, PromptAction::Filter)
            }
            "dired-undo" => self.undo(),
            "switch-to-buffer" => self.open_prompt("Switch to buffer: ", "", PromptAction::SwitchBuffer),
            "help-for-help" => self.show_help(),
            "execute-extended-command" => self.open_prompt("M-x ", "", PromptAction::ExtendedCommand),
            "keyboard-quit" => self.keyboard_quit(),
            "quit-window" => {
                let id = self.active;
                let others = self.buffers.iter().filter(|b| b.id != id && b.is_user_buffer()).count();
                if others == 0 {
                    self.quit = true;
                } else {
                    let id = self.active;
                    let origin = match &self.active().kind {
                        BufferKind::Preview(p) => Some(p.origin),
                        _ => None,
                    };
                    self.remove_buffer(id);
                    if let Some(o) = origin {
                        self.pop_to(o);
                    }
                }
            }
            "save-buffers-kill-terminal" => self.quit = true,
            "dired-reload-config" => self.reload_config(),
            // dired-ta2
            "split-window-below" | "split-window-right" => {
                let dir = if def.name.ends_with("below") {
                    crate::windows::SplitDir::Below
                } else {
                    crate::windows::SplitDir::Right
                };
                if let Err(e) = self.split_selected(dir, false) {
                    self.error(e);
                }
            }
            "other-window" => match self.next_window() {
                Some(w) => self.select_window(w),
                None => self.message("No other window"),
            },
            "delete-window" => {
                if self.windows.len() <= 1 {
                    self.error("Attempt to delete the sole window");
                } else {
                    let w = self.selected;
                    self.delete_window_raw(w);
                }
            }
            "delete-other-windows" => self.delete_other_windows(),
            "dired-other-window" => {
                let init = format!("{}/", self.base_dir().display()).replace("//", "/");
                self.open_prompt("Dired in other window (directory): ", init, PromptAction::OpenDirectoryOther)
            }
            "find-file-other-window" => {
                let init = format!("{}/", self.base_dir().display()).replace("//", "/");
                self.open_prompt("Find file in other window: ", init, PromptAction::FindFileOther)
            }
            "switch-to-buffer-other-window" => {
                self.open_prompt("Switch to buffer in other window: ", "", PromptAction::SwitchBufferOther)
            }
            "dired-find-file-other-window" => {
                let d = self.dir().unwrap();
                let Some(e) = d.point_entry() else { return };
                let abs = d.abs(&e.key);
                if e.special || e.is_dir_like() {
                    if self.select_other_window() {
                        self.open_dir(abs);
                    }
                } else {
                    self.visit_file(&abs, None);
                }
            }
            // dired-7yu
            // dired-2hn
            "load-theme" => {
                let cur = self.config.theme.name.clone().unwrap_or_else(|| "default".into());
                self.open_prompt("Load theme: ", "", PromptAction::LoadTheme);
                // Start on the active theme so RET without typing keeps it.
                if let (Some(i), Some(p)) =
                    (dired_config::THEMES.iter().position(|t| *t == cur), self.prompt.as_mut())
                {
                    p.selected = i;
                }
            }
            // dired-5xb
            "load-style" => {
                let cur = self.config.display.style.clone();
                self.open_prompt("Load style: ", "", PromptAction::LoadStyle);
                // Start on the active style so RET without typing keeps it.
                if let (Some(i), Some(p)) =
                    (dired_config::STYLES.iter().position(|s| *s == cur), self.prompt.as_mut())
                {
                    p.selected = i;
                }
            }
            // dired-p2d
            "cycle-style" => {
                let cur = self.config.display.style.as_str();
                let i = dired_config::STYLES.iter().position(|s| *s == cur).map(|i| i + 1).unwrap_or(0);
                let next = dired_config::STYLES[i % dired_config::STYLES.len()];
                self.config.display.style = next.to_string();
                self.config_gen += 1;
                let note = if self.config.display.ui.is_empty() { "" } else { " (with overrides)" };
                self.message(format!("Style: {next}{note}"));
            }
            "cycle-theme" => {
                let themes = dired_config::THEMES;
                let cur = self.config.theme.name.as_deref().unwrap_or("default");
                let i = themes.iter().position(|t| *t == cur).map(|i| i + 1).unwrap_or(0);
                let next = themes[i % themes.len()];
                self.config.theme.name = Some(next.to_string());
                self.config_gen += 1;
                self.message(format!("Theme: {next}"));
            }
            "view-diagnostics" => match self.find_buffer("*Diagnostics*") {
                Some(id) => self.focus(id),
                None => self.message("No diagnostics"),
            },
            "view-messages" => {
                let lines = self.messages.iter().map(|m| TextLine::normal(m.clone())).collect();
                self.text_buffer("*Messages*", TextRole::Messages, lines, true);
            }
            "operation-execute" => self.execute_preview(),
            "conflict-overwrite" => self.resolve_at_point(Resolution::Overwrite),
            "conflict-skip" => self.resolve_at_point(Resolution::Skip),
            "conflict-ask" => self.resolve_at_point(Resolution::Ask),
            "conflict-rename" => {
                let id = self.active;
                if let BufferKind::Preview(p) = &self.active().kind {
                    if let Some(item) = p.plan.items.get(p.point) {
                        let init = item.effective_dst().map(|d| d.display().to_string()).unwrap_or_default();
                        let idx = p.point;
                        self.open_prompt("Rename target to: ", init, PromptAction::ConflictRename { buf: id, item: idx });
                    }
                }
            }
            "conflict-overwrite-all" => self.resolve_all(ResolutionKind::Overwrite),
            "conflict-skip-all" => self.resolve_all(ResolutionKind::Skip),
            "operation-toggle-permanent" => self.toggle_permanent(),
            "text-visit" => self.text_visit(),
            "process-kill" => {
                if let BufferKind::Text(t) = &self.active().kind {
                    if let Some(k) = t.proc.as_ref().filter(|p| p.running).and_then(|p| p.kill.clone()) {
                        k.kill();
                        self.message("Killing process…");
                        return;
                    }
                }
                self.message("No running process");
            }
            other => self.error(format!("{other} is not available here")),
        }
    }

    pub(crate) fn submit_action(&mut self, action: PromptAction, input: String, base: PathBuf) {
        match action {
            PromptAction::OpenDirectory => {
                self.open_dir(expand_path(&input, &base));
            }
            PromptAction::FindFile => self.open_path(expand_path(&input, &base), true),
            // dired-5xb
            PromptAction::LoadStyle => {
                if dired_config::STYLES.contains(&input.as_str()) {
                    self.config.display.style = input.clone();
                    self.config_gen += 1;
                    let note = if self.config.display.ui.is_empty() { "" } else { " (with overrides)" };
                    self.message(format!("Style: {input}{note}"));
                } else {
                    self.error(format!("No such style `{input}` ({})", dired_config::STYLES.join(", ")));
                }
            }
            // dired-2hn
            PromptAction::LoadTheme => {
                if dired_config::THEMES.contains(&input.as_str()) {
                    self.config.theme.name = Some(input.clone());
                    self.config_gen += 1;
                    self.message(format!("Theme: {input}"));
                } else {
                    self.error(format!("No such theme `{input}` ({})", dired_config::THEMES.join(", ")));
                }
            }
            // dired-0cz
            PromptAction::OpenWith { paths } => {
                match self.open_with_entries(&paths).into_iter().find(|e| e.name == input) {
                    Some(entry) => self.open_files_with(&entry, &paths),
                    None => self.error(format!("No such open-with candidate `{input}`")),
                }
            }
            // dired-ta2
            PromptAction::OpenDirectoryOther => {
                if self.select_other_window() {
                    self.open_dir(expand_path(&input, &base));
                }
            }
            PromptAction::FindFileOther => self.open_path_in(expand_path(&input, &base), true, true),
            PromptAction::SwitchBufferOther => match self.find_buffer(&input) {
                Some(id) => {
                    if self.select_other_window() {
                        self.focus(id);
                    }
                }
                None => self.error(format!("no buffer named `{input}`")),
            },
            PromptAction::Copy | PromptAction::Move => {
                let kind = if action == PromptAction::Copy { OpKind::Copy } else { OpKind::Move };
                let dest = expand_path(&input, &base);
                let sources = std::mem::take(&mut self.pending_operands);
                let (fs, tx, origin, id) = (self.svc.fs.clone(), self.tx.clone(), self.active, self.op_id());
                self.spawn(tasks::plan_transfer(fs, tx, origin, id, kind, sources, dest));
                self.message("Planning…");
            }
            PromptAction::Mkdir => {
                let path = expand_path(&input, &base);
                let mut plan = OpPlan::new(self.op_id(), OpKind::Mkdir);
                plan.items.push(PlanItem::new(path, None));
                self.start_execution(self.active, plan);
            }
            PromptAction::Chmod => match u32::from_str_radix(input.trim(), 8) {
                Ok(mode) if mode <= 0o7777 => {
                    let sources = std::mem::take(&mut self.pending_operands);
                    let (fs, tx, origin, id) = (self.svc.fs.clone(), self.tx.clone(), self.active, self.op_id());
                    self.spawn(tasks::plan_chmod(fs, tx, origin, id, mode, sources));
                }
                _ => self.error(format!("invalid octal mode `{input}`")),
            },
            PromptAction::Shell { asynchronous, shell_mode } => {
                if input.trim().is_empty() {
                    return self.error("empty command");
                }
                if shell_mode {
                    let n = self.pending_operands.len();
                    let sh = self.config.shell.shell.clone();
                    self.open_prompt(
                        format!("Run through SHELL ({sh} -c) on {n} file(s)? Files are passed as \"$@\". Type yes: "),
                        "",
                        PromptAction::ConfirmShell { template: input, asynchronous },
                    );
                } else {
                    self.run_shell(&input, asynchronous, false);
                }
            }
            PromptAction::ConfirmShell { template, asynchronous } => {
                if input.trim() == "yes" {
                    self.run_shell(&template, asynchronous, true);
                } else {
                    self.message("Cancelled");
                }
            }
            PromptAction::MarkRegexp(state) => self.mark_regexp(&input, state),
            PromptAction::MarkWhere(state) => self.mark_where(&input, state), // dired-tlu
            PromptAction::RenameRegexpFrom => match regex::Regex::new(&input) {
                Ok(_) => self.open_prompt(format!("Rename {input} to: "), "", PromptAction::RenameRegexpTo(input)),
                Err(e) => self.error(format!("invalid regexp: {e}")),
            },
            PromptAction::RenameRegexpTo(re) => self.rename_regexp(&re, &input),
            PromptAction::Search => self.run_search(&input),
            PromptAction::GotoFile => self.goto_file(&input),
            PromptAction::Filter => {
                let d = self.dir().unwrap();
                d.filter = if input.is_empty() { None } else { Some(input) };
                d.invalidate();
            }
            PromptAction::SwitchBuffer => match self.find_buffer(&input) {
                Some(id) => self.focus(id),
                None => self.error(format!("no buffer named `{input}`")),
            },
            PromptAction::KillBuffer => {
                let id = if input.is_empty() { Some(self.active) } else { self.find_buffer(&input) };
                match id {
                    None => self.error(format!("no buffer named `{input}`")),
                    Some(id) => {
                        let b = self.buffer(id).unwrap();
                        if b.has_unfinished_work() {
                            let what = match &b.kind {
                                BufferKind::Dir(_) => "uncommitted Wdired edits",
                                BufferKind::Preview(_) => "an unfinished operation",
                                BufferKind::Text(_) => "a running process",
                            };
                            let name = b.name.clone();
                            self.open_prompt(format!("{name} has {what}; kill anyway? (y or n) "), "", PromptAction::ConfirmKill(id));
                        } else {
                            self.remove_buffer(id);
                        }
                    }
                }
            }
            PromptAction::ConfirmKill(id) => self.remove_buffer(id),
            PromptAction::ExtendedCommand => match registry::lookup(input.trim()) {
                Some(def) => self.run_command(def.name),
                None => self.error(format!("[No match] {input}")),
            },
            PromptAction::ConfirmExecute(buf) => {
                if input.trim() == "yes" {
                    self.begin_preview_execution(buf);
                } else {
                    self.message("Not confirmed; nothing was changed");
                }
            }
            PromptAction::ConflictRename { buf, item } => {
                let base = match self.buffer(buf).map(|b| &b.kind) {
                    Some(BufferKind::Preview(p)) => p.plan.items.get(item).and_then(|i| i.dst.as_ref()).and_then(|d| d.parent()).map(Path::to_path_buf),
                    _ => None,
                };
                let target = expand_path(&input, &base.unwrap_or(base_dir_fallback()));
                if let Some(BufferKind::Preview(p)) = self.buffer_mut(buf).map(|b| &mut b.kind) {
                    if let Err(e) = p.plan.set_resolution(item, Resolution::RenameTo(target)) {
                        self.error(e);
                    }
                }
            }
            PromptAction::Isearch { .. } => {}
        }
    }

    // ----- movement & marks -----

    pub(crate) fn move_lines(&mut self, delta: i64) {
        let p = self.point() as i64;
        let n = self.buffer_len() as i64;
        let t = (p + delta).clamp(0, (n - 1).max(0));
        self.set_point(t as usize);
    }

    fn mark_at_point(&mut self, state: EntryState, advance: i64) {
        let d = self.dir().unwrap();
        match d.point_entry() {
            Some(e) if !e.special => {
                d.set_state(&e.key, state);
                if state == EntryState::Unmarked {
                    d.annotations.remove(&e.key);
                }
            }
            _ => {}
        }
        if advance != 0 {
            self.move_lines(advance);
        }
    }

    fn mark_regexp(&mut self, pattern: &str, state: EntryState) {
        let re = match regex::Regex::new(pattern) {
            Ok(r) => r,
            Err(e) => return self.error(format!("invalid regexp: {e}")),
        };
        let d = self.dir().unwrap();
        let mut n = 0;
        for e in d.visible_entries() {
            if re.is_match(&e.name) {
                d.set_state(&e.key, state);
                n += 1;
            }
        }
        let verb = match state {
            EntryState::Marked => "marked",
            EntryState::DeleteFlagged => "flagged for deletion",
            EntryState::Unmarked => "unmarked",
        };
        self.message(format!("{n} matching file(s) {verb}"));
    }

    // dired-tlu
    fn mark_where(&mut self, expr: &str, state: EntryState) {
        let pred = match dired_domain::Predicate::parse(expr) {
            Ok(p) => p,
            Err(e) => return self.error(e),
        };
        let now = self.svc.clock.now();
        let d = self.dir().unwrap();
        let (mut n, mut loading) = (0, 0);
        for e in d.visible_entries() {
            match pred.matches(&e, now) {
                Some(true) => {
                    d.set_state(&e.key, state);
                    n += 1;
                }
                Some(false) => {}
                None => loading += 1,
            }
        }
        let verb = match state {
            EntryState::Marked => "marked",
            EntryState::DeleteFlagged => "flagged for deletion",
            EntryState::Unmarked => "unmarked",
        };
        let pending = if loading > 0 { format!(" ({loading} still loading)") } else { String::new() };
        self.message(format!("{n} matching file(s) {verb}{pending}"));
    }

    fn goto_file(&mut self, q: &str) {
        let d = self.dir().unwrap();
        d.ensure_view();
        let n = d.rows().len();
        let ql = q.to_lowercase();
        let start = d.point;
        for step in 1..=n {
            let i = (start + step) % n;
            let r = d.rows()[i].clone();
            if d.entry(&r).name.to_lowercase().contains(&ql) {
                d.set_point(i);
                return;
            }
        }
        self.error(format!("No file matching `{q}`"));
    }

    /// Record operands at prompt time; false (with message) when there are none.
    pub(crate) fn capture_operands(&mut self) -> bool {
        let d = self.dir().unwrap();
        let ops: Vec<(PathBuf, Option<PathBuf>)> = d
            .operands()
            .into_iter()
            .map(|e| (d.abs(&e.key), Some(e.key)))
            .collect();
        if ops.is_empty() {
            self.error("No file on this line");
            return false;
        }
        self.pending_operands = ops;
        true
    }

    fn transfer_prompt(&mut self, kind: OpKind) {
        if !self.capture_operands() {
            return;
        }
        let n = self.pending_operands.len();
        let verb = if kind == OpKind::Copy { "Copy" } else { "Rename/move" };
        let what = if n == 1 {
            self.pending_operands[0].0.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            format!("{n} files")
        };
        // dired-ta2: default to the directory in another window (dwim target).
        let target = if self.config.display.dwim_target { self.dwim_target() } else { None };
        let init = format!("{}/", target.unwrap_or_else(|| self.base_dir()).display()).replace("//", "/");
        let action = if kind == OpKind::Copy { PromptAction::Copy } else { PromptAction::Move };
        self.open_prompt(format!("{verb} {what} to: "), init, action);
    }

    fn do_delete(&mut self, permanent: bool) {
        let d = self.dir().unwrap();
        let flagged: Vec<(PathBuf, Option<PathBuf>)> =
            d.flagged().into_iter().map(|e| (d.abs(&e.key), Some(e.key))).collect();
        if flagged.is_empty() {
            return self.message("(No deletions requested)");
        }
        let (fs, trash, tx, origin, id) = (self.svc.fs.clone(), self.svc.trash.clone(), self.tx.clone(), self.active, self.op_id());
        self.spawn(tasks::plan_delete(fs, trash, tx, origin, id, permanent, flagged));
    }

    // dired-ta2
    /// Select the other window (splitting if needed); false with a message
    /// when the screen is too small.
    pub(crate) fn select_other_window(&mut self) -> bool {
        match self.ensure_other_window() {
            Ok(w) => {
                self.select_window(w);
                true
            }
            Err(e) => {
                self.error(e);
                false
            }
        }
    }

    // ----- visiting -----

    // dired-p2d
    /// Lines a page command moves: the visible rows minus one, so a line
    /// stays in view. Directory buffers lose their header rows from the view.
    fn page_lines(&self) -> usize {
        let hdr = if matches!(self.active().kind, BufferKind::Dir(_)) { self.dir_header_rows } else { 1 };
        self.viewport.saturating_sub(hdr + 1).max(1)
    }

    fn visit_at_point(&mut self) {
        let d = self.dir().unwrap();
        let Some(e) = d.point_entry() else { return };
        let abs = d.abs(&e.key);
        if e.special {
            return self.up_directory();
        }
        if e.is_dir_like() {
            self.open_dir(abs);
        } else {
            self.visit_file(&abs, None);
        }
    }

    // dired-0cz
    /// Candidates for `paths`: configured entries, then built-ins; entries whose
    /// `extensions` don't cover every path are hidden; the first of a
    /// duplicated name wins.
    pub(crate) fn open_with_entries(&self, paths: &[PathBuf]) -> Vec<dired_config::OpenWithEntry> {
        let exts: Vec<String> = paths
            .iter()
            .map(|p| p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default())
            .collect();
        let mut out: Vec<dired_config::OpenWithEntry> = Vec::new();
        for e in self.config.open_with.iter().cloned().chain(crate::services::builtin_open_with()) {
            let applies = e.extensions.is_empty()
                || exts.iter().all(|ext| e.extensions.iter().any(|x| x.to_lowercase() == *ext));
            if applies && !out.iter().any(|o| o.name == e.name) {
                out.push(e);
            }
        }
        out
    }

    // dired-vhh
    /// Copy the names (or full paths) of the marked entries, else the entry at
    /// point, one per line.
    fn copy_operands(&mut self, full_path: bool) {
        let d = self.dir().unwrap();
        let paths: Vec<PathBuf> = d.operands().into_iter().map(|e| d.abs(&e.key)).collect();
        if paths.is_empty() {
            return self.error("No file on this line");
        }
        let items: Vec<String> = paths
            .iter()
            .map(|p| {
                if full_path {
                    p.to_string_lossy().into_owned()
                } else {
                    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                }
            })
            .collect();
        let msg = if items.len() == 1 {
            format!("Copied {}", items[0])
        } else {
            format!("Copied {} {}", items.len(), if full_path { "paths" } else { "names" })
        };
        self.effects.push(Effect::Clipboard(items.join("\n")));
        self.message(msg);
    }

    // dired-0cz
    fn open_with_at_point(&mut self) {
        // dired-vmx: the marked entries, else the entry at point
        let d = self.dir().unwrap();
        let ops = d.operands();
        let paths: Vec<PathBuf> = ops.iter().map(|e| d.abs(&e.key)).collect();
        let label = match ops.as_slice() {
            [] => return self.message("Nothing to open on this line"),
            [e] => format!("Open {} with: ", e.name),
            many => format!("Open {} files with: ", many.len()),
        };
        self.open_prompt(label, "", PromptAction::OpenWith { paths });
    }

    // dired-0cz
    fn open_files_with(&mut self, entry: &dired_config::OpenWithEntry, paths: &[PathBuf]) {
        // dired-vmx
        let mut detached = 0;
        let mut errors = Vec::new();
        let mut how = String::new();
        for r in self.svc.opener.open_with(entry, paths) {
            match r {
                Ok(OpenOutcome::Detached(h)) => {
                    detached += 1;
                    how = h;
                }
                Ok(OpenOutcome::Foreground(spec)) => self.effects.push(Effect::Foreground(spec)),
                Err(e) => errors.push(e),
            }
        }
        if let Some(first) = errors.first() {
            let more = if errors.len() > 1 { format!(" (+{} more failed)", errors.len() - 1) } else { String::new() };
            return self.error(format!("{first}{more}"));
        }
        match (detached, paths) {
            (0, _) => {}
            (1, [p]) => self.message(format!("Opened {} with {how}", p.display())),
            _ => self.message(format!("Opened {} files with {}", paths.len(), entry.name)),
        }
    }

    pub(crate) fn visit_file(&mut self, path: &Path, line: Option<u64>) {
        match self.svc.opener.open(&OpenRequest { path: path.to_path_buf(), line }) {
            Ok(OpenOutcome::Detached(how)) => self.message(format!("Opened {} with {how}", path.display())),
            Ok(OpenOutcome::Foreground(spec)) => self.effects.push(Effect::Foreground(spec)),
            Err(e) => self.error(e),
        }
    }

    fn up_directory(&mut self) {
        let root = self.dir().unwrap().root.clone();
        let Some(parent) = root.parent().map(Path::to_path_buf) else {
            return self.message("Already at the filesystem root");
        };
        let id = self.open_dir(&parent);
        if let (Some(d), Some(name)) = (self.dir_of(id), root.file_name()) {
            d.goto_key(Path::new(name));
        }
    }

    fn text_visit(&mut self) {
        let target = match &self.active().kind {
            BufferKind::Text(t) => t.targets.get(t.point).cloned().flatten(),
            _ => None,
        };
        match target {
            Some(Target { path, line }) => self.visit_file(&path, line),
            None => self.message("Nothing to visit on this line"),
        }
    }

    // ----- inline subdirectories -----

    fn insert_subdir(&mut self) {
        let follow = self.config.safety.follow_symlinks_on_insert;
        let id = self.active;
        let d = self.dir().unwrap();
        let Some(row) = d.point_row() else { return };
        let e = d.entry(&row).clone();
        if e.special || !e.is_dir_like() {
            return self.error("Not a directory");
        }
        if d.listings.contains_key(&e.key) {
            return self.message(format!("{} is already inserted", e.name));
        }
        if e.kind == dired_domain::FileKind::Symlink {
            if !follow {
                return self.error(format!(
                    "Not inserting directory symlink {} (set safety.follow_symlinks_on_insert = true)",
                    e.name
                ));
            }
            // Refuse expansions that would loop back into an ancestor.
            let abs = d.abs(&e.key);
            let target = e.meta.as_ref().and_then(|m| m.symlink_target.clone()).unwrap_or_default();
            let resolved = crate::app::normalize(&abs.parent().unwrap_or(Path::new("/")).join(target));
            if abs.starts_with(&resolved) || row.depth >= 32 {
                return self.error(format!("Refusing cyclic expansion of {}", e.name));
            }
        }
        self.scan_listing(id, e.key);
    }

    fn hide_subdir(&mut self) {
        let d = self.dir().unwrap();
        let Some(row) = d.point_row() else { return };
        let e = d.entry(&row).clone();
        if !e.special && d.listings.contains_key(&e.key) {
            d.collapse(&e.key);
        } else if !row.dir.as_os_str().is_empty() {
            let header = row.dir.clone();
            d.collapse(&header);
            d.goto_key(&header);
        } else {
            self.error("Not in an inserted subdirectory");
        }
    }

    // ----- wdired -----

    fn wdired_enter(&mut self) {
        let d = self.dir().unwrap();
        if d.wdired.is_some() {
            return self.wdired_finish();
        }
        d.wdired = Some(Wdired { edits: HashMap::new(), col: usize::MAX });
        d.invalidate();
        self.message("Editing file names: C-c C-c to commit, C-c C-k to abort");
    }

    fn wdired_abort(&mut self) {
        let d = self.dir().unwrap();
        d.wdired = None;
        d.invalidate();
        self.message("Changes aborted");
    }

    fn wdired_finish(&mut self) {
        let origin = self.active;
        let d = self.dir().unwrap();
        let Some(w) = &d.wdired else { return };
        let mut requests = Vec::new();
        let mut dirs: HashSet<PathBuf> = HashSet::new();
        for l in d.listings.values() {
            for e in &l.entries {
                if let Some(new) = w.edits.get(&e.key) {
                    if *new != e.name {
                        let src = d.abs(&e.key);
                        dirs.insert(src.parent().unwrap_or(Path::new("/")).to_path_buf());
                        requests.push(RenameRequest { src, key: Some(e.key.clone()), new_name: new.clone(), is_dir: e.is_dir() });
                    }
                }
            }
        }
        if requests.is_empty() {
            d.wdired = None;
            d.invalidate();
            return self.message("(No changes need to be saved)");
        }
        let existing: HashMap<PathBuf, HashSet<OsString>> =
            dirs.into_iter().filter_map(|p| d.names_in(&p).map(|n| (p, n))).collect();
        requests.sort_by(|a, b| a.src.cmp(&b.src));
        let id = self.op_id();
        let plan = build_rename_plan(id, requests, &existing);
        let problems = plan.errors.len();
        self.open_preview(origin, plan, None, true);
        if problems > 0 {
            self.error(format!("Wdired: {problems} problem(s); no files were changed — fix the names (still editing)"));
        }
    }

    /// Text editing within the filename span. Returns true when handled.
    pub(crate) fn wdired_key(&mut self, key: Key) -> bool {
        let d = self.dir().unwrap();
        let Some(row) = d.point_row() else { return false };
        let e = d.entry(&row).clone();
        let w = d.wdired.as_mut().unwrap();
        let mut name: Vec<char> = w.edits.get(&e.key).cloned().unwrap_or_else(|| e.name.clone()).chars().collect();
        let col = w.col.min(name.len());
        let editing = |c: &Key| {
            c.printable().is_some()
                || matches!(
                    (c.code, c.ctrl, c.meta),
                    (KeyCode::Backspace, false, false) | (KeyCode::Delete, false, false) | (KeyCode::Char('d'), true, false) | (KeyCode::Char('k'), true, false)
                )
        };
        if editing(&key) && !e.name_is_editable() {
            self.error("Read-only: this name cannot be edited as text");
            return true;
        }
        let new_col = match (key.code, key.ctrl, key.meta) {
            _ if key.printable().is_some() => {
                name.insert(col, key.printable().unwrap());
                col + 1
            }
            (KeyCode::Backspace, false, false) => {
                if col == 0 {
                    return true;
                }
                name.remove(col - 1);
                col - 1
            }
            (KeyCode::Delete, false, false) | (KeyCode::Char('d'), true, false) => {
                if col < name.len() {
                    name.remove(col);
                }
                col
            }
            (KeyCode::Char('k'), true, false) => {
                name.truncate(col);
                col
            }
            (KeyCode::Char('a'), true, false) | (KeyCode::Home, false, false) => 0,
            (KeyCode::Char('e'), true, false) | (KeyCode::End, false, false) => name.len(),
            (KeyCode::Char('f'), true, false) | (KeyCode::Right, false, false) => (col + 1).min(name.len()),
            (KeyCode::Char('b'), true, false) | (KeyCode::Left, false, false) => col.saturating_sub(1),
            _ => return false,
        };
        let new: String = name.into_iter().collect();
        if e.name_is_editable() {
            if new == e.name {
                w.edits.remove(&e.key);
            } else {
                w.edits.insert(e.key.clone(), new);
            }
        }
        w.col = new_col;
        d.invalidate();
        true
    }

    fn rename_regexp(&mut self, re: &str, replacement: &str) {
        let re = match regex::Regex::new(re) {
            Ok(r) => r,
            Err(e) => return self.error(format!("invalid regexp: {e}")),
        };
        let origin = self.active;
        let ops = std::mem::take(&mut self.pending_operands);
        let d = self.dir().unwrap();
        let mut requests = Vec::new();
        let mut dirs = HashSet::new();
        for (src, key) in ops {
            let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if re.is_match(&name) {
                let new_name = re.replace(&name, replacement).into_owned();
                dirs.insert(src.parent().unwrap_or(Path::new("/")).to_path_buf());
                let is_dir = key.as_ref().map(|k| d.has_kind(k, dired_domain::FileKind::Dir)).unwrap_or(false);
                requests.push(RenameRequest { src, key, new_name, is_dir });
            }
        }
        if requests.is_empty() {
            return self.error("No marked file names match");
        }
        let existing: HashMap<PathBuf, HashSet<OsString>> =
            dirs.into_iter().filter_map(|p: PathBuf| d.names_in(&p).map(|n| (p, n))).collect();
        let id = self.op_id();
        let plan = build_rename_plan(id, requests, &existing);
        self.open_preview(origin, plan, None, true);
    }

    // ----- preview & execution -----

    fn resolve_at_point(&mut self, r: Resolution) {
        let res = match &mut self.active_mut().kind {
            BufferKind::Preview(p) if matches!(p.phase, PreviewPhase::Pending) => {
                let i = p.point;
                p.plan.set_resolution(i, r).map(|_| i)
            }
            _ => Err("no pending operation here".into()),
        };
        match res {
            Ok(_) => self.move_lines(1),
            Err(e) => self.error(e),
        }
    }

    fn resolve_all(&mut self, kind: ResolutionKind) {
        let n = match &mut self.active_mut().kind {
            BufferKind::Preview(p) if matches!(p.phase, PreviewPhase::Pending) => p.plan.resolve_all(kind),
            _ => return self.error("no pending operation here"),
        };
        self.message(format!("{n} conflict(s) resolved"));
    }

    fn toggle_permanent(&mut self) {
        let msg = match &mut self.active_mut().kind {
            BufferKind::Preview(p) if matches!(p.phase, PreviewPhase::Pending) => match &p.delete_facts {
                Some(facts) => {
                    let permanent = p.plan.kind != OpKind::DeletePermanent;
                    let kind = if permanent { OpKind::DeletePermanent } else { OpKind::Trash };
                    let backend = if permanent { "permanent unlink (irreversible)".to_string() } else { "trash".to_string() };
                    let backend = if !permanent && !p.plan.backend.starts_with("permanent") { p.plan.backend.clone() } else { backend };
                    p.plan = dired_domain::plan::build_delete_plan(p.plan.id, kind, facts.clone(), p.trash_supported, &backend);
                    Ok(if permanent { "PERMANENT deletion selected" } else { "Trash selected" })
                }
                None => Err("not a deletion preview"),
            },
            _ => Err("no pending operation here"),
        };
        match msg {
            Ok(m) => self.message(m),
            Err(e) => self.error(e),
        }
    }

    fn execute_preview(&mut self) {
        let buf = self.active;
        let BufferKind::Preview(p) = &self.active().kind else {
            return self.error("not an operation preview");
        };
        if !matches!(p.phase, PreviewPhase::Pending) {
            return self.error("operation already started");
        }
        if let Err(e) = p.plan.can_execute() {
            return self.error(e);
        }
        let plan = &p.plan;
        let n = plan.active_items();
        if plan.kind == OpKind::DeletePermanent {
            let dirs = plan.items.iter().filter(|i| i.is_dir && !i.skipped()).count();
            self.open_prompt(
                format!("PERMANENTLY delete {n} item(s) ({dirs} directories recursively)? This cannot be undone. Type yes: "),
                "",
                PromptAction::ConfirmExecute(buf),
            );
        } else if plan.overwrite_count() > 0 {
            let k = plan.overwrite_count();
            let dirs = plan
                .items
                .iter()
                .filter(|i| i.resolution == Resolution::Overwrite && i.is_dir)
                .count();
            let scope = if dirs > 0 { format!(", {dirs} directory(ies) replaced recursively") } else { String::new() };
            self.open_prompt(
                format!("Overwrite {k} existing destination(s) (of {n} item(s){scope})? Type yes: "),
                "",
                PromptAction::ConfirmExecute(buf),
            );
        } else {
            self.begin_preview_execution(buf);
        }
    }

    fn begin_preview_execution(&mut self, buf: BufferId) {
        let Some(BufferKind::Preview(p)) = self.buffer_mut(buf).map(|b| &mut b.kind) else { return };
        let total = p.plan.items.len();
        p.phase = PreviewPhase::Running { done: 0, total };
        let plan = p.plan.clone();
        let origin = p.origin;
        let cancel = p.cancel.clone();
        self.inflight.insert(plan.id, (plan.clone(), origin));
        let (fs, trash, tx) = (self.svc.fs.clone(), self.svc.trash.clone(), self.tx.clone());
        self.spawn(tasks::execute(fs, trash, tx, buf, plan, cancel));
    }

    /// Execute a plan without a preview (non-destructive operations only).
    pub(crate) fn start_execution(&mut self, buf: BufferId, plan: OpPlan) {
        self.inflight.insert(plan.id, (plan.clone(), buf));
        let (fs, trash, tx) = (self.svc.fs.clone(), self.svc.trash.clone(), self.tx.clone());
        self.spawn(tasks::execute(fs, trash, tx, buf, plan, Default::default()));
    }

    pub(crate) fn op_done(&mut self, buf: BufferId, result: OpResult) {
        if let Some(BufferKind::Preview(p)) = self.buffer_mut(buf).map(|b| &mut b.kind) {
            p.phase = PreviewPhase::Done(result.clone());
        }
        let Some((plan, origin)) = self.inflight.remove(&result.op_id) else { return };
        let entry = JournalEntry::from_result(&plan, &result, self.svc.clock.unix());
        self.record_journal(entry);
        let mut touched: Vec<PathBuf> = Vec::new();
        if let Some(d) = self.dir_of(origin) {
            let mut moves = Vec::new();
            for r in &result.results {
                touched.push(r.src.parent().unwrap_or(Path::new("/")).to_path_buf());
                if let Some(dst) = &r.dst {
                    touched.push(dst.parent().unwrap_or(Path::new("/")).to_path_buf());
                }
                let Some(key) = &r.key else { continue };
                match (&r.outcome, plan.kind) {
                    (ItemOutcome::Done, OpKind::Trash | OpKind::DeletePermanent) => {
                        d.set_state(key, EntryState::Unmarked);
                        d.annotations.remove(key);
                    }
                    (ItemOutcome::Done, OpKind::Move | OpKind::Rename) => {
                        match r.dst.as_ref().and_then(|dst| d.key_for(dst)) {
                            Some(new_key) if new_key.parent() == key.parent() => moves.push((key.clone(), new_key)),
                            _ => {
                                d.set_state(key, EntryState::Unmarked);
                                d.annotations.remove(key);
                            }
                        }
                        if d.listings.contains_key(key) {
                            d.collapse(key);
                        }
                    }
                    (ItemOutcome::Done, _) => {
                        d.annotations.remove(key);
                    }
                    (ItemOutcome::Failed(e), _) => {
                        d.annotations.insert(key.clone(), e.clone());
                    }
                    (ItemOutcome::Skipped, _) => {}
                }
            }
            d.remap(&moves);
            if plan.kind == OpKind::Rename {
                d.wdired = None;
            }
            d.invalidate();
        }
        if plan.kind == OpKind::Mkdir {
            for i in &plan.items {
                touched.push(i.src.parent().unwrap_or(Path::new("/")).to_path_buf());
            }
        }
        self.refresh_touched(&touched);
        let (ok, failed) = (result.succeeded(), result.failed());
        let summary = format!("{}: {ok} succeeded, {failed} failed", plan.kind.label());
        if failed > 0 {
            let first = result.results.iter().find_map(|r| match &r.outcome {
                ItemOutcome::Failed(e) => Some(e.clone()),
                _ => None,
            });
            self.error(format!("{summary} — {}", first.unwrap_or_default()));
        } else {
            self.message(summary);
        }
    }

    fn refresh_touched(&mut self, dirs: &[PathBuf]) {
        let ids: Vec<BufferId> = self
            .buffers
            .iter()
            .filter_map(|b| {
                let d = b.dir()?;
                let hit = dirs.iter().any(|t| {
                    t == &d.root || d.listings.keys().any(|k| !k.as_os_str().is_empty() && d.root.join(k) == *t)
                });
                hit.then_some(b.id)
            })
            .collect();
        for id in ids {
            self.revert(id);
        }
    }

    // ----- undo -----

    fn undo(&mut self) {
        let Some(idx) = self.journal.iter().rposition(|e| !e.undone) else {
            return self.message("Nothing to undo");
        };
        match undo_steps(&self.journal[idx]) {
            Err(e) => self.error(format!("Cannot undo {}: {e}", self.journal[idx].kind.label())),
            Ok(steps) => {
                self.journal[idx].undone = true;
                let op_id = self.journal[idx].op_id;
                let (fs, trash, tx) = (self.svc.fs.clone(), self.svc.trash.clone(), self.tx.clone());
                self.spawn(tasks::undo(fs, trash, tx, op_id, steps));
                self.message("Undoing…");
            }
        }
    }

    pub(crate) fn undo_done(&mut self, op_id: u64, errors: Vec<String>, touched: Vec<PathBuf>) {
        let parents: Vec<PathBuf> = touched.iter().map(|p| p.parent().unwrap_or(Path::new("/")).to_path_buf()).collect();
        self.refresh_touched(&parents);
        if errors.is_empty() {
            self.message(format!("Undid operation {op_id}"));
        } else {
            self.error(format!("Undo of operation {op_id} partially failed: {}", errors.join("; ")));
        }
    }

    // ----- processes -----

    fn run_shell(&mut self, template: &str, asynchronous: bool, shell_mode: bool) {
        let ops = std::mem::take(&mut self.pending_operands);
        let cwd = self.base_dir();
        let paths: Vec<PathBuf> = ops.into_iter().map(|(p, _)| p).collect();
        let shell = self.config.shell.shell.clone();
        let invs = match build_invocations(template, &paths, &cwd, shell_mode.then_some(shell.as_str())) {
            Ok(v) => v,
            Err(e) => return self.error(e),
        };
        let name = if asynchronous { "*Async Shell Command*" } else { "*Shell Command Output*" };
        self.start_process(name, TextRole::Process, invs, asynchronous, shell_mode, cwd);
    }

    pub(crate) fn run_exec(&mut self, argv: Vec<String>) {
        if !matches!(self.active().kind, BufferKind::Dir(_)) {
            return self.error("external commands run on directory entries");
        }
        if !self.capture_operands() {
            return;
        }
        let cwd = self.base_dir();
        let files: Vec<OsString> = self.pending_operands.iter().map(|(p, _)| operand_arg(p, &cwd)).collect();
        let per_file = argv.iter().any(|a| a == "{file}");
        let build = |fs: &[OsString]| -> Vec<OsString> {
            argv.iter()
                .flat_map(|a| match a.as_str() {
                    "{file}" | "{files}" => fs.to_vec(),
                    _ => vec![OsString::from(a)],
                })
                .collect()
        };
        let argvs: Vec<Vec<OsString>> = if per_file {
            files.iter().map(|f| build(std::slice::from_ref(f))).collect()
        } else {
            vec![build(&files)]
        };
        let invs = argvs
            .into_iter()
            .map(|v| {
                let mut it = v.into_iter();
                Invocation { program: it.next().unwrap_or_default(), args: it.collect(), cwd: cwd.clone(), shell_mode: false }
            })
            .collect();
        self.pending_operands.clear();
        self.start_process("*Shell Command Output*", TextRole::Process, invs, false, false, cwd);
    }

    fn start_process(&mut self, name: &str, role: TextRole, invs: Vec<Invocation>, focus: bool, shell_mode: bool, cwd: PathBuf) {
        let reuse = self.find_buffer(name).filter(|id| {
            !matches!(self.buffer(*id).map(|b| &b.kind), Some(BufferKind::Text(t)) if t.proc.as_ref().map(|p| p.running).unwrap_or(false))
        });
        let name = name.to_string();
        let mut header: Vec<TextLine> = Vec::new();
        if shell_mode {
            header.push(TextLine::new("[SHELL MODE] command interpreted by the shell; files passed as \"$@\"", LineStyle::Warning));
        }
        for inv in &invs {
            header.push(TextLine::new(format!("$ {}", inv.display()), LineStyle::Header));
        }
        let label = invs.first().map(|i| i.program.to_string_lossy().into_owned()).unwrap_or_default();
        let id = match reuse {
            Some(id) => {
                self.text_buffer(&name, role, header, focus);
                id
            }
            None if self.find_buffer(&name).is_some() => {
                let id = self.add_buffer(&name, BufferKind::Text(crate::buffers::TextBuffer::new(role, header)));
                if focus {
                    self.focus(id);
                }
                id
            }
            None => self.text_buffer(&name, role, header, focus),
        };
        let kill = dired_process::KillSwitch::default();
        if let Some(BufferKind::Text(t)) = self.buffer_mut(id).map(|b| &mut b.kind) {
            t.follow = true;
            t.cwd = cwd;
            t.proc = Some(ProcState { label, running: true, exit: None, kill: Some(kill.clone()), shell_mode });
        }
        let (runner, tx) = (self.svc.runner.clone(), self.tx.clone());
        self.spawn(tasks::run_processes(runner, tx, id, invs, kill));
        if !focus {
            self.message(format!("Running… output in {name}"));
        }
    }

    fn run_search(&mut self, pattern: &str) {
        if pattern.is_empty() {
            return self.error("empty search pattern");
        }
        let d = self.dir().unwrap();
        let root = d.root.clone();
        let marked: Vec<PathBuf> = d
            .visible_entries()
            .into_iter()
            .filter(|e| d.state(&e.key) == EntryState::Marked)
            .map(|e| d.abs(&e.key))
            .collect();
        let paths = if marked.is_empty() { vec![root.clone()] } else { marked };
        let mut argv: Vec<OsString> = self.config.search.command.iter().map(OsString::from).collect();
        if argv.is_empty() {
            return self.error("search.command is empty");
        }
        argv.push(OsString::from(pattern));
        argv.extend(paths.into_iter().map(|p| p.into_os_string()));
        let mut it = argv.into_iter();
        let inv = Invocation { program: it.next().unwrap(), args: it.collect(), cwd: root.clone(), shell_mode: false };
        let name = format!("*Search: {pattern}*");
        self.start_process(&name, TextRole::Search, vec![inv], true, false, root);
        if let BufferKind::Text(t) = &mut self.active_mut().kind {
            t.follow = false;
        }
    }

    pub(crate) fn proc_line(&mut self, buf: BufferId, line: String, stderr: bool) {
        let Some(BufferKind::Text(t)) = self.buffer_mut(buf).map(|b| &mut b.kind) else { return };
        if t.role == TextRole::Search && !stderr {
            if let Some((text, target)) = parse_rg_json(&line, &t.cwd) {
                t.push(TextLine::new(text, LineStyle::Match), Some(target));
            } else if !line.trim_start().starts_with('{') {
                t.push(TextLine::normal(line), None);
            }
            return;
        }
        t.push(TextLine::new(line, if stderr { LineStyle::Error } else { LineStyle::Normal }), None);
    }

    pub(crate) fn proc_exit(&mut self, buf: BufferId, code: Option<i32>, error: Option<String>) {
        let Some(b) = self.buffer_mut(buf) else { return };
        let name = b.name.clone();
        let BufferKind::Text(t) = &mut b.kind else { return };
        if let Some(p) = &mut t.proc {
            p.running = false;
            p.exit = Some(code);
        }
        let matches = t.targets.iter().filter(|x| x.is_some()).count();
        let summary = match (&error, t.role) {
            (Some(e), _) => format!("Process failed: {e}"),
            (None, TextRole::Search) => format!("Search finished: {matches} match(es)"),
            (None, _) => match code {
                Some(c) => format!("Process finished with exit code {c}"),
                None => "Process killed".to_string(),
            },
        };
        let style = if error.is_some() || code.unwrap_or(1) != 0 && t.role != TextRole::Search { LineStyle::Error } else { LineStyle::Dim };
        t.push(TextLine::new(summary.clone(), style), None);
        let output: Vec<String> = t.lines.iter().filter(|l| l.style == LineStyle::Normal).map(|l| l.text.clone()).collect();
        let is_bg = self.active != buf;
        if is_bg && output.len() == 1 {
            self.message(format!("{} [{summary}]", output[0]));
        } else if is_bg {
            self.message(format!("{summary}; output in {name}"));
        }
    }

    // ----- misc -----

    fn show_help(&mut self) {
        let mode = self.active().mode();
        let mut lines = vec![
            TextLine::new("dired — key bindings and commands", LineStyle::Header),
            TextLine::new("Keys are Emacs notation: C- = Control, M- = Meta/Alt (or ESC then key).", LineStyle::Dim),
            TextLine::normal(""),
        ];
        let mut modes = vec![mode];
        for m in Mode::ALL {
            if !modes.contains(&m) {
                modes.push(m);
            }
        }
        for m in modes {
            let b = self.keymaps.bindings(m);
            if b.is_empty() {
                continue;
            }
            let active = if m == mode { " (active)" } else { "" };
            lines.push(TextLine::new(format!("{} mode{active}", m.name()), LineStyle::Header));
            for (k, binding) in b {
                let doc = match binding {
                    crate::keymap::Binding::Command(c) => registry::lookup(c).map(|d| d.doc).unwrap_or(""),
                    _ => "",
                };
                lines.push(TextLine::normal(format!("  {k:<12} {:<34} {doc}", binding.describe())));
            }
            lines.push(TextLine::normal(""));
        }
        lines.push(TextLine::new("All commands (M-x)", LineStyle::Header));
        for c in registry::COMMANDS {
            let keys = self.keymaps.keys_for(c.name);
            let aliases = if c.aliases.is_empty() { String::new() } else { format!(" (aliases: {})", c.aliases.join(", ")) };
            lines.push(TextLine::normal(format!("  {:<38} {:<16} {}{aliases}", c.name, keys.join(", "), c.doc)));
        }
        self.text_buffer("*Help*", TextRole::Help, lines, true);
    }

    pub fn reload_config(&mut self) {
        let Some(path) = dired_config::config_path() else {
            return self.error("no configuration directory");
        };
        let (config, diags) = dired_config::Config::load(&path);
        self.apply_config(config, diags, &path.display().to_string());
    }

    /// Apply a configuration; errors are nonfatal and go to *Diagnostics*.
    pub fn apply_config(&mut self, config: dired_config::Config, mut diags: Vec<dired_config::Diagnostic>, source: &str) {
        let (keymaps, kd) = crate::keymap::Keymaps::with_user(&config.keymap, source);
        diags.extend(kd);
        self.keymaps = keymaps;
        self.config = config;
        // dired-7yu
        self.config_gen += 1;
        if diags.is_empty() {
            if let Some(id) = self.find_buffer("*Diagnostics*") {
                self.remove_buffer(id);
            }
            self.diagnostics.clear();
            self.message("Configuration reloaded");
        } else {
            self.set_diagnostics(diags);
        }
    }
}

fn base_dir_fallback() -> PathBuf {
    PathBuf::from("/")
}

/// Parse one `rg --json` line into a display line and target.
pub fn parse_rg_json(line: &str, root: &Path) -> Option<(String, Target)> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "match" {
        return None;
    }
    let data = v.get("data")?;
    let path = data.get("path")?.get("text")?.as_str()?;
    let ln = data.get("line_number").and_then(|n| n.as_u64());
    let text = data.get("lines").and_then(|l| l.get("text")).and_then(|t| t.as_str()).unwrap_or("");
    let p = PathBuf::from(path);
    let shown = p.strip_prefix(root).unwrap_or(&p).display().to_string();
    let text = text.trim_end_matches(['\n', '\r']);
    Some((
        format!("{shown}:{}: {text}", ln.map(|n| n.to_string()).unwrap_or_default()),
        Target { path: if p.is_absolute() { p } else { root.join(p) }, line: ln },
    ))
}
