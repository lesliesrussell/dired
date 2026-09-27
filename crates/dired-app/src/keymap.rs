// dired-7qs
//! Mode keymaps with prefix sequences; defaults follow Dired.

use std::collections::{BTreeMap, HashMap, HashSet};

use dired_config::{format_seq, parse_seq, Diagnostic, Key, KeyBindingSpec};

use crate::registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    Global,
    Dired,
    Wdired,
    Preview,
    Text,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Global, Mode::Dired, Mode::Wdired, Mode::Preview, Mode::Text];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Global => "global",
            Mode::Dired => "dired",
            Mode::Wdired => "wdired",
            Mode::Preview => "preview",
            Mode::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.name() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    Command(String),
    /// External argv template; `{file}` runs once per operand, `{files}` expands all.
    Exec(Vec<String>),
    /// Reserved for a future plugin runtime.
    Plugin(String),
}

impl Binding {
    pub fn describe(&self) -> String {
        match self {
            Binding::Command(c) => c.clone(),
            Binding::Exec(argv) => format!("exec {}", argv.join(" ")),
            Binding::Plugin(p) => format!("plugin {p}"),
        }
    }
}

pub enum Lookup<'a> {
    Found(&'a Binding),
    Prefix,
    None,
}

type ModeMap = BTreeMap<Vec<KeyOrd>, (Vec<Key>, Binding)>;

#[derive(Debug, Clone, Default)]
pub struct Keymaps {
    maps: HashMap<Mode, ModeMap>,
    prefixes: HashMap<Mode, HashSet<Vec<Key>>>,
}

/// Orderable wrapper so help output is stable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct KeyOrd(String);

fn ord(seq: &[Key]) -> Vec<KeyOrd> {
    seq.iter().map(|k| KeyOrd(k.to_string())).collect()
}

