# `dired` — Terminal Directory Editor

## Context

Build a standalone terminal user interface named **`dired`**: a native Rust directory editor that adopts Emacs Dired’s interaction model without embedding Emacs.

This is explicitly **not** a Vim file manager, a pane-oriented commander clone, nor a graphical file browser rendered in a terminal. A directory is presented as a mutable, line-oriented buffer. The core concepts are point, marks, operation flags, minibuffer prompts, buffers, and command dispatch.

The binary name is `dired`.

Primary target: Linux and macOS terminals. Windows support is desirable once the core filesystem and trash abstractions are stable.

---

## Goals

### G1 — Dired-native directory buffer

Opening a path renders one directory buffer whose rows are stable, keyboard-addressable entries. The buffer supports Emacs-like point movement, refresh, parent traversal, visiting files, and entering directories.

### G2 — Mark-based batch operations

Users can mark any set of entries and execute copy, move/rename, shell-command, permission, archive, and other batch operations against that set. When no entries are marked, commands target the entry at point.

### G3 — Deferred destructive operations

Deletion is a two-phase operation: `d` records a deletion flag; `x` reviews and executes all queued deletion flags. The default deletion backend sends eligible entries to the platform trash rather than permanently unlinking them.

### G4 — Editable filenames

A Wdired-compatible edit mode lets users change multiple displayed filenames as text, validates the entire rename plan, previews it, and commits it atomically where the platform/filesystem allows.

### G5 — Minibuffer-driven command UX

Path entry, command arguments, destination prompts, search, filtering, confirmation, and command execution use a persistent minibuffer with history, completion, cancellation, and clear error reporting.

### G6 — Buffer-oriented application model

Every directory, search result, process output, help page, confirmation preview, and error report is a buffer. Users can switch buffers, close buffers, refresh buffers, and preserve independent cursor/filter/sort/mark state per directory buffer.

### G7 — Modern terminal quality

Rendering must be responsive on directories with at least 100,000 entries, never block input during directory scanning or previews, work on standard ANSI terminals, degrade gracefully without image support, and remain accessible without a mouse.

### G8 — Correct, inspectable filesystem changes

All mutating operations are planned before execution, expose conflicts, produce structured results, and are recorded in an operation journal suitable for best-effort undo where the underlying filesystem semantics permit it.

---

## Gates

### Gate 1 — Directory buffer lifecycle (G1, G6)

- `dired [PATH]` opens `PATH`, defaulting to the current working directory.
- `C-x d` opens a minibuffer prompt and creates or focuses a directory buffer for the supplied path.
- `RET` / `f` visits the entry at point: directories open in a directory buffer; regular files invoke the configured opener.
- `^` opens or focuses the parent directory.
- `g` asynchronously reloads the listing while preserving point by stable entry identity if the entry still exists.
- `q` closes the active buffer without terminating the app unless it is the last user buffer.
- Each directory buffer independently retains point, sort, filter, hidden-file state, marks, deletion flags, and expanded subdirectory state.

### Gate 2 — Display and navigation (G1, G7)

- Each visible entry line includes a state prefix, filename, file kind indicator, size, modification timestamp, permissions, and symlink target when applicable.
- Cursor navigation supports `C-n`, `C-p`, arrow keys, `M-<`, `M->`, `C-v`, `M-v`, `C-s`, and `C-r`.
- `j` prompts for a filename prefix/substring and moves point to the next matching entry.
- Directories sort before non-directories by default; user-selectable sorts include name, modification time, size, extension, and natural-name ordering.
- Hidden files can be toggled without losing marks or flags on currently hidden entries.
- A directory scan for 100,000 entries starts producing renderable rows before the scan completes and does not stall event processing for more than 16 ms per frame.

### Gate 3 — Mark semantics (G2)

- `m` marks the entry at point and advances point by one visible entry.
- `u` removes the ordinary mark at point; it also removes a deletion flag when no ordinary mark exists.
- `U` clears ordinary marks in the active directory buffer without clearing deletion flags.
- `t` toggles ordinary marks for all visible, actionable entries.
- `% m` accepts a Rust-compatible regex and marks all visible entries whose display name matches it.
- Every batch command resolves operands as: explicitly marked actionable entries if one or more exist, otherwise the current actionable entry.
- The status line always shows marked count, flagged-for-deletion count, scan state, active filter, and selected sort.

### Gate 4 — Deferred deletion and trash (G3, G8)

- `d` changes the entry state at point to `DeleteFlagged` and advances point.
- `u` clears a deletion flag at point.
- `x` opens an operation-preview buffer containing every flagged source, resolved deletion backend, warnings, and count summary.
- `x` cannot mutate the filesystem until the user explicitly confirms in the preview buffer.
- Default deletion uses the OS trash/recycle-bin abstraction where supported; permanent deletion is a distinct command and must require an explicit irreversible confirmation.
- Individual failures do not abort unrelated deletions; the result buffer records successes, failures, causes, and paths.
- Entries successfully deleted are removed from the buffer on refresh; failures retain their deletion flags and an error annotation.

