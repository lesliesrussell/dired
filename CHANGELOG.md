# Changelog

All notable changes are listed here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/). Entries mention the issue (bead) they
came from.

## [Unreleased]

### Added
- Open a terminal from a directory buffer (dired-z5y): `C-c t` opens a new terminal window there (configurable with `[terminal] command`, `{dir}` is the directory), and `C-c s` suspends dired and runs `$SHELL` in place.

## [0.1.0] - 2026-10-13

First release.

### Added

**The core: Emacs Dired in the terminal** (dired-7qs)
- Directory buffers with marks (`m` `u` `t` `U`), deletion flags and a preview of every operation before it runs.
- Copy, move, delete (to the trash), `chmod`, and create directory, each with a conflict-resolution preview and best-effort undo from a journal.
- Marking by regular expression (`% m` `% d` `% u`), renaming by pattern (`% r`), and editing names like text with Wdired (`C-x C-q`).
- Shell commands on files without a shell (`!`, `&`), an opt-in shell mode, and content search (`A`) that jumps to the match.
- Inline subdirectories (`i`, `$`), sorting, filtering, hidden-file toggle, buffers, incremental search, and a minibuffer with history and completion.
- Asynchronous listing and metadata, so large directories open at once.

**Windows** (dired-ta2)
- Emacs-style windows: split below/right, other window, delete, `C-x 4` commands, and a "dwim" copy/move destination that offers the directory in the other window.

**Looks**
- Seven built-in themes, per-role styles and glyphs, live `cycle-theme`, and a commented example config (dired-7yu).
- Three UI styles, `rich`, `clean` and `minimal`, with per-feature overrides (`[display.ui]`): rounded framed windows, icons, scrollbar, colored size and age columns, a segmented mode line, and a floating completion popup (dired-p2d).
- `load-theme` and `load-style` prompts with completion, next to `cycle-theme` and `cycle-style` (dired-2hn, dired-5xb).

**Working with files**
- Open with (`O`): pick a program for the marked files from your `[[open_with]]` entries or the built-ins (dired-0cz, dired-vmx).
- Copy names (`w`) or full paths (`W`) to the clipboard, via the terminal's OSC 52 escape and the platform clipboard tool (dired-vhh).
- Mark, flag or unmark by condition (`% M` `% D` `% U`): `size>10M age>30d type=file ext=log name~re` (dired-tlu).
- Jump list (`J`): bookmarks (`C-x r m`, `C-x r d`) and your most-used directories ranked by frequency and recency; `C-x 4 J` opens the target in the other window (dired-s23, dired-2fs).
- File view (`v`): a read-only window with the start of the file, a hex dump for binaries, or a directory listing; scroll it from the directory with `C-M-v` and `C-M-b` (dired-6xv).
- Git state: a two-letter status column, directories rolled up, and `git:branch*↑↓` in the mode line, read in the background (dired-99k).

**Minibuffer**
- `C-n` / `C-p` and the arrow keys select among completion candidates, in `M-x`, buffer and theme prompts, open-with, the jump picker, and path prompts after `TAB` (dired-t1z, dired-bmd).

**Project**
- Makefile with build, test, lint, install and uninstall targets (dired-7gc, dired-9m8), the `dired(1)` man page (dired-ji3), an exhaustive README (dired-1wy), and dual MIT/Apache-2.0 licensing with CI and dependency auditing (dired-fjh).

### Fixed
- Directory listings and the git column are refreshed when an editor or pager launched from dired exits (dired-blw).
- A mistake in `[display.ui]` no longer resets the rest of `[display]`, and several small rendering issues found in review (dired-mv2).

[0.1.0]: https://github.com/lesliesrussell/dired/releases/tag/v0.1.0