const DEFAULTS: &[(Mode, &str, &str)] = &[
    (Mode::Global, "C-x d", "dired-open-directory"),
    (Mode::Global, "C-x C-f", "dired-find-file"),
    (Mode::Global, "C-x k", "buffer-kill"),
    (Mode::Global, "C-x b", "switch-to-buffer"),
    (Mode::Global, "C-x u", "dired-undo"),
    (Mode::Global, "C-/", "dired-undo"),
    (Mode::Global, "C-_", "dired-undo"),
    (Mode::Global, "C-x C-c", "save-buffers-kill-terminal"),
    (Mode::Global, "C-n", "next-line"),
    (Mode::Global, "<down>", "next-line"),
    (Mode::Global, "C-p", "previous-line"),
    (Mode::Global, "<up>", "previous-line"),
    (Mode::Global, "M-<", "beginning-of-buffer"),
    (Mode::Global, "<home>", "beginning-of-buffer"),
    (Mode::Global, "M->", "end-of-buffer"),
    (Mode::Global, "<end>", "end-of-buffer"),
    (Mode::Global, "C-v", "scroll-up-command"),
    (Mode::Global, "<next>", "scroll-up-command"),
    (Mode::Global, "M-v", "scroll-down-command"),
    (Mode::Global, "<prior>", "scroll-down-command"),
    (Mode::Global, "C-s", "isearch-forward"),
    (Mode::Global, "C-r", "isearch-backward"),
    (Mode::Global, "M-x", "execute-extended-command"),
    (Mode::Global, "C-g", "keyboard-quit"),
    (Mode::Global, "C-h m", "help-for-help"),
    (Mode::Global, "C-h b", "help-for-help"),
    (Mode::Global, "<f1>", "help-for-help"),
    (Mode::Dired, "RET", "dired-visit"),
    (Mode::Dired, "f", "dired-visit"),
    (Mode::Dired, "e", "dired-visit"),
    (Mode::Dired, "^", "dired-up-directory"),
    (Mode::Dired, "g", "revert-buffer"),
    (Mode::Dired, "n", "next-line"),
    (Mode::Dired, "SPC", "next-line"),
    (Mode::Dired, "p", "previous-line"),
    (Mode::Dired, "j", "dired-goto-file"),
    (Mode::Dired, "m", "dired-mark"),
    (Mode::Dired, "u", "dired-unmark"),
    (Mode::Dired, "DEL", "dired-unmark-backward"),
    (Mode::Dired, "U", "dired-unmark-all-marks"),
    (Mode::Dired, "t", "dired-toggle-marks"),
    (Mode::Dired, "d", "dired-flag-file-deletion"),
    (Mode::Dired, "x", "dired-do-flagged-delete"),
    (Mode::Dired, "C", "dired-do-copy"),
    (Mode::Dired, "R", "dired-do-rename"),
    (Mode::Dired, "M", "dired-do-chmod"),
    (Mode::Dired, "+", "dired-create-directory"),
    (Mode::Dired, "!", "dired-do-shell-command"),
    (Mode::Dired, "&", "dired-do-async-shell-command"),
    (Mode::Dired, "i", "dired-maybe-insert-subdir"),
    (Mode::Dired, "$", "dired-hide-subdir"),
    (Mode::Dired, "% m", "dired-mark-files-regexp"),
    (Mode::Dired, "% d", "dired-flag-files-regexp"),
    (Mode::Dired, "% u", "dired-unmark-files-regexp"),
    (Mode::Dired, "% r", "dired-do-rename-regexp"),
    (Mode::Dired, "% R", "dired-do-rename-regexp"),
    (Mode::Dired, "A", "dired-search-files-content"),
    (Mode::Dired, "s", "dired-sort-toggle-or-edit"),
    (Mode::Dired, "/", "dired-filter"),
    (Mode::Dired, "C-x M-o", "dired-toggle-hidden"),
    (Mode::Dired, "C-x C-q", "wdired-toggle-edit"),
    (Mode::Dired, "b", "switch-to-buffer"),
    (Mode::Dired, "?", "help-for-help"),
    (Mode::Dired, "h", "help-for-help"),
    (Mode::Dired, "q", "quit-window"),
    (Mode::Wdired, "C-c C-c", "wdired-finish-edit"),
    (Mode::Wdired, "C-x C-q", "wdired-finish-edit"),
    (Mode::Wdired, "C-x C-s", "wdired-finish-edit"),
    (Mode::Wdired, "C-c C-k", "wdired-abort-changes"),
    (Mode::Wdired, "C-c ESC", "wdired-abort-changes"),
    (Mode::Preview, "x", "operation-execute"),
    (Mode::Preview, "y", "operation-execute"),
    (Mode::Preview, "C-c C-c", "operation-execute"),
    (Mode::Preview, "n", "next-line"),
    (Mode::Preview, "p", "previous-line"),
    (Mode::Preview, "o", "conflict-overwrite"),
    (Mode::Preview, "s", "conflict-skip"),
    (Mode::Preview, "r", "conflict-rename"),
    (Mode::Preview, "a", "conflict-ask"),
    (Mode::Preview, "O", "conflict-overwrite-all"),
    (Mode::Preview, "S", "conflict-skip-all"),
    (Mode::Preview, "D", "operation-toggle-permanent"),
    (Mode::Preview, "q", "quit-window"),
    (Mode::Preview, "?", "help-for-help"),
    (Mode::Preview, "b", "switch-to-buffer"),
    (Mode::Text, "RET", "text-visit"),
    (Mode::Text, "n", "next-line"),
    (Mode::Text, "p", "previous-line"),
    (Mode::Text, "C-c C-c", "process-kill"),
    (Mode::Text, "q", "quit-window"),
    (Mode::Text, "?", "help-for-help"),
    (Mode::Text, "b", "switch-to-buffer"),
];

impl Keymaps {
    pub fn bind(&mut self, mode: Mode, seq: Vec<Key>, b: Binding) {
        for i in 1..seq.len() {
            self.prefixes.entry(mode).or_default().insert(seq[..i].to_vec());
        }
        self.maps.entry(mode).or_default().insert(ord(&seq), (seq, b));
    }

