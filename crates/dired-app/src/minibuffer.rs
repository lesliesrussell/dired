// dired-7qs
//! The minibuffer: a single-line editor with history and completion.

use std::path::PathBuf;

use dired_config::{Key, KeyCode};
use dired_domain::EntryState;

use crate::buffers::BufferId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    None,
    Path,
    Command,
    Buffer,
    // dired-0cz
    OpenWith,
    // dired-2hn
    Theme,
    // dired-5xb
    Style,
}

impl Completion {
    // dired-2hn
    /// Completions whose candidate list is a closed set the user picks from:
    /// the highlighted candidate is accepted, and C-n/C-p move it.
    pub fn picks(self) -> bool {
        matches!(
            self,
            Completion::Command | Completion::Buffer | Completion::OpenWith | Completion::Theme | Completion::Style
        )
    }
}

/// What happens when the prompt is submitted.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptAction {
    OpenDirectory,
    // dired-ta2
    OpenDirectoryOther,
    FindFileOther,
    SwitchBufferOther,
    FindFile,
    Copy,
    Move,
    Mkdir,
    Chmod,
    Shell { asynchronous: bool, shell_mode: bool },
    ConfirmShell { template: String, asynchronous: bool },
    MarkRegexp(EntryState),
    RenameRegexpFrom,
    RenameRegexpTo(String),
    Search,
    GotoFile,
    Filter,
    SwitchBuffer,
    KillBuffer,
    ConfirmKill(BufferId),
    ExtendedCommand,
    ConfirmExecute(BufferId),
    ConflictRename { buf: BufferId, item: usize },
    Isearch { forward: bool, origin: usize },
    // dired-0cz
    OpenWith { paths: Vec<PathBuf> },
    // dired-2hn
    LoadTheme,
    // dired-5xb
    LoadStyle,
    // dired-tlu
    MarkWhere(EntryState),
}

impl PromptAction {
    pub fn history_key(&self) -> &'static str {
        match self {
            PromptAction::OpenDirectory
            | PromptAction::FindFile
            | PromptAction::OpenDirectoryOther
            | PromptAction::FindFileOther => "file",
            PromptAction::Copy | PromptAction::Move | PromptAction::ConflictRename { .. } => "destination",
            PromptAction::Mkdir => "mkdir",
            PromptAction::Chmod => "chmod",
            PromptAction::Shell { .. } => "shell",
            PromptAction::MarkRegexp(_) | PromptAction::RenameRegexpFrom => "regexp",
            PromptAction::RenameRegexpTo(_) => "replacement",
            PromptAction::Search => "search",
            PromptAction::GotoFile => "goto",
            PromptAction::Filter => "filter",
            PromptAction::SwitchBuffer | PromptAction::KillBuffer | PromptAction::SwitchBufferOther => "buffer",
            PromptAction::ExtendedCommand => "command",
            // dired-0cz
            PromptAction::OpenWith { .. } => "open-with",
            PromptAction::LoadTheme => "theme", // dired-2hn
            PromptAction::LoadStyle => "style", // dired-5xb
            PromptAction::MarkWhere(_) => "predicate", // dired-tlu
            PromptAction::Isearch { .. } => "isearch",
            PromptAction::ConfirmShell { .. }
            | PromptAction::ConfirmKill(_)
            | PromptAction::ConfirmExecute(_) => "confirm",
        }
    }

    pub fn completion(&self) -> Completion {
        match self {
            PromptAction::OpenDirectory
            | PromptAction::FindFile
            | PromptAction::OpenDirectoryOther
            | PromptAction::FindFileOther
            | PromptAction::Copy
            | PromptAction::Move
            | PromptAction::Mkdir
            | PromptAction::ConflictRename { .. } => Completion::Path,
            PromptAction::ExtendedCommand => Completion::Command,
            PromptAction::SwitchBuffer | PromptAction::KillBuffer | PromptAction::SwitchBufferOther => Completion::Buffer,
            // dired-0cz
            PromptAction::OpenWith { .. } => Completion::OpenWith,
            PromptAction::LoadTheme => Completion::Theme, // dired-2hn
            PromptAction::LoadStyle => Completion::Style, // dired-5xb
            _ => Completion::None,
        }
    }

    /// Single-key y/n confirmation.
    pub fn yes_or_no_key(&self) -> bool {
        matches!(self, PromptAction::ConfirmKill(_))
    }
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub label: String,
    pub input: String,
    /// Cursor, in chars.
    pub cursor: usize,
    pub action: PromptAction,
    pub candidates: Vec<String>,
    pub selected: usize,
    // dired-t1z
    /// The user moved the selection (C-n/C-p/arrows); it then wins on submit
    /// even when the typed text is an exact match.
    pub picked: bool,
    pub hist_pos: Option<usize>,
    /// Bumped on every edit; stale async completions are discarded.
    pub gen: u64,
    pub completing_for: Option<String>,
    pub base_dir: PathBuf,
    /// Buffer the prompt was opened from; submission acts on it.
    pub origin: BufferId,
}

pub enum EditResult {
    Changed,
    Moved,
    Submit,
    Complete,
    HistoryPrev,
    HistoryNext,
    IsearchNext,
    IsearchPrev,
    // dired-t1z
    CandidateNext,
    CandidatePrev,
    Unhandled,
}

impl Prompt {
    pub fn new(label: impl Into<String>, initial: impl Into<String>, action: PromptAction) -> Self {
        let input: String = initial.into();
        Prompt {
            label: label.into(),
            cursor: input.chars().count(),
            input,
            action,
            candidates: Vec::new(),
            selected: 0,
            picked: false,
            hist_pos: None,
            gen: 0,
            completing_for: None,
            base_dir: PathBuf::new(),
            origin: 0,
        }
    }

