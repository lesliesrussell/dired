<!-- dired-7qs -->
<!-- dired-1wy -->
<!-- dired-fjh -->
# dired

[![CI](https://github.com/lesliesrussell/dired/actions/workflows/ci.yml/badge.svg)](https://github.com/lesliesrussell/dired/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A terminal file manager that works like Emacs Dired. Each directory is shown as a list of lines, one per file. You move a cursor (called **point**) up and down, **mark** the files you want, and then run a command on all of them at once: copy, move, delete, rename, chmod, or run any program. Nothing destructive happens without a preview that you confirm.

You don't need to know Emacs to use it. This README explains everything from scratch.

**Requirements:** a Rust toolchain ([rustup](https://rustup.rs)) to build, on Linux or macOS. [ripgrep](https://github.com/BurntSushi/ripgrep) is optional (content search with `A`).

```sh
git clone https://github.com/lesliesrussell/dired && cd dired
make install                    # build and install to /usr/local/bin/dired
make install PREFIX=~/.local    # or install somewhere you own (no sudo)
dired                           # open the current directory
dired ~/Downloads               # open a specific directory
dired ~/notes/todo.md           # open its directory with point on that file
dired --check-config            # validate your config file and exit
```

Inside dired, press `?` for a list of every key and command, and `C-x C-c` to quit. After `make install`, `man dired` shows the full manual (the source is [`man/dired.1`](man/dired.1); preview it without installing using `man ./man/dired.1`).

---

## Contents

1. [Reading key names](#1-reading-key-names)
2. [The screen](#2-the-screen)
3. [Moving around](#3-moving-around)
4. [Opening files and directories](#4-opening-files-and-directories)
5. [Marks: choosing what a command acts on](#5-marks-choosing-what-a-command-acts-on)
6. [Marking by pattern (`% m`) and regular expressions](#6-marking-by-pattern--m-and-regular-expressions)
7. [Deleting files](#7-deleting-files)
8. [Copying and moving](#8-copying-and-moving)
9. [The preview buffer and conflicts](#9-the-preview-buffer-and-conflicts)
10. [Renaming: one file, many files, or by pattern](#10-renaming-one-file-many-files-or-by-pattern)
11. [Other file operations](#11-other-file-operations)
12. [Running programs on files (`!` and `&`)](#12-running-programs-on-files--and-)
13. [Searching inside files (`A`)](#13-searching-inside-files-a)
14. [Viewing subdirectories inline (`i` and `$`)](#14-viewing-subdirectories-inline-i-and-)
15. [Sorting, filtering, and hidden files](#15-sorting-filtering-and-hidden-files)
16. [Buffers](#16-buffers)
17. [Windows: several buffers on screen](#17-windows-several-buffers-on-screen)
18. [The minibuffer (prompts)](#18-the-minibuffer-prompts)
19. [Cancelling with `C-g`, and undo](#19-cancelling-with-c-g-and-undo)
20. [Worked examples](#20-worked-examples)
21. [Complete key reference](#21-complete-key-reference)
22. [Configuration](#configuration)

---

## 1. Reading key names

Keys are written the way Emacs writes them:

| Written | Means |
|---|---|
| `C-x` | Hold **Control** and press `x` |
| `M-x` | Hold **Meta** (Alt/Option) and press `x`, or press **ESC** and then `x` |
| `C-x d` | Press `C-x`, release, then press `d` (a *key sequence*) |
| `% m` | Press `%`, then `m` |
| `RET` `TAB` `DEL` `SPC` `ESC` | Enter, Tab, Backspace, Space, Escape |
| `<up>` `<down>` `<prior>` `<next>` | Arrow keys, Page Up, Page Down |

Uppercase letters are separate commands: `C` (Shift+c) copies, while `c` does nothing.

**macOS tip:** if Option+key types special characters instead of acting as Meta, either press `ESC` and then the key (`ESC x` is the same as `M-x`), or turn on "Use Option as Meta key" in your terminal's settings.

When you press a prefix key such as `C-x` or `%`, the status line shows what can follow it, for example `%- m:mark-files-regexp d:flag-files-regexp …`. Press `C-g` to abandon a half-typed sequence.

## 2. The screen

```
  /home/ana/photos:                                        ← header: the directory
 >                                     ../
    drwxr-xr-x     96 2026-09-21 09:10  2025/              ← a directory
 *  -rw-r--r--   3.4M 2026-09-22 18:44  IMG_0001.JPG       ← marked
 D  -rw-r--r--   3.1M 2026-09-22 18:45  IMG_0002.JPG       ← flagged for deletion
 D! -rw-r--r--   3.0M 2026-09-22 18:46  locked.JPG  ! trash: permission denied
    lrwxr-xr-x     11 2026-09-01 08:00  latest@ -> IMG_0001.JPG
    -rwxr-xr-x    812 2026-08-30 10:00  resize.sh*
 photos/  marked:1 flagged:2  [done]  filter:-  sort:name  ← status line
                                                           ← echo area / prompt
```

Each line starts with up to three state characters:

| Column | Character | Meaning |
|---|---|---|
| 1 | `>` | This is **point**, the line the cursor is on (it is also highlighted) |
| 2 | `*` | **Marked**: batch commands will act on it |
| 2 | `D` | **Flagged for deletion**: `x` will delete it (after a preview) |
| 3 | `!` | The last operation on this file **failed**; the reason is shown at the end of the line |
| 3 | `~` | The name has been **edited** but not yet saved (Wdired mode, section 10) |

State is always shown with these characters as well as colors, so it works on monochrome terminals. You can change the characters in your config.

Next come permissions, size, and modification time. On a narrow terminal these columns are hidden (permissions first, then time, then size) before names get cut off.

After the name comes a **type indicator**:

| Indicator | Meaning |
|---|---|
| `/` | directory |
| `@` | symbolic link (followed by `-> target`) |
| `@/` | symbolic link to a directory |
| `*` | executable file |
| `\|` | named pipe (FIFO) |
| `=` | socket |

The first line, `..`, is the parent directory. Visiting it goes up one level. It can never be marked, deleted, or renamed.

The **status line** shows:

| Field | Meaning |
|---|---|
| `photos/` | buffer name |
| `marked:1 flagged:2` | how many entries are marked and flagged in this buffer (hidden ones included) |
| `[done]` | loading progress: `scanning 12000` (reading names), `stat 3000/12000` (reading sizes and dates), `done`, or `error: …` |
| `filter:-` | the active filter (section 15), or `-` for none |
| `sort:name` | the sort order |
| `hidden:off` | only shown when dotfiles are hidden |
| `WDIRED (2 edited)` | only shown while editing names |

When the screen is split into several windows (section 17), each window has its own status line (also called the *mode line*) directly below it. The selected window's is highlighted; the others are dimmer.

The **echo area** at the bottom shows messages and errors, and turns into the **prompt** when a command asks you something.

Large directories open immediately. Names appear first, and sizes and dates fill in a moment later. You can move around and run commands while loading continues.

## 3. Moving around

| Keys | Moves |
|---|---|
| `C-n` `↓` `n` `SPC` | down one line |
| `C-p` `↑` `p` | up one line |
| `M-<` `<home>` | first line |
| `M->` `<end>` | last line |
| `C-v` `<next>` | down one screen |
| `M-v` `<prior>` | up one screen |
| `j` | jump to a file by name |
| `C-s` / `C-r` | incremental search forward / backward |

**`j` (goto file)** asks `Goto file:`. Type any part of a name and press `RET`. Point jumps to the next entry whose name *contains* that text, ignoring case, wrapping around at the end. For example, `j` then `read` then `RET` lands on `README.md`.

**`C-s` (incremental search)** moves point *while you type*:

1. Press `C-s`, then type `inv`. Point jumps to the first name containing `inv`, such as `invoice-march.pdf`.
2. Press `C-s` again to go to the next match, or `C-r` to go to the previous one.
3. Press `RET` to stay where you are, or `C-g` to jump back to where you started.

Search ignores case unless you type an uppercase letter ("smart case"). If nothing matches, the echo area says `Failing I-search`. Pressing `C-s` with nothing typed reuses your previous search.

## 4. Opening files and directories

| Keys | Does |
|---|---|
| `RET` `f` `e` | Visit the entry at point |
| `^` | Go to the parent directory (point lands on the directory you came from) |
| `O` | **Open with…**: pick a program for the marked files, else the file at point (see below) |
| `o` | Open the directory at point in the **other window** (section 17); files open as with `RET` |
| `C-x d` | Ask for a directory path and open it |
| `C-x C-f` | Ask for any path: directories open in dired, files are visited |
| `g` | Reload the listing from disk |

**Visiting a directory** opens it in a new buffer. If it is already open, dired switches to that buffer instead of opening a second copy.

**Visiting a file** runs an *opener*, chosen in this order:

1. The first `[[opener]]` rule in your config whose `extensions` match. For example, images go to `open` and code goes to `vim +LINE`.
2. Otherwise `$VISUAL` or `$EDITOR`, run inside the terminal. dired steps aside while the editor runs and comes back when you quit it.
3. Otherwise the system opener: `open` on macOS, `xdg-open` on Linux.

**Open with (`O`)** asks which program to use on the marked files (or the file at point when nothing is marked), instead of the automatic choice above. The prompt completes over a list of candidates: type a few letters (`neo` finds `Neovim`), `TAB` or `RET` takes the best match. Candidates are your `[[open_with]]` entries, followed by two built-ins: **System default** (`open` / `xdg-open`) and **$EDITOR** (when `$VISUAL` or `$EDITOR` is set). Add your own in the config file:

```toml
[[open_with]]
name       = "Preview"                       # shown in the prompt
command    = ["open", "-a", "Preview", "{file}"]
extensions = ["png", "jpg", "pdf"]           # optional; omit to offer it for every entry

[[open_with]]
name     = "Neovim"
command  = ["nvim", "{file}"]
terminal = true                              # suspend dired while it runs
```

A candidate with `extensions` is only offered when **every** selected file has one of those extensions (case-insensitive).

With several files selected, `{files}` in a command expands to all of them and the program runs **once** (`["nvim", "-p", "{files}"]`); a command that only has `{file}` runs **once per file**, one after another for terminal programs. The built-in **$EDITOR** uses `{files}`; **System default** runs once per file. Arguments are passed as argv, never through a shell. Rebind the key with `command = "dired-open-with"` in a `[[keymap]]` entry.

**`g` (refresh)** re-reads the directory in the background. Point stays on the same *file*, even if new files now appear above it. Marks and flags on files that still exist are kept.

## 5. Marks: choosing what a command acts on

This is the core idea of dired:

> **Commands act on the marked files. If nothing is marked, they act on the file at point.**

So to copy one file, put point on it and press `C`. To copy ten files, mark them and press `C`. The same rule applies to `R`, `M`, `!`, `&`, `% r`, and search with `A`.

| Keys | Does |
|---|---|
| `m` | Mark the file at point and move down |
| `u` | Remove the mark **or** deletion flag at point and move down (also clears a `!` error) |
| `DEL` | Move up and remove the mark there (undo your last `m`) |
| `U` | Remove **all** marks in this buffer. Deletion flags are **kept** |
| `t` | Toggle: every marked visible file becomes unmarked and vice versa. Flagged files are left alone |
| `% m` | Mark files whose names match a pattern (section 6) |
| `% u` | Unmark files whose names match a pattern |
| `d` | Flag the file at point for deletion and move down |
| `% d` | Flag files whose names match a pattern |

A line has exactly one state: unmarked, marked (`*`), or flagged (`D`). Marking a flagged line replaces the `D` with `*`, and flagging a marked line replaces the `*` with `D`.

**Marks are tied to the file, not the line number.** They survive:
- **refreshing** with `g`
- **re-sorting** with `s`
- **hiding dotfiles** and **filtering**: a hidden marked file keeps its mark and gets it back when shown again
- **renaming**: the mark follows the file to its new name

Marks belong to one buffer. Two directory buffers have completely separate marks, sort order, filter, and point.

**Hidden marked files are not acted on.** Batch commands only use marked files you can currently see. If you mark files, then filter them out and press `C`, only the visible marked files are copied. The status line's `marked:` count includes hidden ones, so you can tell when this is happening. Deletion (`x`) is the one exception, covered in section 7.

## 6. Marking by pattern (`% m`) and regular expressions

`% m` marks every file whose name matches a **regular expression** (regexp): a small language for describing text patterns.

**How to use it**

1. Press `%`, then `m`. The prompt says `Mark files (regexp):`.
2. Type a pattern, for example `\.pdf$`.
3. Press `RET`. Every matching file gets a `*`, and the echo area reports `N matching file(s) marked`.

**Exactly what is matched**

- **Only the file name** is matched, never the directory path. `^src` means "name starts with src".
- **Only visible entries.** Hidden dotfiles (when hidden) and entries removed by a filter are skipped. `..` is never marked. Files shown inline with `i` are included.
- **A match anywhere counts.** `pdf` matches `report.pdf` and also `pdf-notes.txt`. Use `^` (start) and `$` (end) to be precise.
- **Case matters.** `jpg` does not match `PHOTO.JPG`. Start the pattern with `(?i)` to ignore case.
- **Existing marks are kept.** Running `% m` twice adds to the set, so you can build a selection in several steps.
- **Matches become marked even if they were flagged.** A matching `D` turns into `*`.
- **Mistakes are harmless.** An invalid pattern shows `invalid regexp: …` and changes nothing.
- Names with control characters (such as a newline) are shown and matched in escaped form, like `\n`.

The same rules apply to `% d` (flag matches for deletion) and `% u` (unmark matches).

### Regular expressions in five minutes

Most characters match themselves. `cat` matches any name containing "cat". These characters are special:

| Pattern | Matches | Example | Matches | Doesn't match |
|---|---|---|---|---|
| `.` | any one character | `a.c` | `abc`, `a-c` | `ac` |
| `\.` | a literal dot | `\.txt` | `a.txt` | `atxt` |
| `^` | start of the name | `^draft` | `draft-2.md` | `old-draft.md` |
| `$` | end of the name | `\.md$` | `notes.md` | `notes.md.bak` |
| `*` | the previous item, 0 or more times | `ab*c` | `ac`, `abbbc` | `adc` |
| `+` | the previous item, 1 or more times | `ab+c` | `abc`, `abbc` | `ac` |
| `?` | the previous item, optional | `colou?r` | `color`, `colour` | `colr` |
| `[abc]` | one of the listed characters | `[0-9]` | any digit | |
| `[^abc]` | any character *not* listed | `^[^.]+$` | `Makefile` | `a.txt` |
| `\d` `\w` `\s` | a digit / letter-digit-underscore / whitespace | `\d{4}` | `2026` | `26` |
| `{n}` `{n,m}` | exactly n / between n and m times | `\d{2,3}` | `42`, `123` | `7` |
| `a\|b` | either side | `\.(jpg\|png)$` | `x.jpg`, `y.png` | `z.gif` |
| `( … )` | a group | `(ab)+` | `abab` | |
| `(?i)` | at the very start: ignore case | `(?i)^readme` | `README.md`, `readme` | |

To match a special character literally, put `\` before it: `\.` `\(` `\[` `\+` `\?` `\$` `\^`.

The syntax is Rust's [`regex`](https://docs.rs/regex/latest/regex/#syntax) crate. It is like most other regexp flavors, but has no backreferences or lookaround.

### Shell globs are not regexps

If you are used to `*.txt` in the shell, translate it:

| Shell glob | Regexp |
|---|---|
| `*.txt` | `\.txt$` |
| `*.jpg` or `*.JPG` | `(?i)\.jpg$` |
| `IMG_*` | `^IMG_` |
| `*draft*` | `draft` |
| `file?.log` | `^file.\.log$` |
| `*.{jpg,png}` | `\.(jpg\|png)$` |
| `[0-9]*` | `^[0-9]` |

Typing `*.txt` into `% m` shows an error (`*` with nothing before it) and marks nothing.

### Pattern cookbook

| Goal | Pattern |
|---|---|
| All PDFs | `\.pdf$` |
| All images, any case | `(?i)\.(jpe?g\|png\|gif\|webp\|heic)$` |
| Files starting with `test_` | `^test_` |
| Backup and temp files | `(~\|\.bak\|\.tmp\|\.swp)$` |
| Names containing a date like 2026-09-27 | `\d{4}-\d{2}-\d{2}` |
| Screenshots from 2025 | `^Screenshot 2025` |
| Files with no extension | `^[^.]+$` |
| Names with a space | ` ` (a single space) |
| Log files numbered 1–9 | `^app\.log\.[1-9]$` |
| Everything | `.` |
| Everything except Markdown | mark all with `.`, then `% u` with `\.md$` |

### Step-by-step example

The directory contains:

```
  report-2025.pdf   report-2026.pdf   Summary.PDF   notes.txt   report-draft.txt
```

- `% m` `\.pdf$` marks `report-2025.pdf` and `report-2026.pdf`, but not `Summary.PDF` (case).
- `% m` `(?i)\.pdf$` also marks `Summary.PDF`.
- `% u` `2025` then unmarks `report-2025.pdf`.
- Now `R` `archive/` moves the two remaining marked PDFs, and `U` clears the marks when you're done.

## 7. Deleting files

Deleting is always two steps, so you can never delete by accident:

1. **Flag** files with `d` (at point) or `% d` (by pattern). Flagged lines show `D`, and nothing is deleted yet. `u` on a flagged line removes the flag.
2. Press **`x`**. A **preview buffer** opens listing every flagged file, the deletion method, the count, and warnings such as "3 directories will be moved to trash with their contents".
3. In the preview, press **`x`** to go ahead, or **`q`** / **`C-g`** to back out with nothing changed.

`x` includes **every** flagged file in the buffer, even ones currently hidden or filtered out, so the preview always matches the `flagged:` count in the status line.

**Files go to the system trash** (macOS Trash, the freedesktop.org trash on Linux, or the Recycle Bin), so you can restore them from there.

**Permanent deletion** skips the trash:
- In the deletion preview, press **`D`** to switch to permanent deletion; press `D` again to switch back.
- Or run `M-x dired-do-flagged-delete-permanently`.
- Either way you must **type `yes`** at a prompt that states how many items and directories will be removed. Directories are removed recursively, and symbolic links are deleted, not followed.

If trash isn't available (for example on some network drives), the preview says so and won't run. Press `D` to delete permanently instead, or set `trash_fallback = "permanent"` in your config.

**When something fails**, the others still go ahead. The preview turns into a results list with each file marked `done`, `skipped`, or `FAILED: reason`. Back in the directory, deleted files disappear. Failed ones keep their `D` and get a `!` with the error, so you can fix the cause and press `x` again.

## 8. Copying and moving

| Keys | Does |
|---|---|
| `C` | Copy the marked files (or the one at point) |
| `R` | Move or rename the marked files (or the one at point) |

dired asks for a destination. It is pre-filled with the current directory or, when another window shows a different directory, with *that* directory (see "Copying between windows" in section 17). Edit it and press `RET`. `TAB` completes paths, and `~` means your home directory.

**How the destination is interpreted**

| You type | Result |
|---|---|
| An **existing directory** | Every file goes *into* it under its own name: `C` `~/backup/` gives `~/backup/a.txt` |
| A path that **doesn't exist yet**, with **one** file | The file is copied or moved *to that name*: `R` `notes-old.txt` renames |
| A path that doesn't exist, with **several** files | Error: several files need an existing directory |

Relative paths are relative to the directory you're in: `R` `../` moves up one level.

A **preview** always opens before anything happens (section 9). It shows every `source → destination` pair and any problems.

**Directories are copied recursively.** Symbolic links inside them are copied as links, not followed.

**Moving between disks.** When source and destination are on different filesystems (for example to a USB drive), a move becomes *copy, then delete the original*. The preview warns `cross-device: non-atomic`. The original is only deleted after its copy fully succeeds. If the copy fails, the original stays where it was.

## 9. The preview buffer and conflicts

Every copy, move, rename, chmod, and delete goes through a preview. The preview opens in a new window below the directory it came from, so that directory stays visible. The window closes when the preview is closed (`q`, or `C-g` before running). If the screen is too small to split, the preview takes over the window instead:

```
 Operation preview: Copy
 Items: 3 (0 directories) · conflicts: 1 · undecided: 1 · skipped: 0
 x/C-c C-c execute · q/C-g cancel · o overwrite · s skip · r rename · a ask · O overwrite all · S skip all
 Blocked: 1 conflict(s) need a policy (o overwrite, s skip, r rename, O overwrite all, S skip all)

>/home/ana/a.txt → /backup/a.txt  (CONFLICT: destination exists [ask])
 /home/ana/b.txt → /backup/b.txt  (ok)
 /home/ana/c.txt → /backup/c.txt  (ok)
```

Move between items with `n` / `p` (or `C-n` / `C-p`). While any conflict is undecided (`[ask]`), **nothing can run**. Pressing `x` just tells you what still needs a decision.

| Key | Does |
|---|---|
| `o` | Overwrite the existing destination (this item) |
| `s` | Skip this item |
| `r` | Choose a different destination name for this item |
| `a` | Back to undecided |
| `O` | Overwrite every undecided conflict where that's allowed |
| `S` | Skip every undecided conflict |
| `x` `y` `C-c C-c` | Run it |
| `q` `C-g` | Cancel; nothing is changed |
| `D` | (deletion previews only) switch between trash and permanent |

**Kinds of conflict**

| Conflict | Meaning | Allowed choices |
|---|---|---|
| destination exists | Something already has that name | overwrite, skip, rename |
| duplicate destination | Two sources would land on the same name | skip, rename |
| destination is inside source | Copying a folder into itself, even through a symlink | skip |
| destination contains the source | Overwriting the folder the source lives in | skip |
| source and destination are the same | Nothing to do | skip |
| inaccessible | The source can't be read (missing, no permission) | skip |

**Overwriting is protected.** If you chose overwrite for anything, dired asks you to **type `yes`**. The prompt says how many destinations are replaced and how many are directories replaced recursively. The old destination is set aside first and only removed once the new copy succeeds. If the copy fails, the old one is put back.

After running, the preview becomes a **result list**: `done`, `skipped`, or `FAILED: reason` per item, plus a summary. Press `q` to return to the directory, which has refreshed itself.

Every completed operation is saved to a journal, so it can usually be undone with `C-x u` (section 19).

## 10. Renaming: one file, many files, or by pattern

### One file

Put point on it, press `R`, type the new name, then `RET`, and confirm the preview with `x`.

### Many files at once: Wdired (edit names like text)

`C-x C-q` makes the file names **editable**, like a text document:

1. Press `C-x C-q`. The status line shows `WDIRED`.
2. Move between lines with `C-n` / `C-p` / arrows. On a line, type to insert text. The cursor sits at the end of the name.

   | Key | Does |
   |---|---|
   | letters, digits, symbols | insert at the cursor |
   | `DEL` | delete the character before the cursor |
   | `C-d` `<delete>` | delete the character under the cursor |
   | `C-k` | delete from the cursor to the end of the name |
   | `C-a` / `C-e` | start / end of the name |
   | `C-f` `C-b` `←` `→` | move within the name |

   Only the name can be edited. The other columns are protected. `..` and names that can't be shown exactly as text (with control characters or invalid UTF-8) are read-only.
3. Changed lines show `~`.
4. Press **`C-c C-c`** (or `C-x C-q` again) to save. A preview lists every rename. Press `x` to apply.
5. Or press **`C-c C-k`** to throw away all edits and restore the real names.

Before anything happens, dired checks the whole set of renames:
- two files can't get the same name
- a new name can't clash with a file that isn't being renamed
- names can't be empty, `.`, or `..`, and can't contain `/`

If there's a problem, the preview lists it, **nothing is renamed**, and you stay in edit mode to fix it.

Renames are all-or-nothing: you can't skip individual items in a rename preview; change the names instead.

**Swaps and chains work.** Renaming `a → b` and `b → a` at the same time exchanges the two files safely using a temporary name, as does `1 → 2, 2 → 3, 3 → 1`.

**Marks and flags follow** the renamed files.

### By pattern: `% r`

`% r` renames the marked files (or the one at point) using a search-and-replace on each name:

1. Mark the files (or don't, to rename just the one at point).
2. Press `% r`. At `Rename from (regexp):` type the pattern to find.
3. At `Rename … to:` type the replacement. `$1`, `$2`, … insert what the numbered groups `( … )` matched, and `$0` inserts the whole match. Write `${1}` when a letter or digit follows, as in `${1}x`.
4. Check the preview and press `x`.

Only the **first** match in each name is replaced. Selected files whose names don't match are left alone.

| From | To | Before | After |
|---|---|---|---|
| `\.jpeg$` | `.jpg` | `cat.jpeg` | `cat.jpg` |
| `^IMG_(\d+)` | `photo-$1` | `IMG_0042.JPG` | `photo-0042.JPG` |
| `(?i)\.JPG$` | `.jpg` | `photo-0042.JPG` | `photo-0042.jpg` |
| `^(\d{4})(\d{2})(\d{2})` | `$1-$2-$3` | `20260927 notes.md` | `2026-09-27 notes.md` |
| `^(.*) - (.*)\.mp3$` | `$2 - $1.mp3` | `Artist - Song.mp3` | `Song - Artist.mp3` |
| ` ` | `_` | `my file.txt` | `my_file.txt` (first space only) |
| `^` | `old-` | `plan.md` | `old-plan.md` (adds a prefix) |
| `(\.[^.]+)$` | `.bak$1` | `data.csv` | `data.bak.csv` |

To replace *every* space, use Wdired instead, or run `% r` again until nothing matches.

## 11. Other file operations

| Keys | Does |
|---|---|
| `+` | Create a directory. Type a name, relative to here or absolute. No preview (it's harmless), and `C-x u` removes it again |
| `M` | Change permissions of the marked files. Type an octal mode such as `644` (rw-r--r--), `755` (rwxr-xr-x), or `600` (rw-------). A preview shows each file first |
| `w` | Copy the **names** of the marked files (else the file at point) to the clipboard, one per line |
| `W` | Copy the **full paths** the same way |

Copying uses the terminal's clipboard escape code (OSC 52), which works over ssh and in iTerm2, kitty, WezTerm, Alacritty, foot, and tmux with `set-clipboard on`. Outside an ssh session dired also hands the text to `pbcopy`, `wl-copy`, `xclip`, or `xsel` when one is installed, which covers macOS Terminal.app.

## 12. Running programs on files (`!` and `&`)

`!` asks for a command and runs it on the marked files (or the one at point), from the current directory.

| Command you type | What runs, for marked `a.txt` and `b.txt` |
|---|---|
| `wc -l` | `wc -l a.txt b.txt` (files are **added at the end**) |
| `tar czf backup.tgz *` | `tar czf backup.tgz a.txt b.txt` (a lone `*` is **where the files go**) |
| `gzip -k ?` | `gzip -k a.txt`, then `gzip -k b.txt` (a lone `?` means **once per file**) |
| `cp ? backup/` | `cp a.txt backup/`, then `cp b.txt backup/` |
| `cp ? ?.orig` | `cp a.txt ?.orig`, then `cp b.txt ?.orig`: only the *lone* `?` is replaced, `?.orig` stays literal |

- `*` and `?` only count when they stand alone as a whole word.
- Quote arguments with spaces: `grep -n "to do"`.
- `!` runs in the background. Output goes to the `*Shell Command Output*` buffer (switch with `b`). If the output is a single line, it's also shown in the echo area, like `2 a.txt [Process finished with exit code 0]`.
- `&` runs the same way but switches to an `*Async Shell Command*` buffer immediately and shows output as it arrives. Press `C-c C-c` or `C-g` there to stop the program.

**It's safe with odd file names.** Files are passed to the program directly as separate arguments; no shell ever sees them. A file named `my file; rm -rf ~` is just one argument, with the space and semicolon intact. A file whose name starts with `-` is passed as `./-name`, so programs can't mistake it for an option. Because there's no shell, pipes (`|`), redirection (`>`), and `$VARIABLES` in the command are passed literally too.

**Shell mode (opt-in)** is for when you do want pipes. Run `M-x dired-do-shell-command-in-shell`, type something like `cat * | sort | uniq -c`, and confirm by typing `yes`. The command runs with `/bin/sh -c`. The files are still passed separately, as `"$@"` (all files) or `"$1"` (with `?`), and are never pasted into the command text. The output buffer is labeled `[SHELL MODE]`.

## 13. Searching inside files (`A`)

`A` searches the **contents** of files using [ripgrep](https://github.com/BurntSushi/ripgrep) (`rg`, which must be installed):

1. Press `A`, type a regexp (same syntax as section 6), and press `RET`.
2. It searches the marked files and directories, or the whole current directory tree if nothing is marked.
3. A `*Search: …*` buffer lists matches as `path:line: text`.
4. Move to a match and press `RET` to open that file **at that line**, using your opener or `$EDITOR +LINE`.

Examples: `TODO`, `fn main`, `(?i)password`, `\bdeprecated\b`.

## 14. Viewing subdirectories inline (`i` and `$`)

- **`i`** on a directory shows its contents right underneath it, indented, without leaving the current buffer. Press `i` on a subdirectory inside that to go deeper.
- **`$`** on an expanded directory collapses it. On a file inside an expanded directory, `$` collapses that directory and moves point to it.

Files shown this way can be marked and acted on like any others. `% m`, `C`, `R`, and `x` all work across levels.

Symbolic links to directories are **not** expanded, to avoid loops. Set `follow_symlinks_on_insert = true` to allow it; dired still refuses links that point back up to a parent.

## 15. Sorting, filtering, and hidden files

| Keys | Does |
|---|---|
| `s` | Cycle sort: name → natural → time → size → extension → name … |
| `/` | Filter: show only names containing some text (ignores case). Empty input clears it |
| `C-x M-o` | Show or hide dotfiles (names starting with `.`) |

| Sort | Order |
|---|---|
| name | alphabetical, byte order (`B` before `a`) |
| natural | numbers compared as numbers, ignoring case: `file2` before `file10` |
| time | newest first |
| size | largest first |
| extension | by the part after the last dot |

Directories are listed first by default. Commands available through `M-x`:
- `dired-sort-by-name` and the other `dired-sort-by-…` commands
- `dired-sort-reverse`
- `dired-sort-toggle-dirs-first`

**Filtering** is a quick way to narrow a big directory, for example `/` `invoice` `RET`. It combines well with marks: filter, then `t` to mark everything visible, then clear the filter.

Sort, filter, and hidden-file settings belong to each buffer. Marks on entries you've filtered out or hidden are kept.

## 16. Buffers

Everything dired shows is a **buffer**:
- each directory you open
- previews and results
- the output of `!` and `&`
- search results, help (`?`), and `*Diagnostics*` (configuration problems)

| Keys | Does |
|---|---|
| `b` (or `C-x b`) | Switch buffer. Type a few letters of its name; matching is fuzzy, so `dwn` finds `Downloads/`. `RET` picks the top match, `TAB` completes |
| `q` | Close the current buffer. Closing the last one quits dired. Other windows showing it switch to another buffer |
| `C-x k` | Close a buffer by name (`RET` for the current one). Asks `y`/`n` first if it has unsaved Wdired edits, an unfinished preview, or a running program |
| `C-x C-c` | Quit dired |

Each directory buffer remembers its own point, marks, flags, sort, filter, hidden-file setting, expanded subdirectories, and edits.

## 17. Windows: several buffers on screen

As in Emacs, the screen can be divided into **windows**, each showing a buffer. The most common use is two directories side by side, so you can copy and move between them.

| Keys | Command | Does |
|---|---|---|
| `C-x 2` | `split-window-below` | Split the selected window into two, one above the other |
| `C-x 3` | `split-window-right` | Split the selected window into two, side by side |
| `C-x o` | `other-window` | Select the next window (in layout order: windows split from the same window come one after another; it wraps around) |
| `C-x 0` | `delete-window` | Close the selected window. Its buffer stays open; `b` brings it back |
| `C-x 1` | `delete-other-windows` | Keep only the selected window |
| `C-x 4 d` | `dired-other-window` | Open a directory in the other window |
| `C-x 4 f` `C-x 4 C-f` | `find-file-other-window` | Open any path in the other window |
| `C-x 4 b` | `switch-to-buffer-other-window` | Show a buffer in the other window |
| `o` | `dired-find-file-other-window` | In a directory buffer: open the directory at point (or `..`) in the other window |

**The "other window"** is the next window after the selected one. If there is only one window, the `C-x 4` commands and `o` split it first: side by side when it is at least 100 columns wide (or too short to stack), otherwise one above the other. They then select the other window and open there. `C-x 4 f` on a file just opens the file, without splitting.

**What the screen shows**
- Each window has its own status line (mode line). The selected window's is highlighted; the others use the dimmer `inactive` theme style.
- Side-by-side windows are separated by a vertical rule `│`.
- Only the selected window highlights the point line and shows the cursor. The others mark their point with the `>` glyph.

```
  /home/ana/src:                      │  /home/ana/backup:
                           ../        │>                           ../
 *  -rw-r--r--  2 …  one.txt          │    -rw-r--r--  9 …  old.txt
>   -rw-r--r--  2 …  two.txt          │
 src/  marked:1 flagged:0  [done] …   │ backup/  marked:0 flagged:0 …
```

**What belongs to a window and what belongs to a buffer**
- **Per window:** point and scroll position. Showing the same directory in two windows gives two independent cursors, like two bookmarks in one list.
- **Per buffer, shared by every window showing it:** marks, flags, sort order, filter, hidden-file setting, inserted subdirectories, and Wdired edits. If you mark a file in one window, it appears marked in the other.

Keys always act on the selected window. A prompt belongs to the window it was started from, even if something else opens meanwhile.

### Copying between windows

When another window shows a different directory, `C` (copy) and `R` (move) pre-fill the destination with **that** directory. This is Emacs's `dired-dwim-target` ("do what I mean"). With `src/` on the left and `backup/` on the right:

1. In the left window, mark files with `m`.
2. Press `C`. The prompt already says `/home/ana/backup/`.
3. Press `RET`. The preview opens below the left window; press `x`.
4. The right window refreshes and shows the copies.

If several other windows show directories, the next one in `C-x o` order wins. To always start from the current directory instead, set `dwim_target = false` in the `[display]` section of your config.

### Previews, closing, and sizes

- Previews pop up in a new window below the directory they came from. The window closes when the preview is closed (`q`, `C-g` before running, or `q` after running). Moving away with `C-x o` or `b` leaves it open. If the window is too small to split, the preview replaces it instead. If a preview finishes planning while you are typing in a prompt, it waits in the background (`b` to switch to it) rather than interrupting you.
- Closing a buffer (`q`, `C-x k`) closes a popped-up window showing it. Any other window showing it switches to the most recent buffer that is not already visible.
- Windows split in half. The smallest window is 4 rows (including its mode line) by 20 columns; a split that would go below that is refused with "too small to split".
- `C-x 0` never closes the last window; `q` on the last buffer quits dired as usual.

## 18. The minibuffer (prompts)

Whenever dired asks for input, you're editing a one-line text field at the bottom of the screen:

| Keys | Does |
|---|---|
| `RET` | Accept |
| `C-g` | Cancel (nothing happens) |
| `TAB` | Complete. For paths it fills in as much as is unambiguous and lists the choices; for commands and buffers it takes the best match |
| `C-n` `↓` / `C-p` `↑` | In prompts with a candidate list (`M-x`, buffer switching, open-with, theme names, and path prompts after `TAB` has listed the matches): select the next / previous candidate, one line at a time. `RET` then accepts the highlighted one, even if what you typed also matches another exactly; in path prompts `TAB` inserts it and lists what is inside |
| `M-p` / `M-n` | Previous / next entry from the history of this kind of prompt (`↑` / `↓` also do this when there is no candidate list) |
| `C-a` `C-e` | Start / end of the line |
| `C-f` `C-b` `←` `→` | Move one character |
| `DEL` / `C-d` | Delete backward / forward |
| `C-k` | Delete to the end of the line |
| `C-u` | Delete to the start of the line |
| `C-w` `M-DEL` | Delete the previous word or path component |

History is kept separately for paths, regexps, shell commands, searches, and so on, and is saved between sessions.

**Starting a new path.** Path prompts are pre-filled (usually with the current directory). As in Emacs, you don't have to erase that first: typing `~/` or a second `/` starts over. For example, `/home/ana/src/~/backup` means `~/backup`, and `/home/ana/src//tmp` means `/tmp`. A `~` inside a name, as in `a~b`, is literal.

**`M-x`** runs any command by name. Type part of it (for example `sortsize` finds `dired-sort-by-size`) and press `RET`. Every command, including those without a key, is listed in the `?` help buffer.

## 19. Cancelling with `C-g`, and undo

`C-g` is "stop what I'm doing". It cancels the first thing that applies:

1. an open prompt (an interrupted `C-s` search also returns point to where it started)
2. a preview that hasn't run: it closes and nothing changes
3. a half-typed key sequence such as `C-x` or `%`
4. something running in the current buffer: loading a huge directory, a program started with `&`, or an operation partway through

**Undo** (`C-x u`, also `C-/` or `C-_`) reverses the most recent operation that can be reversed. Press it again for the one before.

| Operation | Undo |
|---|---|
| move / rename (including Wdired, swaps, and `% r`) | renames everything back |
| copy | moves the copies to the trash (not possible if the copy overwrote something) |
| create directory | removes it, if it's still empty |
| chmod | restores the previous permissions |
| trash | not automatic; restore from your system's trash |
| permanent delete | impossible |

Undo is best-effort. If files changed in the meantime, some steps may fail, and the echo area says which.

## 20. Worked examples

**Clean out log files**
1. `% d` `\.log$` `RET`: every `.log` file gets a `D`.
2. Unflag any you want to keep: move to it and press `u`.
3. `x` shows the preview. Press `x` again: they go to the trash.

**Move this year's photos into a folder**
1. `+` `2026` `RET` creates the folder.
2. `% m` `(?i)^IMG_2026.*\.(jpe?g|heic)$` `RET`.
3. `R` `2026/` `RET`, check the preview, then `x`.

**Give camera files readable names**
1. `% m` `^DSC` `RET`.
2. `% r` `^DSC_(\d+)` `RET` `holiday-$1` `RET`: `DSC_0412.JPG` becomes `holiday-0412.JPG`.
3. Check the preview, then `x`. Changed your mind? `C-x u`.

**Rename a few files by hand**
1. `C-x C-q`, then edit the names directly (e.g. `C-e`, `DEL` ×3, type `md`).
2. `C-c C-c`, then `x`.

**Back up a set of config files**
1. Mark them with `m`.
2. `C` `~/backup/` `RET`. If some already exist in the backup, press `O` (overwrite all), then `x`, then type `yes`.

**Archive the marked files**
- `!` `tar czf archive.tgz *` `RET`, or `!` `zip -r archive.zip *` `RET`.

**Count lines in every Rust file in a project**
- `% m` `\.rs$` `RET`, then `!` `wc -l` `RET`, then `b` `*Shell Command Output*` `RET` to read the result.

**Find where something is used, then edit it**
- `A` `parse_config` `RET`, move to a match, `RET` opens your editor at that line.

**Work with two directories side by side**
1. `dired ~/src`, then `C-x 4 d` `~/backup` `RET`: `~/backup` opens in a second window beside (or below) the first.
2. `C-x o` back to `~/src`, mark files with `m`, press `C`. The destination already reads `~/backup/`: `RET`, then `x`.
3. `C-x 1` returns to a single window; `b` still reaches both directories.

## 21. Complete key reference

**Directory buffers**

| Keys | Command | What it does |
|---|---|---|
| `RET` `f` `e` | `dired-visit` | Open directory / visit file |
| `O` | `dired-open-with` | Choose a program to open the marked files (else the file at point) |
| `w` / `W` | `dired-copy-name` / `dired-copy-path` | Copy names / full paths of the marked entries (else point) to the clipboard |
| `^` | `dired-up-directory` | Parent directory |
| `o` | `dired-find-file-other-window` | Open in the other window |
| `g` | `revert-buffer` | Reload, keeping point and marks |
| `n` `SPC` / `p` | `next-line` / `previous-line` | Move |
| `j` | `dired-goto-file` | Jump to a name |
| `m` | `dired-mark` | Mark and move down |
| `u` | `dired-unmark` | Unmark/unflag and move down |
| `DEL` | `dired-unmark-backward` | Move up and unmark |
| `U` | `dired-unmark-all-marks` | Clear all marks (keeps flags) |
| `t` | `dired-toggle-marks` | Invert marks on visible files |
| `d` | `dired-flag-file-deletion` | Flag for deletion and move down |
| `x` | `dired-do-flagged-delete` | Preview and delete flagged files |
| `C` | `dired-do-copy` | Copy |
| `R` | `dired-do-rename` | Move / rename |
| `M` | `dired-do-chmod` | Change permissions |
| `+` | `dired-create-directory` | New directory |
| `!` | `dired-do-shell-command` | Run a program on the files |
| `&` | `dired-do-async-shell-command` | Same, with live output |
| `% m` | `dired-mark-files-regexp` | Mark by pattern |
| `% u` | `dired-unmark-files-regexp` | Unmark by pattern |
| `% d` | `dired-flag-files-regexp` | Flag by pattern |
| `% r` `% R` | `dired-do-rename-regexp` | Rename by pattern |
| `A` | `dired-search-files-content` | Search contents |
| `i` | `dired-maybe-insert-subdir` | Expand a subdirectory inline |
| `$` | `dired-hide-subdir` | Collapse it |
| `s` | `dired-sort-toggle-or-edit` | Cycle sort |
| `/` | `dired-filter` | Filter by name |
| `C-x M-o` | `dired-toggle-hidden` | Show/hide dotfiles |
| `C-x C-q` | `wdired-toggle-edit` | Edit names as text |
| `b` | `switch-to-buffer` | Switch buffer |
| `?` `h` | `help-for-help` | List all keys and commands |
| `q` | `quit-window` | Close this buffer |

**Everywhere**

| Keys | Command | What it does |
|---|---|---|
| `C-n` `↓` / `C-p` `↑` | `next-line` / `previous-line` | Move |
| `M-<` / `M->` | `beginning-of-buffer` / `end-of-buffer` | First / last line |
| `C-v` / `M-v` | `scroll-up-command` / `scroll-down-command` | Page down / up |
| `C-s` / `C-r` | `isearch-forward` / `isearch-backward` | Incremental search |
| `C-x d` | `dired-open-directory` | Open a directory |
| `C-x C-f` | `dired-find-file` | Open any path |
| `C-x b` | `switch-to-buffer` | Switch buffer |
| `C-x k` | `buffer-kill` | Close a buffer |
| `C-x 2` / `C-x 3` | `split-window-below` / `split-window-right` | Split the window |
| `C-x o` | `other-window` | Next window |
| `C-x 0` / `C-x 1` | `delete-window` / `delete-other-windows` | Close this window / all others |
| `C-x 4 d` | `dired-other-window` | Open a directory in the other window |
| `C-x 4 f` `C-x 4 C-f` | `find-file-other-window` | Open a path in the other window |
| `C-x 4 b` | `switch-to-buffer-other-window` | Show a buffer in the other window |
| `C-x u` `C-/` `C-_` | `dired-undo` | Undo last operation |
| `M-x` | `execute-extended-command` | Run a command by name |
| `C-h m` `C-h b` `<f1>` | `help-for-help` | Help |
| `C-g` | `keyboard-quit` | Cancel |
| `C-x C-c` | `save-buffers-kill-terminal` | Quit |

**Wdired (editing names)**: `C-c C-c` / `C-x C-q` / `C-x C-s` save, `C-c C-k` abort; editing keys are listed in section 10.

**Previews**: `x` `y` `C-c C-c` run, `q` `C-g` cancel, `n` `p` move, `o` `s` `r` `a` `O` `S` resolve conflicts, `D` trash/permanent.

**Output and search buffers**: `RET` visits the location on the line, `C-c C-c` stops the running program, `q` closes.

**Commands without a key** (use `M-x`):

| Command | What it does |
|---|---|
| `dired-do-flagged-delete-permanently` | Delete flagged files, skipping the trash |
| `dired-do-shell-command-in-shell` | `!` through `/bin/sh` |
| `dired-sort-by-name`, `…-natural`, `…-time`, `…-size`, `…-extension` | Pick a sort order |
| `dired-sort-reverse` | Reverse the sort order |
| `dired-sort-toggle-dirs-first` | List directories first, or mixed in |
| `cycle-theme` | Try the next color theme |
| `load-theme` | Ask for a theme by name (`TAB` completes, `C-n` / `C-p` pick) and switch to it |
| `cycle-style` | Try the next UI style (rich, clean, minimal) |
| `load-style` | Ask for a UI style by name (`TAB` completes, `C-n` / `C-p` pick) and switch to it |
| `dired-reload-config` | Re-read the config file |
| `view-diagnostics`, `view-messages` | Configuration problems / message history |

You can bind any of these to keys in your config (next section).

---

## Configuration

`$XDG_CONFIG_HOME/dired/config.toml` (or `~/.config/dired/config.toml`, or `$DIRED_CONFIG`). Errors never stop the app; they are listed in the `*Diagnostics*` buffer, and `dired --check-config` reports them without starting the UI.

**[`examples/config.toml`](examples/config.toml)** is a fully commented tour of every option: themes, per-role styles, glyphs, key bindings to commands and external programs, openers, search, and safety. `make release` copies it to `target/release/examples/`, and `make install` installs it to `$(PREFIX)/share/dired/examples/`.

```sh
cp examples/config.toml ~/.config/dired/config.toml
```

### Styles

dired has three looks, chosen with `[display] style`:

| Style | Look |
|---|---|
| `clean` (default) | Airy: a rule under the header, a left gutter, wider columns, a two-tone mode line. No font requirements. |
| `rich` | Rounded titled frames around each window, column labels, file-type icons (Nerd Font), scrollbar on the border. |
| `minimal` | The compact layout, plus colored size/age columns, a scrollbar and a floating completion box. |

`M-x cycle-style` steps through them and `M-x load-style` picks one by name (the prompt starts on the active style); both last for the session, so you can try each one. Put `style = "..."` under `[display]` to make it permanent. Individual features can be forced on or off under `[display.ui]`:

```toml
[display]
style = "clean"

[display.ui]
borders       = true        # frames in any style
icons         = false       # icons need a Nerd Font; set the characters under [theme.glyphs]
scrollbar     = true
column_colors = true        # sizes: grey under 1K, plain under 1M, yellow under 1G, bold yellow above; times: green within a day, plain within a month, grey older (italic past a year); plain in mono / NO_COLOR
mode_line     = "segmented" # or "plain"
popup         = true        # completion candidates in a floating box
```

Everything still goes through the theme, so `NO_COLOR` and the `mono` theme keep working. The powerline-style mode-line separators are only used when icons are on and the theme has two distinct background colors.

### Themes

Built-in: `default`, `high-contrast`, `mono`, `nord`, `gruvbox`, `solarized-dark`, `dracula`. Run `M-x cycle-theme` to step through them, or `M-x load-theme` to pick one by name (the prompt starts on the active theme). Both last for the session; put `name = "..."` under `[theme]` to make it permanent. Hex colors are exact on truecolor terminals (`COLORTERM=truecolor`) and fall back to the nearest ANSI color elsewhere. `NO_COLOR` selects `mono`.

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

The roles are `normal`, `directory`, `symlink`, `executable`, `special`, `marked`, `flagged`, `error`, `changed`, `header`, `dim`, `warning`, `conflict`, `success`, `matched`, `status`, `point`, `prompt`, and `inactive` (mode lines of unselected windows).

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

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
