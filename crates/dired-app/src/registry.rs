// dired-7qs
//! Interactive command registry: canonical names, aliases, documentation.

pub struct CommandDef {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub doc: &'static str,
}

macro_rules! cmds {
    ($( $name:literal [$($alias:literal),*] $doc:literal ;)*) => {
        pub const COMMANDS: &[CommandDef] = &[
            $( CommandDef { name: $name, aliases: &[$($alias),*], doc: $doc }, )*
        ];
    };
}

cmds! {
    "dired-open-directory" ["dired"] "Prompt for a directory and open or focus its buffer.";
    "dired-find-file" ["find-file"] "Prompt for a path; open directories as buffers and visit files.";
    "wdired-toggle-edit" ["wdired-change-to-wdired-mode", "dired-toggle-read-only"] "Enter or finish editable-filename (Wdired) mode.";
    "wdired-finish-edit" [] "Validate name edits and open the rename preview.";
    "wdired-abort-changes" [] "Discard uncommitted filename edits.";
    "buffer-kill" ["kill-buffer"] "Close a buffer (confirms when work is unfinished).";
    "next-line" ["dired-next-line"] "Move point to the next line.";
    "previous-line" ["dired-previous-line"] "Move point to the previous line.";
    "beginning-of-buffer" [] "Move point to the first line.";
    "end-of-buffer" [] "Move point to the last line.";
    "scroll-up-command" [] "Scroll forward one screen.";
    "scroll-down-command" [] "Scroll backward one screen.";
    "isearch-forward" [] "Incremental search forward over names/lines.";
    "isearch-backward" [] "Incremental search backward over names/lines.";
    "dired-goto-file" [] "Prompt for a name substring and move to the next match.";
    "dired-visit" ["dired-find-file-at-point", "dired-view-file"] "Visit the entry at point: open directories, run the opener on files.";
    "dired-open-with" [] "Choose a program (configured [[open_with]] or built-in) to open the entry at point.";
    "dired-copy-name" [] "Copy the file names of the marked entries (else the entry at point) to the clipboard.";
    "dired-copy-path" [] "Copy the full paths of the marked entries (else the entry at point) to the clipboard.";
    "jump-to-directory" ["jump"] "Pick a bookmark or a frequently used directory and open it.";
    "dired-bookmark-set" [] "Bookmark the current directory under a name.";
    "dired-bookmark-delete" [] "Delete a bookmark.";
    "dired-up-directory" [] "Open or focus the parent directory.";
    "revert-buffer" ["dired-revert"] "Reload the listing asynchronously, preserving point.";
    "dired-mark" [] "Mark the entry at point and advance.";
    "dired-unmark" [] "Remove the mark or deletion flag at point and advance.";
    "dired-unmark-backward" [] "Move up and remove the mark there.";
    "dired-unmark-all-marks" [] "Clear ordinary marks (deletion flags are kept).";
    "dired-toggle-marks" [] "Toggle ordinary marks on all visible entries.";
    "dired-flag-file-deletion" [] "Flag the entry at point for deletion and advance.";
    "dired-do-flagged-delete" [] "Preview and trash all flagged entries.";
    "dired-do-flagged-delete-permanently" [] "Preview and PERMANENTLY delete flagged entries.";
    "dired-do-copy" [] "Copy marked entries (or the entry at point).";
    "dired-do-rename" ["dired-do-move"] "Move/rename marked entries (or the entry at point).";
    "dired-create-directory" [] "Create a directory.";
    "dired-do-chmod" [] "Change the mode of marked entries.";
    "dired-do-shell-command" [] "Run a command on marked entries (argv-safe).";
    "dired-do-async-shell-command" [] "Run a command asynchronously, streaming into a process buffer.";
    "dired-do-shell-command-in-shell" [] "Run a command through the shell (opt-in; files passed as \"$@\").";
    "dired-maybe-insert-subdir" [] "Insert the directory at point as an indented subtree.";
    "dired-hide-subdir" [] "Collapse the inserted subtree at or containing point.";
    "dired-mark-files-where" [] "Mark files matching a condition such as size>10M age>30d ext=log.";
    "dired-flag-files-where" [] "Flag files matching a condition for deletion.";
    "dired-unmark-files-where" [] "Unmark files matching a condition.";
    "dired-mark-files-regexp" [] "Mark visible entries whose names match a regexp.";
    "dired-flag-files-regexp" [] "Flag visible entries whose names match a regexp for deletion.";
    "dired-unmark-files-regexp" [] "Unmark visible entries whose names match a regexp.";
    "dired-do-rename-regexp" [] "Rename marked entries by regexp replacement.";
    "dired-search-files-content" ["dired-do-find-regexp"] "Search file contents recursively (rg --json).";
    "dired-sort-toggle-or-edit" ["dired-sort-cycle"] "Cycle sort: name, natural, time, size, extension.";
    "dired-sort-by-name" [] "Sort by name.";
    "dired-sort-by-natural" [] "Sort by natural (version) name order.";
    "dired-sort-by-time" [] "Sort by modification time, newest first.";
    "dired-sort-by-size" [] "Sort by size, largest first.";
    "dired-sort-by-extension" [] "Sort by extension.";
    "dired-sort-reverse" [] "Reverse the sort order.";
    "dired-sort-toggle-dirs-first" [] "Toggle listing directories first.";
    "dired-toggle-hidden" ["dired-omit-mode"] "Show or hide dotfiles (marks are kept).";
    "dired-filter" ["dired-narrow"] "Filter visible entries by substring (empty clears).";
    "dired-undo" ["undo"] "Best-effort undo of the last journaled operation.";
    "switch-to-buffer" [] "Switch buffers with fuzzy completion.";
    "help-for-help" ["describe-mode", "describe-bindings"] "Show key bindings and commands.";
    "execute-extended-command" [] "Run any command by name.";
    "keyboard-quit" [] "Cancel prompt, preview, prefix, or running task.";
    "quit-window" [] "Close the active buffer; quits after the last one.";
    "save-buffers-kill-terminal" ["kill-emacs", "quit"] "Exit dired.";
    "dired-reload-config" [] "Reload configuration; problems go to *Diagnostics*.";
    "split-window-below" [] "Split the selected window into two, one above the other (C-x 2).";
    "split-window-right" [] "Split the selected window into two, side by side (C-x 3).";
    "other-window" [] "Select the next window.";
    "delete-window" [] "Close the selected window; its buffer stays open.";
    "delete-other-windows" [] "Make the selected window fill the screen.";
    "dired-other-window" [] "Prompt for a directory and open it in the other window.";
    "find-file-other-window" [] "Prompt for a path and open it in the other window.";
    "switch-to-buffer-other-window" [] "Show a buffer in the other window.";
    "dired-find-file-other-window" ["dired-visit-other-window"] "Visit the entry at point in the other window.";
    "load-style" [] "Prompt for a UI style (with completion) and switch to it for this session.";
    "cycle-style" [] "Switch to the next UI style: rich, clean, minimal (for this session).";
    "cycle-theme" [] "Switch to the next built-in theme (for this session).";
    "load-theme" [] "Prompt for a built-in theme (with completion) and switch to it for this session.";
    "view-diagnostics" [] "Show the *Diagnostics* buffer.";
    "view-messages" [] "Show the *Messages* buffer.";
    "operation-execute" [] "Execute the previewed operation.";
    "conflict-overwrite" [] "Resolve the conflict at point by overwriting.";
    "conflict-skip" [] "Skip the item at point.";
    "conflict-rename" [] "Choose a new destination for the item at point.";
    "conflict-ask" [] "Reset the item at point to undecided.";
    "conflict-overwrite-all" [] "Overwrite every undecided conflict where valid.";
    "conflict-skip-all" [] "Skip every undecided conflict.";
    "operation-toggle-permanent" [] "Toggle trash vs PERMANENT deletion in a deletion preview.";
    "text-visit" [] "Visit the location on the current line (search results).";
    "process-kill" ["kill-process"] "Stop the process of this buffer.";
}

pub fn lookup(name: &str) -> Option<&'static CommandDef> {
    COMMANDS
        .iter()
        .find(|c| c.name == name || c.aliases.contains(&name))
}