### Gate 5 — Copy, move, and rename (G2, G8)

- `C` prompts for a destination and builds a copy plan for batch operands.
- `R` prompts for a destination and builds a move/rename plan for batch operands.
- Plans detect duplicate destinations, existing destinations, self-recursive copies, cross-device moves, inaccessible paths, and case-only rename hazards.
- A preview buffer presents source → destination mappings and per-item conflict state before execution.
- The conflict resolver supports ask, overwrite, skip, rename target, overwrite all, and skip all where valid.
- Cross-device moves transparently use copy-then-delete, report non-atomic semantics, and do not delete a source until its destination copy succeeds.
- Completed operations create a journal entry containing sufficient pre-operation metadata for best-effort undo.

### Gate 6 — Wdired edit mode (G4, G8)

- `C-x C-q` enters editable-name mode for the active directory buffer.
- Only filename spans are editable; metadata and state-prefix spans remain protected.
- `C-c C-c` extracts edits, constructs a rename plan, validates collisions/cycles/invalid names, and opens the standard operation-preview buffer.
- Rename cycles (for example `a → b`, `b → a`) are executed using generated safe temporary names in the same directory where possible.
- `C-c C-k` discards uncommitted name edits and restores the last filesystem-backed buffer state.
- Filename edits preserve ordinary marks and deletion flags by stable entry identity whenever possible.

### Gate 7 — Minibuffer, commands, and completion (G5)

- `C-g` cancels the current prompt, operation preview, transient command state, or long-running cancellable task in that priority order.
- Every prompt keeps per-command history in memory and optionally persists it in application state.
- File prompts provide asynchronous path completion; command prompts provide fuzzy command completion.
- `M-x` opens a command prompt that exposes every interactive command by canonical name and aliases.
- `?` opens a help buffer generated from the active keymap and command registry.
- Prefix keys such as `C-x` and `%` show an unobtrusive transient key hint in the status area.

### Gate 8 — Inline directories and buffer operations (G1, G6)

- `i` scans and inserts the directory at point directly below its row as an indented subtree.
- `$` collapses the inserted subtree containing point, or collapses the subtree at point if it is an expanded directory header.
- Nested inserted directories preserve tree depth and cannot create cyclic expansions through symlink traversal unless explicitly enabled.
- `b` opens a buffer-switcher prompt with fuzzy matching; it includes directory, search, process, help, and operation buffers.
- `C-x k` closes a selected buffer after confirmation only when the buffer has an unfinished operation or uncommitted Wdired edits.

### Gate 9 — Shell, process, and search buffers (G2, G6, G7)

- `!` prompts for a shell command and runs it once with the batch operand set substituted safely as argv values.
- `&` runs the same operation asynchronously and streams stdout/stderr into a process buffer.
- Shell command execution never constructs an unescaped shell string from file paths; argv execution is the default. Explicit shell mode is opt-in and visibly labeled.
- `%` opens a transient regexp command family for mark, unmark, rename, and delete-flag operations.
- `A` invokes configurable recursive content search, initially using `rg --json`, and renders matches in a search-results buffer that can visit source files at the matched location.

### Gate 10 — Extensibility and configuration (G5, G7)

- A versioned TOML configuration format controls theme, keymaps, opener rules, shell/search backends, display columns, and safety policy.
- User keymaps can bind a key sequence to a built-in command, external command template, or a future plugin command without changing core code.
- Configuration reload errors are nonfatal and appear in a dedicated diagnostics buffer.
- Default bindings are Dired-compatible wherever a meaningful standalone mapping exists; no Vim normal mode is implemented by default.

---

## Test plan