    pub fn defaults() -> Keymaps {
        let mut k = Keymaps::default();
        for (mode, seq, cmd) in DEFAULTS {
            let seq = parse_seq(seq).expect("valid default key");
            k.bind(*mode, seq, Binding::Command((*cmd).to_string()));
        }
        k
    }

    /// Defaults plus user bindings; problems are reported, never fatal.
    pub fn with_user(specs: &[KeyBindingSpec], source: &str) -> (Keymaps, Vec<Diagnostic>) {
        let mut k = Keymaps::defaults();
        let mut diags = Vec::new();
        let mut seen: HashMap<(Mode, Vec<Key>), usize> = HashMap::new();
        for (i, spec) in specs.iter().enumerate() {
            let at = format!("keymap[{i}] `{}`", spec.key);
            let Some(mode) = Mode::parse(&spec.mode) else {
                diags.push(Diagnostic::error(source, format!("{at}: unknown mode `{}`", spec.mode)));
                continue;
            };
            let seq = match parse_seq(&spec.key) {
                Ok(s) => s,
                Err(e) => {
                    diags.push(Diagnostic::error(source, format!("{at}: {e}")));
                    continue;
                }
            };
            let binding = if let Some(c) = &spec.command {
                match registry::lookup(c) {
                    Some(def) => Binding::Command(def.name.to_string()),
                    None => {
                        diags.push(Diagnostic::error(source, format!("{at}: unknown command `{c}`")));
                        continue;
                    }
                }
            } else if let Some(argv) = &spec.exec {
                if argv.is_empty() {
                    diags.push(Diagnostic::error(source, format!("{at}: empty exec argv")));
                    continue;
                }
                Binding::Exec(argv.clone())
            } else {
                Binding::Plugin(spec.plugin.clone().unwrap_or_default())
            };
            if let Some(prev) = seen.insert((mode, seq.clone()), i) {
                diags.push(Diagnostic::error(
                    source,
                    format!(
                        "{at}: duplicate chord `{}` in mode {} (also keymap[{prev}]); last one wins",
                        format_seq(&seq),
                        mode.name()
                    ),
                ));
            }
            if k.prefixes.get(&mode).map(|p| p.contains(&seq)).unwrap_or(false) {
                diags.push(Diagnostic::warning(
                    source,
                    format!("{at}: `{}` is a prefix key; binding it shadows longer sequences", format_seq(&seq)),
                ));
            }
            k.bind(mode, seq, binding);
        }
        (k, diags)
    }

    pub fn lookup(&self, modes: &[Mode], seq: &[Key]) -> Lookup<'_> {
        for m in modes {
            if let Some((_, b)) = self.maps.get(m).and_then(|map| map.get(&ord(seq))) {
                return Lookup::Found(b);
            }
        }
        if modes
            .iter()
            .any(|m| self.prefixes.get(m).map(|p| p.contains(seq)).unwrap_or(false))
        {
            return Lookup::Prefix;
        }
        Lookup::None
    }

    /// Continuations of a pending prefix, for the transient key hint.
    pub fn continuations(&self, modes: &[Mode], prefix: &[Key]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for m in modes {
            if let Some(map) = self.maps.get(m) {
                for (seq, b) in map.values() {
                    if seq.len() > prefix.len() && seq.starts_with(prefix) {
                        let rest = format_seq(&seq[prefix.len()..]);
                        if seen.insert(rest.clone()) {
                            out.push((rest, b.describe()));
                        }
                    }
                }
            }
        }
        out
    }

    pub fn bindings(&self, mode: Mode) -> Vec<(String, &Binding)> {
        self.maps
            .get(&mode)
            .map(|m| m.values().map(|(s, b)| (format_seq(s), b)).collect())
            .unwrap_or_default()
    }

    pub fn keys_for(&self, command: &str) -> Vec<String> {
        let mut v = Vec::new();
        for m in Mode::ALL {
            for (s, b) in self.bindings(m) {
                if *b == Binding::Command(command.to_string()) {
                    v.push(s);
                }
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_reference_real_commands() {
        for (_, _, c) in DEFAULTS {
            assert!(registry::lookup(c).is_some(), "{c}");
        }
    }
}
