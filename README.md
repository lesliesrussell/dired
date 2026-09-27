<!-- dired-7qs -->
# dired

A standalone terminal directory editor built on Emacs Dired's interaction model: a directory is a mutable, line-oriented buffer with point, marks, deletion flags, a minibuffer, and multiple buffers.

```sh
make install                    # builds release, installs to /usr/local/bin/dired
make install PREFIX=~/.local    # or any prefix; ~ is expanded (DESTDIR is honored for packaging)
make uninstall                  # same PREFIX as install
make check                      # tests + clippy
dired [PATH]
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

`$XDG_CONFIG_HOME/dired/config.toml` (or `~/.config/dired/config.toml`, or `$DIRED_CONFIG`). Errors never stop the app; they are listed in the `*Diagnostics*` buffer, and `dired --check-config` reports them without starting the UI.

**[`examples/config.toml`](examples/config.toml)** is a fully commented tour of every option: themes, per-role styles, glyphs, key bindings to commands and external programs, openers, search, and safety. `make release` copies it to `target/release/examples/`, and `make install` installs it to `$(PREFIX)/share/dired/examples/`.

```sh
cp examples/config.toml ~/.config/dired/config.toml
```

### Themes

Built-in: `default`, `high-contrast`, `mono`, `nord`, `gruvbox`, `solarized-dark`, `dracula`. Run `M-x cycle-theme` to preview them live. Hex colors are exact on truecolor terminals (`COLORTERM=truecolor`) and fall back to the nearest ANSI color elsewhere. `NO_COLOR` selects `mono`.

```toml
[theme]
name = "gruvbox"

[theme.colors]                 # shorthand: role → foreground
directory = "#83a598"

[theme.styles.point]           # full control: fg, bg, bold, italic, underline, reversed, dim
fg = "#fbf1c7"
bg = "#504945"
bold = true

[theme.glyphs]                 # state is always shown by glyphs as well as color
focused = "▸"
marked  = "●"
flagged = "✗"
```

The roles are `normal`, `directory`, `symlink`, `executable`, `special`, `marked`, `flagged`, `error`, `changed`, `header`, `dim`, `warning`, `conflict`, `success`, `matched`, `status`, `point`, and `prompt`.

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
