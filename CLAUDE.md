# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:6cd5cc61 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->


## Build & Test

```bash
just check                      # what CI runs: tests + clippy -D warnings + man page lint
cargo test --workspace --locked # ~270 tests, about 12 s (one timing test lives in tests/gates.rs)
cargo clippy --workspace --all-targets --locked -- -D warnings
just run --check-config         # release build; --check-config validates the config and exits
just install                   # user-local binary, man page, and examples (no sudo)
```

- CI builds on Ubuntu and macOS and runs `cargo deny` and a RustSec audit. **Windows is not supported and not planned** (a decision, not an oversight): Unix-only APIs are fine.
- `Cargo.lock` is committed and CI uses `--locked`; after changing dependencies run a plain `cargo build` once and commit the lock file.
- The man page is mdoc; lint it with `just man-lint` (mandoc or groff).

## Architecture Overview

A Cargo workspace of small internal crates plus the `dired` binary (`src/main.rs`: wiring and the CLI).

| Crate | Role |
|---|---|
| `dired-domain` | Pure model: entries and metadata, sorting, operation plans (copy/move/delete/rename with conflicts), argv building, the undo journal, the injectable `Clock`, and the mark `Predicate` (`size>10M age>30d ...`) |
| `dired-config` | Config loading (never fatal: problems become `Diagnostic`s), Emacs-style key notation, and `UiOptions::resolve` (style presets plus overrides) |
| `dired-fs` | The async `FileSystem` trait, `LocalFs`, plan execution, trash backends |
| `dired-process` | Argv-only process runner with streamed output (no shell involved) |
| `dired-app` | The whole UI-independent state machine, `App`: buffers (directory, text, operation preview), a window layout tree, the minibuffer with completion and pickers, keymaps, the command registry (`registry.rs`), command handlers (`commands.rs`), background `tasks`, injected `services` (filesystem, trash, runner, opener, clock, git), plus `jumps`, `fileview`, `git` |
| `dired-tui` | The terminal driver (crossterm input thread + a tokio `select!` loop), ratatui rendering (`render.rs`, `widgets.rs`, `theme.rs`), and the clipboard |
| `dired-testkit` | `MockFs`, `MockClock`, `MockRunner`, `MockTrash` |

**Data flow.** Keys go to `App::handle_key`, then the keymap, then `run_command(name)`. Slow work (listing, metadata, git, file views, operations) runs in tokio tasks that send `AppEvent`s tagged with a buffer id and a generation number, so stale results are dropped. `render::draw(frame, &mut App, &Theme)` only reads `App`. Things that need the real terminal come back as `Effect`s (`Foreground`, `Clipboard`) for the driver to perform. Because the renderer decides how many rows the frame takes (borders, header rows), it tells the app via `App::set_chrome` before `set_window_area`; scroll math depends on that.

**State on disk** (`$DIRED_STATE_DIR` or the platform state dir): `history.json`, `journal.jsonl`, `jumps.json`. Config: `$DIRED_CONFIG` or `~/.config/dired/config.toml`.

## Conventions & Patterns

- **Test first.** App behavior tests are in `crates/dired-app/tests/` (`common::harness`, `press("C-x C-f")`, `settle().await`); rendering tests are in `crates/dired-tui/tests/` (the `Ui` helper over ratatui's `TestBackend`). Anything slow or external goes through an injected service (`Clock`, `GitProbe`, `FileSystem`, `Opener`) so tests are deterministic. After writing a test for a guard, break the code once on purpose and confirm the test fails.
- **Adding a command:** register it in `registry.rs` (name, aliases, doc), bind a default key in `keymap.rs`, handle it in `run_command`, and for prompts add a `PromptAction` (and a `Completion` kind if it lists candidates; closed-set pickers are listed in `Completion::picks()`). Document it in `README.md`, `man/dired.1` and `examples/config.toml` (a test keeps the example config free of diagnostics).
- **Never put file names, paths or process output on screen without `render::truncate`**, which escapes control characters. Never build a shell command line from strings; use argv.
- **Config structs** use `#[serde(default, deny_unknown_fields)]`; a bad value is a diagnostic and falls back to the default, never a crash.
- **Keys** use Emacs notation (`C-x C-f`, `M-x`, `<up>`). Terminals cannot tell `C-M-S-v` from `C-M-v`, so pairs of "forward/back" commands need distinct keys.
- **Beads:** every block of code added for a bead carries a `// <bead-id>` comment; one branch per bead, named after it; commits end with the co-author trailer. `CHANGELOG.md` is written from closed beads.