    fn byte(&self, char_idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.input.len())
    }

    pub fn set_input(&mut self, s: String) {
        self.cursor = s.chars().count();
        self.input = s;
        self.gen += 1;
    }

    // dired-t1z
    /// Whether C-n/C-p/Up/Down select among candidates (rather than walk history).
    fn navigable(&self) -> bool {
        // dired-bmd: path prompts list candidates only after TAB; then they navigate too
        let c = self.action.completion();
        (c.picks() || c == Completion::Path) && !self.candidates.is_empty()
    }

    pub fn edit(&mut self, key: Key) -> EditResult {
        let len = self.input.chars().count();
        if let Some(c) = key.printable() {
            let b = self.byte(self.cursor);
            self.input.insert(b, c);
            self.cursor += 1;
            self.gen += 1;
            return EditResult::Changed;
        }
        let r = match (key.code, key.ctrl, key.meta) {
            (KeyCode::Enter, false, false) | (KeyCode::Char('j'), true, false) | (KeyCode::Char('m'), true, false) => {
                return EditResult::Submit
            }
            (KeyCode::Tab, false, false) | (KeyCode::Char('i'), true, false) => return EditResult::Complete,
            // dired-t1z
            (KeyCode::Char('p'), true, false) | (KeyCode::Up, false, false) if self.navigable() => {
                return EditResult::CandidatePrev
            }
            (KeyCode::Char('n'), true, false) | (KeyCode::Down, false, false) if self.navigable() => {
                return EditResult::CandidateNext
            }
            (KeyCode::Char('p'), false, true) | (KeyCode::Up, false, false) => return EditResult::HistoryPrev,
            (KeyCode::Char('n'), false, true) | (KeyCode::Down, false, false) => return EditResult::HistoryNext,
            (KeyCode::Char('s'), true, false) => return EditResult::IsearchNext,
            (KeyCode::Char('r'), true, false) => return EditResult::IsearchPrev,
            (KeyCode::Backspace, false, _) | (KeyCode::Char('h'), true, false) => {
                if self.cursor == 0 {
                    return EditResult::Moved;
                }
                if key.meta {
                    return self.kill_word_back();
                }
                let b = self.byte(self.cursor - 1);
                self.input.remove(b);
                self.cursor -= 1;
                EditResult::Changed
            }
            (KeyCode::Delete, false, false) | (KeyCode::Char('d'), true, false) => {
                if self.cursor >= len {
                    return EditResult::Moved;
                }
                let b = self.byte(self.cursor);
                self.input.remove(b);
                EditResult::Changed
            }
            (KeyCode::Char('k'), true, false) => {
                let b = self.byte(self.cursor);
                self.input.truncate(b);
                EditResult::Changed
            }
            (KeyCode::Char('u'), true, false) => {
                let b = self.byte(self.cursor);
                self.input.drain(..b);
                self.cursor = 0;
                EditResult::Changed
            }
            (KeyCode::Char('w'), true, false) => return self.kill_word_back(),
            (KeyCode::Char('a'), true, false) | (KeyCode::Home, false, false) => {
                self.cursor = 0;
                return EditResult::Moved;
            }
            (KeyCode::Char('e'), true, false) | (KeyCode::End, false, false) => {
                self.cursor = len;
                return EditResult::Moved;
            }
            (KeyCode::Char('f'), true, false) | (KeyCode::Right, false, false) => {
                self.cursor = (self.cursor + 1).min(len);
                return EditResult::Moved;
            }
            (KeyCode::Char('b'), true, false) | (KeyCode::Left, false, false) => {
                self.cursor = self.cursor.saturating_sub(1);
                return EditResult::Moved;
            }
            _ => return EditResult::Unhandled,
        };
        self.gen += 1;
        r
    }

    fn kill_word_back(&mut self) -> EditResult {
        let chars: Vec<char> = self.input.chars().collect();
        let mut i = self.cursor;
        while i > 0 && (chars[i - 1] == '/' || chars[i - 1].is_whitespace()) {
            i -= 1;
        }
        while i > 0 && chars[i - 1] != '/' && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let (a, b) = (self.byte(i), self.byte(self.cursor));
        self.input.drain(a..b);
        self.cursor = i;
        self.gen += 1;
        EditResult::Changed
    }
}

/// Longest common prefix of candidates.
pub fn common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else { return String::new() };
    let mut end = first.len();
    for s in &items[1..] {
        end = end.min(
            first
                .char_indices()
                .zip(s.chars())
                .take_while(|((_, a), b)| a == b)
                .last()
                .map(|((i, c), _)| i + c.len_utf8())
                .unwrap_or(0),
        );
    }
    first[..end].to_string()
}

/// Fuzzy-rank `items` against `query` (best first).
pub fn fuzzy(query: &str, items: impl IntoIterator<Item = String>) -> Vec<String> {
    use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
    use nucleo_matcher::{Config, Matcher};
    let items: Vec<String> = items.into_iter().collect();
    if query.is_empty() {
        return items;
    }
    let mut m = Matcher::new(Config::DEFAULT);
    Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart)
        .match_list(items, &mut m)
        .into_iter()
        .map(|(s, _)| s)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix() {
        assert_eq!(common_prefix(&["abc".into(), "abd".into()]), "ab");
        assert_eq!(common_prefix(&["x".into()]), "x");
        assert_eq!(common_prefix(&["é1".into(), "é2".into()]), "é");
    }

    #[test]
    fn fuzzy_ranks() {
        let r = fuzzy("dcopy", vec!["dired-do-copy".to_string(), "next-line".to_string()]);
        assert_eq!(r, vec!["dired-do-copy"]);
    }
}