| Test | Coverage | Procedure / assertion |
|---|---|---|
| T1 — Open and visit | Gate 1, Gate 2 | Start in a fixture directory; assert rendered rows, `RET` visits a file through the mock opener, and a directory produces/focuses a directory buffer. |
| T2 — Refresh preserves point | Gate 1 | Point at `b.txt`, add a preceding fixture file, invoke `g`, and assert point remains on `b.txt`. |
| T3 — Independent buffers | Gate 1, Gate 8 | Open two fixture directories, assign different sort/filter/marks, switch with `b`, and assert all state remains isolated. |
| T4 — Mark fallback | Gate 3 | With no marks, invoke copy and assert only point is the operand; with two marks, assert exactly those two are operands. |
| T5 — Regex marking | Gate 3 | Run `% m` with a fixture regex and assert matching visible filenames gain ordinary marks and nonmatching files do not. |
| T6 — Delete is deferred | Gate 4 | Press `d`; assert the fixture file still exists and the row has `DeleteFlagged`; invoke `x`, reject preview, and assert unchanged filesystem. |
| T7 — Trash execution | Gate 4 | Confirm `x` against a mock trash backend; assert backend receives all flagged paths, successful flags clear after refresh, and failures remain annotated. |
| T8 — Copy conflict preview | Gate 5 | Copy two entries to a target containing one same-name entry; assert preview exposes conflict and execution cannot begin until a policy is selected. |
| T9 — Cross-device move | Gate 5 | Use mock filesystems with distinct device IDs; assert destination copy completes before source removal and a non-atomic warning exists. |
| T10 — Wdired validation | Gate 6 | Edit two names into a collision, commit, and assert no filesystem mutation occurs and preview/diagnostics identify the collision. |
| T11 — Wdired rename cycle | Gate 6 | Rename `a → b` and `b → a`; confirm and assert contents are exchanged correctly without data loss. |
| T12 — Prompt cancellation | Gate 7 | Enter every prompt type and send `C-g`; assert original buffer state and no operation execution. |
| T13 — Inline expansion | Gate 8 | Expand nested fixture directories using `i`, assert correctly indented flattened rows, then `$` and assert all descendant rows disappear. |
| T14 — Argument-safe shell execution | Gate 9 | Use paths containing spaces, quotes, newline-like characters, and shell metacharacters; assert mocked process argv receives exact values without interpolation. |
| T15 — Large-directory responsiveness | Gate 2, Gate 7 | Feed 100,000 synthetic entries; assert first viewport becomes available under a configured target, input events are processed during scan, and memory remains within budget. |
| T16 — Keymap diagnostics | Gate 10 | Load a TOML keymap with an invalid command and duplicate chord; assert app stays usable and diagnostics enumerate both errors. |

---

## Non-functional constraints and guardrails

### Performance

- Target first usable viewport in under 250 ms for a local directory containing 10,000 entries on a modern laptop, excluding pathological network filesystems.
- Target input-to-render latency below 16 ms for ordinary navigation after the first viewport is available.
- Never synchronously stat every entry before presenting the first viewport. Enumerate names first; enrich metadata in bounded asynchronous batches.
- Virtualize rendering: retain structured entries, but render only the viewport plus an overscan margin.
- Bound preview work and cancel stale preview/search/stat tasks when point changes or buffers close.

### Safety

- Default deletion is trash, not permanent unlink.
- Permanent deletion, recursive permanent deletion, overwrite, and external shell mode require explicit confirmation that clearly states scope and affected count.
- Use `std::process::Command` argv APIs by default. Do not invoke a shell unless the user requests shell mode.
- Canonicalize only when it does not compromise useful symlink semantics; display and operate on user-visible paths while explicitly tracking symlink traversal policy.
- Do not follow directory symlinks during recursive operations by default.
- All filesystem errors must include the relevant path, operation, OS error text/code where available, and recovery guidance when possible.

### Portability

- Core support: Linux and macOS.
- Terminal backend must support common terminals through ANSI/VT sequences and gracefully handle absent truecolor, OSC 8 links, image protocol, or clipboard integration.
- File metadata columns adapt to platform capabilities rather than fabricating Unix permission fields on unsupported filesystems.
- Trash behavior is behind a platform abstraction with a configurable fallback policy.

### Configuration and data

- Store configuration under the XDG configuration directory on Unix; use platform-standard locations elsewhere.
- Store state, history, journals, and diagnostics under the platform application-data/state directory.
- Never transmit filenames, directory metadata, search contents, telemetry, or configuration externally.
- No network access is required by the core application.

### Dependency and security policy

- Pin exact dependency versions in `Cargo.lock`; audit with `cargo deny` and `cargo audit` in CI.
- Prefer maintained crates with clear licenses and active releases.
- Feature-gate optional subsystems such as image preview, git integration, archives, remote filesystems, and scripting.
- Fuzz filename parsing, rendering-width handling, Wdired edit extraction, rename-plan construction, and config parsing.

### Accessibility and ergonomics

- All features are keyboard-operable.
- Do not encode state exclusively in color; prefixes and glyphs must express marked, flagged, error, focused, and changed states.
- Provide high-contrast themes and configurable colors/glyphs.
- Respect terminal width; collapse low-priority metadata before truncating filenames.

---

## Implementation notes

### Proposed workspace

```text
dired/
├── Cargo.toml
├── crates/
│   ├── dired-app/          # application state, commands, orchestration
│   ├── dired-domain/       # buffers, entries, selection, operations, plans
│   ├── dired-fs/           # local filesystem and future VFS traits
│   ├── dired-tui/          # Ratatui rendering, events, input editor
│   ├── dired-config/       # TOML config, themes, keymaps, persistence
│   ├── dired-process/      # argv process execution and output streaming
│   └── dired-testkit/      # deterministic mock FS, clock, process runner
└── src/main.rs             # thin binary bootstrap
```

