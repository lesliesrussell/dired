// dired-7qs
//! Versioned TOML configuration. Loading never fails: problems become
//! diagnostics and the affected section falls back to defaults.

pub mod keys;

// dired-p2d
mod ui;
pub use ui::{ModeLine, UiOptions, UiOverrides, UiStyle, STYLES};

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use keys::{format_seq, parse_key, parse_seq, Key, KeyCode};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub source: String,
    pub message: String,
}

impl Diagnostic {
    pub fn error(source: impl Into<String>, message: impl Into<String>) -> Self {
        Diagnostic { severity: Severity::Error, source: source.into(), message: message.into() }
    }
    pub fn warning(source: impl Into<String>, message: impl Into<String>) -> Self {
        Diagnostic { severity: Severity::Warning, source: source.into(), message: message.into() }
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{sev}: {}: {}", self.source, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    pub show_hidden: bool,
    pub dirs_first: bool,
    pub sort: String,
    /// Metadata columns in display order; dropped right-to-left priority
    /// when the terminal is narrow. Known: git, permissions, size, mtime.
    pub columns: Vec<String>,
    // dired-ta2
    /// Copy/move prompts default to the directory shown in another window.
    pub dwim_target: bool,
    // dired-p2d
    /// One of [`STYLES`].
    pub style: String,
    pub ui: UiOverrides,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        DisplayConfig {
            show_hidden: true,
            dirs_first: true,
            sort: "name".into(),
            columns: vec!["git".into(), "permissions".into(), "size".into(), "mtime".into()], // dired-99k
            dwim_target: true,
            // dired-p2d
            style: "clean".into(),
            ui: UiOverrides::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Glyphs {
    pub marked: String,
    pub flagged: String,
    pub error: String,
    pub changed: String,
    pub focused: String,
    // dired-p2d
    /// File-type icons (Nerd Font by default; set ASCII if your font lacks them).
    pub icon_dir: String,
    pub icon_file: String,
    pub icon_symlink: String,
    pub icon_exec: String,
    pub icon_special: String,
}

impl Default for Glyphs {
    fn default() -> Self {
        Glyphs {
            marked: "*".into(),
            flagged: "D".into(),
            error: "!".into(),
            changed: "~".into(),
            focused: ">".into(),
            // dired-p2d
            icon_dir: "\u{f07b}".into(),
            icon_file: "\u{f016}".into(),
            icon_symlink: "\u{f0c1}".into(),
            icon_exec: "\u{f013}".into(),
            icon_special: "\u{f059}".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// One of [`THEMES`].
    pub name: Option<String>,
    /// Shorthand overrides: role → foreground color name or `#rrggbb`.
    pub colors: std::collections::BTreeMap<String, String>,
    /// Full per-role style overrides, applied after `colors`.
    pub styles: std::collections::BTreeMap<String, StyleSpec>,
    pub glyphs: Glyphs,
}

/// A style override. Unset fields keep the theme's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct StyleSpec {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub reversed: Option<bool>,
    pub dim: Option<bool>,
}

/// Built-in theme names.
pub const THEMES: &[&str] =
    &["default", "high-contrast", "mono", "nord", "gruvbox", "solarized-dark", "dracula"];

/// Styleable roles.
pub const STYLE_ROLES: &[&str] = &[
    "normal", "directory", "symlink", "executable", "special", "marked", "flagged", "error",
    "changed", "header", "dim", "warning", "conflict", "success", "matched", "status", "point",
    "prompt", "inactive",
];

/// Named colors accepted in addition to `#rrggbb`.
pub const COLOR_NAMES: &[&str] = &[
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "gray", "grey", "darkgray",
    "darkgrey", "white", "lightred", "lightgreen", "lightyellow", "lightblue", "lightmagenta",
    "lightcyan", "reset", "default",
];

pub fn is_valid_color(s: &str) -> bool {
    let s = s.trim().to_lowercase();
    match s.strip_prefix('#') {
        Some(hex) => hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()),
        None => COLOR_NAMES.contains(&s.as_str()),
    }
}

fn validate_theme(t: &ThemeConfig, source: &str, diags: &mut Vec<Diagnostic>) {
    if let Some(n) = &t.name {
        if !THEMES.contains(&n.as_str()) {
            diags.push(Diagnostic::error(
                source,
                format!("[theme] unknown theme `{n}` (known: {})", THEMES.join(", ")),
            ));
        }
    }
    let check_role = |role: &str, diags: &mut Vec<Diagnostic>, table: &str| {
        if !STYLE_ROLES.contains(&role) {
            diags.push(Diagnostic::error(source, format!("[theme.{table}] unknown role `{role}`")));
        }
    };
    let check_color = |c: &str, at: String, diags: &mut Vec<Diagnostic>| {
        if !is_valid_color(c) {
            diags.push(Diagnostic::error(source, format!("{at}: invalid color `{c}` (use a name or #rrggbb)")));
        }
    };
    for (role, c) in &t.colors {
        check_role(role, diags, "colors");
        check_color(c, format!("[theme.colors] {role}"), diags);
    }
    for (role, st) in &t.styles {
        check_role(role, diags, "styles");
        for (k, c) in [("fg", &st.fg), ("bg", &st.bg)] {
            if let Some(c) = c {
                check_color(c, format!("[theme.styles.{role}] {k}"), diags);
            }
        }
    }
}

/// A user key binding. Exactly one of `command`, `exec`, `plugin` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyBindingSpec {
    #[serde(default = "default_mode")]
    pub mode: String,
    pub key: String,
    pub command: Option<String>,
    /// External command template; `{file}`/`{files}` expand to operands as argv.
    pub exec: Option<Vec<String>>,
    pub plugin: Option<String>,
}

fn default_mode() -> String {
    "dired".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenerRule {
    /// File extensions (without dot), case-insensitive. Empty matches all.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// argv; `{file}` is the path, `{line}` the line number (search results).
    pub command: Vec<String>,
    /// Runs in the terminal (the UI is suspended) instead of detached.
    #[serde(default)]
    pub terminal: bool,
}

// dired-0cz
/// A candidate for "open with" (`O`): a labelled program for the entry at point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenWithEntry {
    /// Label shown in (and typed into) the picker.
    pub name: String,
    /// File extensions (without dot), case-insensitive. Empty offers it for every entry.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// argv; `{file}` is the path.
    pub command: Vec<String>,
    /// Runs in the terminal (the UI is suspended) instead of detached.
    #[serde(default)]
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShellConfig {
    /// Shell used only for explicit shell mode.
    pub shell: String,
}

impl Default for ShellConfig {
    fn default() -> Self {
        ShellConfig { shell: "/bin/sh".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchConfig {
    /// argv prefix; the pattern and search paths are appended.
    pub command: Vec<String>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig { command: vec!["rg".into(), "--json".into(), "-e".into()] }
    }
}

// dired-99k
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GitConfig {
    /// Show git state (status column, branch in the mode line). Costs one
    /// `git` process per directory refresh.
    pub enabled: bool,
    // dired-6ul
    /// argv of the git program `C-c G` runs in the terminal (the UI is
    /// suspended), for example `["gitui"]`. Empty means `lazygit`.
    pub ui: Vec<String>,
}

impl Default for GitConfig {
    fn default() -> Self {
        GitConfig { enabled: true, ui: Vec::new() }
    }
}

// dired-z5y
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// argv for "open terminal here" (`C-c t`); `{dir}` is the directory. The
    /// program also starts in that directory. Empty uses the platform default.
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SafetyConfig {
    /// When trash is unavailable: `refuse` (default) or `permanent`
    /// (offer permanent deletion, still with explicit confirmation).
    pub trash_fallback: String,
    pub follow_symlinks_on_insert: bool,
    pub persist_history: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        SafetyConfig {
            trash_fallback: "refuse".into(),
            follow_symlinks_on_insert: false,
            persist_history: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub version: u32,
    pub display: DisplayConfig,
    pub theme: ThemeConfig,
    pub keymap: Vec<KeyBindingSpec>,
    pub opener: Vec<OpenerRule>,
    // dired-0cz
    pub open_with: Vec<OpenWithEntry>,
    pub shell: ShellConfig,
    pub search: SearchConfig,
    pub safety: SafetyConfig,
    // dired-99k
    pub git: GitConfig,
    // dired-z5y
    pub terminal: TerminalConfig,
}

fn section<T: DeserializeOwned + Default>(
    table: &toml::Table,
    name: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) -> T {
    match table.get(name) {
        None => T::default(),
        Some(v) => T::deserialize(v.clone()).unwrap_or_else(|e| {
            diags.push(Diagnostic::error(source, format!("[{name}]: {}", e.message())));
            T::default()
        }),
    }
}

fn list<T: DeserializeOwned>(
    table: &toml::Table,
    name: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) -> Vec<T> {
    match table.get(name) {
        None => Vec::new(),
        Some(toml::Value::Array(items)) => items
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                T::deserialize(v.clone())
                    .map_err(|e| {
                        diags.push(Diagnostic::error(source, format!("{name}[{i}]: {}", e.message())))
                    })
                    .ok()
            })
            .collect(),
        Some(_) => {
            diags.push(Diagnostic::error(source, format!("`{name}` must be an array of tables ([[{name}]])")));
            Vec::new()
        }
    }
}

// dired-p2d
fn validate_display(d: &mut DisplayConfig, source: &str, diags: &mut Vec<Diagnostic>) {
    if UiStyle::parse(&d.style).is_none() {
        diags.push(Diagnostic::error(
            source,
            format!("display.style `{}` is not one of {}", d.style, STYLES.join(", ")),
        ));
        d.style = DisplayConfig::default().style;
    }
    if let Some(m) = &d.ui.mode_line {
        if ModeLine::parse(m).is_none() {
            diags.push(Diagnostic::error(
                source,
                format!("display.ui.mode_line `{m}` must be `segmented` or `plain`"),
            ));
            d.ui.mode_line = None;
        }
    }
}

impl Config {
    pub fn defaults() -> Self {
        Config { version: CONFIG_VERSION, ..Default::default() }
    }

    /// Parse config text. Always returns a usable config.
    pub fn parse(text: &str, source: &str) -> (Config, Vec<Diagnostic>) {
        let mut diags = Vec::new();
        let table: toml::Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                diags.push(Diagnostic::error(source, format!("TOML syntax: {}", e.message())));
                return (Config::defaults(), diags);
            }
        };
        let version = match table.get("version") {
            Some(toml::Value::Integer(v)) => *v as u32,
            Some(_) => {
                diags.push(Diagnostic::error(source, "`version` must be an integer"));
                CONFIG_VERSION
            }
            None => {
                diags.push(Diagnostic::warning(source, format!("missing `version`; assuming {CONFIG_VERSION}")));
                CONFIG_VERSION
            }
        };
        if version != CONFIG_VERSION {
            diags.push(Diagnostic::warning(
                source,
                format!("config version {version} is not {CONFIG_VERSION}; reading best-effort"),
            ));
        }
        // dired-0cz
        // dired-z5y
        const KNOWN: [&str; 11] = [
            "version", "display", "theme", "keymap", "opener", "open_with", "shell", "search", "safety", "git",
            "terminal",
        ];
        for k in table.keys() {
            if !KNOWN.contains(&k.as_str()) {
                diags.push(Diagnostic::warning(source, format!("unknown key `{k}`")));
            }
        }
        let keymap: Vec<KeyBindingSpec> = list(&table, "keymap", source, &mut diags);
        let keymap = keymap
            .into_iter()
            .filter(|b| {
                let n = b.command.is_some() as u8 + b.exec.is_some() as u8 + b.plugin.is_some() as u8;
                if n != 1 {
                    diags.push(Diagnostic::error(
                        source,
                        format!("keymap `{}`: set exactly one of command, exec, plugin", b.key),
                    ));
                }
                n == 1
            })
            .collect();
        // dired-0cz
        let open_with: Vec<OpenWithEntry> = list(&table, "open_with", source, &mut diags);
        let open_with = open_with
            .into_iter()
            .filter(|e| {
                let ok = !e.name.trim().is_empty() && !e.command.is_empty();
                if !ok {
                    diags.push(Diagnostic::error(
                        source,
                        format!("open_with `{}`: needs a non-empty name and command", e.name),
                    ));
                }
                ok
            })
            .collect();
        // dired-p2d
        // dired-mv2: `[display.ui]` is read on its own, so a mistake in it cannot
        // reset the rest of `[display]`.
        let mut display_table = table.clone();
        let mut ui_value = None;
        if let Some(toml::Value::Table(d)) = display_table.get_mut("display") {
            ui_value = d.remove("ui");
        }
        let mut display: DisplayConfig = section(&display_table, "display", source, &mut diags);
        if let Some(v) = ui_value {
            display.ui = UiOverrides::deserialize(v).unwrap_or_else(|e| {
                diags.push(Diagnostic::error(source, format!("[display.ui]: {}", e.message())));
                UiOverrides::default()
            });
        }
        validate_display(&mut display, source, &mut diags);
        let theme: ThemeConfig = section(&table, "theme", source, &mut diags);
        validate_theme(&theme, source, &mut diags);
        let config = Config {
            version,
            display,
            theme,
            keymap,
            opener: list(&table, "opener", source, &mut diags),
            open_with,
            shell: section(&table, "shell", source, &mut diags),
            search: section(&table, "search", source, &mut diags),
            safety: section(&table, "safety", source, &mut diags),
            git: section(&table, "git", source, &mut diags), // dired-99k
            terminal: section(&table, "terminal", source, &mut diags), // dired-z5y
        };
        (config, diags)
    }

    pub fn load(path: &Path) -> (Config, Vec<Diagnostic>) {
        match std::fs::read_to_string(path) {
            Ok(t) => Config::parse(&t, &path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::defaults(), Vec::new()),
            Err(e) => (
                Config::defaults(),
                vec![Diagnostic::error(path.display().to_string(), e.to_string())],
            ),
        }
    }
}

/// `$XDG_CONFIG_HOME/dired/config.toml` (or `~/.config/...`) on Unix,
/// the platform config directory elsewhere.
pub fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DIRED_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let base = if cfg!(unix) {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
    } else {
        dirs::config_dir()
    };
    base.map(|b| b.join("dired").join("config.toml"))
}

/// Directory for history, journal, and diagnostics.
pub fn state_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DIRED_STATE_DIR") {
        return Some(PathBuf::from(p));
    }
    dirs::state_dir().or_else(dirs::data_local_dir).map(|d| d.join("dired"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_is_clean() {
        let (c, d) = Config::parse(include_str!("../../../examples/config.toml"), "examples/config.toml");
        assert!(d.is_empty(), "{d:?}");
        assert!(!c.keymap.is_empty() && !c.theme.styles.is_empty());
    }

    // dired-6ul
    #[test]
    fn git_ui_defaults_to_lazygit_and_parses() {
        let (c, d) = Config::parse("version = 1\n", "t");
        assert!(d.is_empty() && c.git.ui.is_empty(), "{d:?}");
        let (c, d) = Config::parse("version = 1\n[git]\nui = [\"gitui\"]\n", "t");
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(c.git.ui, ["gitui"]);
        let (c, d) = Config::parse("version = 1\n[git]\nui = 3\n", "t");
        assert!(c.git.ui.is_empty());
        assert_eq!(d.len(), 1, "{d:?}");
    }

    // dired-z5y
    #[test]
    fn terminal_section_defaults_to_the_platform_launcher() {
        let (c, d) = Config::parse("version = 1\n", "t");
        assert!(d.is_empty(), "{d:?}");
        assert!(c.terminal.command.is_empty());
    }

    // dired-z5y
    #[test]
    fn terminal_command_parses() {
        let (c, d) = Config::parse("version = 1\n[terminal]\ncommand = [\"kitty\", \"--directory\", \"{dir}\"]\n", "t");
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(c.terminal.command, ["kitty", "--directory", "{dir}"]);
        let (c, d) = Config::parse("version = 1\n[terminal]\ncommand = 3\n", "t");
        assert!(c.terminal.command.is_empty());
        assert_eq!(d.len(), 1, "{d:?}");
    }

    // dired-0cz
    #[test]
    fn open_with_entries_parse_and_validate() {
        let (c, d) = Config::parse(
            r#"
version = 1
[[open_with]]
name = "Preview"
command = ["open", "-a", "Preview", "{file}"]
extensions = ["png"]
[[open_with]]
name = "Neovim"
command = ["nvim", "{file}"]
terminal = true
[[open_with]]
name = ""
command = ["x"]
[[open_with]]
name = "Empty"
command = []
"#,
            "t",
        );
        assert_eq!(c.open_with.len(), 2);
        assert_eq!(c.open_with[0].extensions, vec!["png"]);
        assert!(!c.open_with[0].terminal && c.open_with[1].terminal);
        assert_eq!(d.len(), 2, "{d:?}");
    }

    #[test]
    fn theme_validation() {
        let (_, d) = Config::parse(
            "version = 1\n[theme]\nname = \"nope\"\ncolors = { directory = \"#12345g\", bogus = \"red\" }\n[theme.styles.point]\nbg = \"#223344\"\nbold = true\n",
            "t",
        );
        assert_eq!(d.len(), 3, "{d:?}");
    }

    #[test]
    fn errors_are_nonfatal() {
        let (c, d) = Config::parse(
            r#"
version = 1
bogus = 3
[display]
show_hidden = false
[safety]
trash_fallback = 5
[[keymap]]
key = "C-c o"
command = "dired-visit"
[[keymap]]
key = "C-c p"
"#,
            "t",
        );
        assert!(!c.display.show_hidden);
        assert_eq!(c.keymap.len(), 1);
        assert_eq!(c.safety, SafetyConfig::default());
        assert_eq!(d.len(), 3, "{d:?}");
        let (_, d) = Config::parse("version = [", "t");
        assert_eq!(d.len(), 1);
    }

    // dired-p2d
    #[test]
    fn display_style_defaults_and_validation() {
        let (c, d) = Config::parse("version = 1\n", "t");
        assert_eq!(c.display.style, "clean");
        assert!(d.is_empty(), "{d:?}");
        assert!(c.display.ui.is_empty());

        let (c, d) = Config::parse(
            "version = 1\n[display]\nstyle = \"glam\"\n[display.ui]\nmode_line = \"fancy\"\nborders = true\n",
            "t",
        );
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(c.display.style, "clean");
        assert_eq!(c.display.ui.mode_line, None);
        assert_eq!(c.display.ui.borders, Some(true));

        let (_, d) = Config::parse("version = 1\n[display.ui]\nbogus = 1\n", "t");
        assert_eq!(d.len(), 1, "{d:?}");

        let (c, d) = Config::parse("version = 1\n[theme.glyphs]\nicon_dir = \"D\"\n", "t");
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(c.theme.glyphs.icon_dir, "D");
        assert_eq!(c.theme.glyphs.icon_file, Glyphs::default().icon_file);
    }

    // dired-p2d
    #[test]
    fn example_config_documents_styles() {
        let text = include_str!("../../../examples/config.toml");
        for needle in ["style", "[display.ui]", "cycle-style", "icon_dir"] {
            assert!(text.contains(needle), "examples/config.toml should mention {needle}");
        }
        let (c, d) = Config::parse(text, "examples/config.toml");
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(c.display.style, "clean");
    }

    // dired-mv2
    #[test]
    fn bad_display_ui_does_not_reset_the_rest_of_display() {
        for bad in ["bordrs = true", "borders = \"yes\""] {
            let text = format!("version = 1\n[display]\nshow_hidden = false\nsort = \"size\"\nstyle = \"rich\"\n[display.ui]\n{bad}\n");
            let (c, d) = Config::parse(&text, "t");
            assert_eq!(d.len(), 1, "{bad}: {d:?}");
            assert!(!c.display.show_hidden, "{bad}");
            assert_eq!(c.display.sort, "size", "{bad}");
            assert_eq!(c.display.style, "rich", "{bad}");
            assert!(c.display.ui.is_empty(), "{bad}");
        }
    }

    // dired-99k
    #[test]
    fn git_section_and_default_columns() {
        let (c, d) = Config::parse("version = 1\n", "t");
        assert!(d.is_empty(), "{d:?}");
        assert!(c.git.enabled);
        assert_eq!(c.display.columns, ["git", "permissions", "size", "mtime"]);
        let (c, d) = Config::parse("version = 1\n[git]\nenabled = false\n", "t");
        assert!(d.is_empty() && !c.git.enabled, "{d:?}");
        let (c, d) = Config::parse("version = 1\n[git]\nenabeld = false\n", "t");
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(c.git.enabled, "a typo falls back to the default");
    }
}
