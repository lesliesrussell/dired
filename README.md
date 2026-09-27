<!-- dired-7qs -->
# dired

A standalone terminal directory editor built on Emacs Dired's interaction model: a directory is a mutable, line-oriented buffer with point, marks, deletion flags, a minibuffer, and multiple buffers.

```sh
cargo run --release -- [PATH]
dired --check-config
```

Press `?` inside dired to see every binding and command.

## Essentials

| Keys | Command |
|---|---|
| `C-n` `C-p` `↓` `↑` `M-<` `M->` `C-v` `M-v` | Move |
| `C-s` `C-r` / `j` | Incremental search / goto file |
| `RET` `f` / `^` | Visit entry / parent directory |
| `g` | Refresh (keeps point on the same file) |
| `m` `u` `U` `t` `% m` `% u` | Mark, unmark, unmark all, toggle, regexp mark/unmark |
| `d` `% d` then `x` | Flag for deletion, then preview and move to trash |
| `C` `R` `M` `+` | Copy, move/rename, chmod, mkdir (copy/move/chmod go through a preview) |
| `C-x C-q` … `C-c C-c` / `C-c C-k` | Wdired: edit names as text, then commit or abort |
| `% r` | Rename marked entries by regexp |
| `!` `&` | Run a command on marked entries (argv-safe; `*` = all files, `?` = once per file) |
| `A` | Search file contents (`rg --json`); `RET` on a match visits it |
| `i` `$` | Insert subdirectory inline / collapse it |
| `s` `/` `C-x M-o` | Cycle sort, filter, toggle hidden files |
| `b` `C-x k` `q` | Switch buffer, kill buffer, close buffer |
| `C-x u` | Best-effort undo of the last operation |
| `M-x` `C-g` | Run a command by name / cancel |

In a preview buffer: `x` executes, `q` cancels. For conflicts: `o` overwrite, `s` skip, `r` rename the target, `a` ask, `O` overwrite all, `S` skip all. In a deletion preview, `D` switches to permanent deletion. Permanent deletion, overwrites, and shell mode each require you to type `yes`.

## Configuration

`$XDG_CONFIG_HOME/dired/config.toml` (or `~/.config/dired/config.toml`, or `$DIRED_CONFIG`). Errors never stop the app; they are listed in the `*Diagnostics*` buffer.

```toml
version = 1

[display]
show_hidden = true
dirs_first = true
sort = "name"                  # name | natural | time | size | extension
columns = ["permissions", "size", "mtime"]

[theme]
name = "default"               # default | high-contrast | mono (NO_COLOR selects mono)
colors = { directory = "lightblue", flagged = "#ff5555" }
glyphs = { marked = "*", flagged = "D", error = "!", changed = "~", focused = ">" }

[[keymap]]
mode = "dired"                 # global | dired | wdired | preview | text
key = "C-c o"
exec = ["open", "-R", "{file}"] # {file}: once per operand, {files}: all operands

[[keymap]]
key = "C-c s"
command = "dired-sort-by-size"

[[opener]]
extensions = ["png", "jpg", "pdf"]
command = ["open", "{file}"]

[[opener]]
extensions = ["rs", "toml", "md"]
command = ["nvim", "+{line}", "{file}"]
terminal = true                # suspends the UI while it runs

[shell]
shell = "/bin/sh"              # used only by dired-do-shell-command-in-shell

[search]
command = ["rg", "--json", "-e"]

[safety]
trash_fallback = "refuse"      # or "permanent" (still requires typing yes)
follow_symlinks_on_insert = false
persist_history = true
```

History, the operation journal (`journal.jsonl`), and diagnostics are stored in the platform state directory (`$DIRED_STATE_DIR` overrides). Nothing is sent over the network.

## Layout

| Crate | Role |
|---|---|
| `dired-domain` | Entries, sorting, operation plans, Wdired rename scheduling, argv construction, journal |
| `dired-fs` | `FileSystem` and `TrashBackend` traits, local adapter, plan executor |
| `dired-process` | `ProcessRunner` with streamed output (argv only) |
| `dired-config` | Versioned TOML config, key notation, diagnostics |
| `dired-app` | Buffers, keymaps, minibuffer, commands, background tasks |
| `dired-tui` | ratatui/crossterm rendering and event loop |
| `dired-testkit` | Mock filesystem (device IDs, failure injection), trash, process runner, clock |

`cargo test --workspace` runs the spec acceptance tests (T1–T16, in `crates/dired-app/tests/gates.rs`) plus unit and property tests.
