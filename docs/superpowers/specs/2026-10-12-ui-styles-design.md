# UI styles: rich / clean / minimal (dired-p2d)

## Goal

Make dired look much better using only ratatui, with the look chosen in
config rather than imposed. Three presets ship (`rich`, `clean`, `minimal`);
each visual feature can also be switched individually. Behavior, key
bindings, and the `dired-app` / `dired-tui` split do not change: this is
`dired-tui` rendering plus config schema.

tui-realm was evaluated and rejected (it owns the event loop and duplicates
`dired-app::App`; it contributes no styling).

## Non-goals

- No new commands besides `cycle-style`; no change to key handling,
  `dired-app`, or the event loop in `dired-tui/src/lib.rs`.
- No image/sixel previews, no git column, no new dependencies.
- No change to filename sanitizing (`render::sanitize`), virtualized row
  formatting, or narrow-terminal column collapsing; styles build on them.

## Config

```toml
[display]
style = "clean"            # rich | clean | minimal   (default: clean)

[display.ui]               # every key optional; overrides the preset
borders       = true       # rounded bordered, titled windows
icons         = false      # file-type icons (Nerd Font glyphs)
scrollbar     = true
column_colors = true       # size by magnitude, mtime by age
mode_line     = "segmented"  # "segmented" | "plain"
popup         = true       # completion candidates as a floating box
```

Preset defaults:

| feature        | rich      | clean     | minimal |
|----------------|-----------|-----------|---------|
| borders        | true      | false     | false   |
| icons          | true      | false     | false   |
| scrollbar      | true      | true      | true    |
| column_colors  | true      | true      | true    |
| mode_line      | segmented | segmented | plain   |
| popup          | true      | true      | true    |

`clean` additionally draws a `─` rule under the directory header and adds a
one-column left gutter and a two-column gap between metadata columns
(today: one); `rich` draws the header as the window border title.
These two are fixed properties of the preset, not separate options.

Icons are off in `clean` and `minimal` so no special font is required.
Icon characters are configurable under the existing glyph mechanism:
`[theme.glyphs]` gains `icon_dir`, `icon_file`, `icon_symlink`,
`icon_exec`, and `icon_special` (Nerd Font defaults; ASCII-safe single-cell
alternatives documented in the example config).

Validation (nonfatal, to *Diagnostics*, same as other sections): unknown
`style` or `mode_line` value -> error, falls back to the default; unknown
key under `[display.ui]` -> error (`deny_unknown_fields`).

## Design

### Config crate (`dired-config`)

- `DisplayConfig` gains `style: String` and `ui: UiOverrides`
  (`Option<bool>` / `Option<String>` fields, all default `None`).
- New pure resolver `UiOptions::resolve(&DisplayConfig) -> UiOptions`
  (concrete booleans / enums), implementing the preset table plus
  overrides. All preset logic lives here so it is unit-testable without a
  terminal.
- `Glyphs` gains the `icon_*` fields.
- `example_config_is_clean` keeps passing: `examples/config.toml` documents
  the new keys.

### Runtime switching

`cycle-style` (registered in `registry.rs`, no default key; bind via
`[[keymap]]`) mirrors `cycle-theme`: it rewrites `config.display.style`,
clears nothing else, bumps `config_gen`, and echoes `Style: <name>`.
Overrides in `[display.ui]` still apply on top, so the echo notes
"(with overrides)" when any are set.

### Rendering (`dired-tui`)

`render::draw` resolves `UiOptions` once per frame (cheap; or when
`config_generation` changes, alongside the existing theme refresh) and
passes it down.

1. **Windows / borders.** With `borders`, each window's rect is wrapped in a
   `Block` with `BorderType::Rounded` and a title (directory path; buffer
   name for non-dir buffers; scan state appended). The inner rect is what
   `app.set_window_area` and `window_rects` receive, so per-window
   viewport math stays correct. The selected window's border uses the
   `point`/`header` accent; unselected use `inactive`. Without borders the
   header line is kept (`minimal`) or becomes header + rule (`clean`).
2. **Scrollbar.** A one-column `Scrollbar` on the right edge of each window
   when content exceeds the viewport; thumb position from `top` and total
   row count. With borders it sits on the border.
3. **Column colors.** Size color steps by magnitude (B/K/M/G/T) and mtime
   by age (today, week, month, older), using new `Theme` roles
   (`size_small..size_huge`, `age_new..age_old`) derived from existing
   palette colors so all built-in themes work without new palette
   entries; `mono`/`NO_COLOR` degrade to the current `dim`.
4. **Icons.** A glyph column before the name chosen from entry kind
   (dir, symlink, executable, special, file); extension-specific icons are
   out of scope. Width accounted for in `name_w` so truncation stays
   correct (icon cell + space).
5. **Mode line.** `segmented`: separately styled spans (state, buffer
   name, position `n/total`, sort key, prefix-key hint) divided by Nerd Font
   powerline separators (U+E0B0 / U+E0B2) when `icons` is on, else by a plain
   `│`. `plain`: today's text.
6. **Completion popup.** With `popup`, candidates render as a bordered
   `Block` anchored above the minibuffer (width = longest candidate,
   capped to the screen; `Clear` underneath), selected row highlighted;
   without, today's plain lines. `MAX_CANDIDATES` unchanged.
7. **Cursor.** `draw_dir` already returns the cursor position for Wdired;
   border/icon offsets are added there. Existing tests guard it.

### Files touched

- `crates/dired-config/src/lib.rs`: `style`, `UiOverrides`, `UiOptions`,
  glyph fields, validation.
- `crates/dired-app/src/{registry,commands}.rs`: `cycle-style`.
- `crates/dired-tui/src/{render,theme,lib}.rs`: rendering, roles, resolve.
- `examples/config.toml`, `README.md`, `man/dired.1`.
- Tests: `dired-config` unit tests (resolver table, overrides, bad
  values); `dired-tui/tests/` `TestBackend` tests per style and per
  override (border corners present/absent, scrollbar column, icon cell,
  popup box, mode line text, cursor column in Wdired with borders+icons).

## Delivery stages (each ends green: `cargo test`, clippy `-D warnings`)

1. Config: `style`, overrides, resolver, validation, `cycle-style`,
   plumbing into the frame (no visual change yet beyond `minimal`).
2. `minimal` + shared pieces: scrollbar, column colors, popup.
3. `clean`: header rule, spacing, segmented mode line.
4. `rich`: borders/titles, icons, icon glyph config.
5. Docs: example config, README section, man page.

## Risks

- Borders shrink the usable area: viewport math must use the inner rect;
  covered by the per-style render tests and the existing window tests.
- Nerd Font glyphs render as tofu without the font: hence icons off by
  default, with documented ASCII alternatives.
- 24-bit color on limited terminals: unchanged, handled by the existing
  `nearest_ansi` mapping in `theme.rs`.