### Modern Rust stack

| Concern | Recommended crate / approach |
|---|---|
| Async runtime | `tokio` |
| TUI rendering | `ratatui` |
| Terminal events/backend | `crossterm` initially; isolate behind adapter trait |
| Event streams | `tokio-util`, `futures-util`, bounded channels |
| Directory enumeration | `ignore` for scalable walking/filter rules; `std::fs`/`tokio::fs` for direct directory work |
| File metadata and platform work | `rustix` / `cap-std` where capability-oriented APIs are useful |
| Regex | `regex` |
| Fuzzy matching | `nucleo-matcher` |
| Config | `serde`, `toml`, `schemars` as needed |
| Errors | `miette` for rich diagnostics; `thiserror` for domain errors |
| Logging/tracing | `tracing`, `tracing-subscriber` |
| Unicode display width | `unicode-width`, `unicode-segmentation` |
| Time | `time` |
| File watching | `notify` behind a debounced reconciliation layer |
| Trash | a maintained platform-trash crate behind a `TrashBackend` trait, or native adapters if reliability requires it |
| Testing | `rstest`, `proptest`, `insta`, `tempfile` |

Do not let UI code call filesystem APIs directly. The UI dispatches typed commands to the application layer; the application layer creates domain operation plans; adapters execute plans and return structured events/results.

### Essential traits

```rust
#[async_trait::async_trait]
pub trait FileSystem: Send + Sync {
    async fn read_dir(&self, path: &Path) -> Result<DirectoryStream, FsError>;
    async fn metadata(&self, path: &Path, follow_symlinks: bool) -> Result<EntryMetadata, FsError>;
    async fn copy(&self, from: &Path, to: &Path, options: CopyOptions) -> Result<(), FsError>;
    async fn rename(&self, from: &Path, to: &Path, options: RenameOptions) -> Result<(), FsError>;
    async fn remove(&self, path: &Path, options: RemoveOptions) -> Result<(), FsError>;
}

pub trait TrashBackend: Send + Sync {
    fn supported(&self) -> bool;
    fn trash(&self, paths: &[PathBuf]) -> Result<TrashReport, TrashError>;
}

pub trait ProcessRunner: Send + Sync {
    fn spawn(&self, spec: ProcessSpec) -> Result<ProcessHandle, ProcessError>;
}
```

### State and event model

- Use a single application reducer/state machine for deterministic command handling.
- Emit typed events for directory rows, metadata updates, watch events, process output, operation progress, and task completion.
- Background tasks may not mutate UI state directly; they send events tagged with buffer generation/version so stale results are discarded.
- Keep operation planning pure where possible. Plans should be serializable for preview, journaling, testing, and future scripting.

### Initial command registry

```text
C-x d      dired-open-directory
C-x C-f    dired-find-file
C-x C-q    wdired-toggle-edit
C-x k      buffer-kill
C-n / ↓    next-line
C-p / ↑    previous-line
RET / f    dired-visit
^          dired-up-directory
g          revert-buffer
m          dired-mark
u          dired-unmark
t          dired-toggle-marks
d          dired-flag-file-deletion
x          dired-do-flagged-delete
C          dired-do-copy
R          dired-do-rename
+          dired-create-directory
!          dired-do-shell-command
&          dired-do-async-shell-command
i          dired-maybe-insert-subdir
$          dired-hide-subdir
% m        dired-mark-files-regexp
% d        dired-flag-files-regexp
% r        dired-do-rename-regexp
A          dired-search-files-content
b          switch-to-buffer
?          help-for-help
M-x        execute-extended-command
C-g        keyboard-quit
q          quit-window
```

---

## Out-of-scope

The first production release does not include:

- Vim modal editing or Vim-compatible defaults.
- A graphical GUI, mouse-first UX, or desktop file-manager integration beyond configurable openers.
- Full Emacs Lisp compatibility, an Emacs package runtime, or embedding Emacs.
- Remote filesystem support such as SSH/SFTP/TRAMP; define VFS traits so it can be added later.
- Git status, diff staging, or version-control porcelain UI.
- Archive browsing/editing.
- Image rendering or rich-media preview protocols.
- A general plugin runtime or embedded scripting language.
- Cloud sync, accounts, telemetry, analytics, or network services.
- A promise of transactional undo across arbitrary external filesystems; undo is best-effort and operation-specific.

---

## Acceptance slice

The first demonstrable release is complete when a user can run `dired`, navigate a directory with Emacs-like keys, mark files, queue deletion flags, review and trash flagged files, copy/move marked files through a conflict-aware plan, use Wdired-style batch renaming, run an argv-safe command on marks, and switch among directory/process/help buffers without blocking the terminal UI.
