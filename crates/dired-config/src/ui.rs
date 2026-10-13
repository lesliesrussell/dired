// dired-p2d
//! UI style presets and per-feature overrides, resolved to concrete options.

use serde::{Deserialize, Serialize};

use crate::DisplayConfig;

/// Built-in style names, in `cycle-style` order.
pub const STYLES: &[&str] = &["rich", "clean", "minimal"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiStyle {
    Rich,
    Clean,
    Minimal,
}

impl UiStyle {
    pub fn parse(s: &str) -> Option<UiStyle> {
        match s {
            "rich" => Some(UiStyle::Rich),
            "clean" => Some(UiStyle::Clean),
            "minimal" => Some(UiStyle::Minimal),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            UiStyle::Rich => "rich",
            UiStyle::Clean => "clean",
            UiStyle::Minimal => "minimal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeLine {
    Segmented,
    Plain,
}

impl ModeLine {
    pub fn parse(s: &str) -> Option<ModeLine> {
        match s {
            "segmented" => Some(ModeLine::Segmented),
            "plain" => Some(ModeLine::Plain),
            _ => None,
        }
    }
}

/// `[display.ui]`: every key optional; set keys override the preset.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiOverrides {
    pub borders: Option<bool>,
    pub icons: Option<bool>,
    pub scrollbar: Option<bool>,
    pub column_colors: Option<bool>,
    /// `segmented` or `plain`.
    pub mode_line: Option<String>,
    pub popup: Option<bool>,
}

impl UiOverrides {
    pub fn is_empty(&self) -> bool {
        *self == UiOverrides::default()
    }
}

/// Concrete UI options after applying the preset and overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiOptions {
    pub style: UiStyle,
    pub borders: bool,
    pub icons: bool,
    pub scrollbar: bool,
    pub column_colors: bool,
    pub mode_line: ModeLine,
    pub popup: bool,
    /// `clean`: a `─` rule under the directory header.
    pub header_rule: bool,
    /// `clean`: left gutter and wider gaps between columns.
    pub roomy: bool,
}

impl UiOptions {
    pub fn resolve(d: &DisplayConfig) -> UiOptions {
        let style = UiStyle::parse(&d.style).unwrap_or(UiStyle::Clean);
        let rich = style == UiStyle::Rich;
        let o = &d.ui;
        let default_mode = if style == UiStyle::Minimal { ModeLine::Plain } else { ModeLine::Segmented };
        UiOptions {
            style,
            borders: o.borders.unwrap_or(rich),
            icons: o.icons.unwrap_or(rich),
            scrollbar: o.scrollbar.unwrap_or(true),
            column_colors: o.column_colors.unwrap_or(true),
            mode_line: o.mode_line.as_deref().and_then(ModeLine::parse).unwrap_or(default_mode),
            popup: o.popup.unwrap_or(true),
            header_rule: style == UiStyle::Clean,
            roomy: style == UiStyle::Clean,
        }
    }

    /// Rows a directory buffer's header takes (path or column labels, plus the rule).
    pub fn dir_header_rows(&self) -> u16 {
        if self.header_rule {
            2
        } else {
            1
        }
    }

    /// Rows each window's frame takes beyond its mode line.
    pub fn border_rows(&self) -> u16 {
        if self.borders {
            2
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(style: &str, ui: UiOverrides) -> DisplayConfig {
        DisplayConfig { style: style.into(), ui, ..DisplayConfig::default() }
    }

    #[test]
    fn presets_match_the_table() {
        let r = UiOptions::resolve(&display("rich", UiOverrides::default()));
        assert!(r.borders && r.icons && r.scrollbar && r.column_colors && r.popup);
        assert_eq!(r.mode_line, ModeLine::Segmented);
        assert!(!r.header_rule && !r.roomy);
        assert_eq!((r.dir_header_rows(), r.border_rows()), (1, 2));

        let c = UiOptions::resolve(&display("clean", UiOverrides::default()));
        assert!(!c.borders && !c.icons && c.scrollbar && c.column_colors && c.popup);
        assert_eq!(c.mode_line, ModeLine::Segmented);
        assert!(c.header_rule && c.roomy);
        assert_eq!((c.dir_header_rows(), c.border_rows()), (2, 0));

        let m = UiOptions::resolve(&display("minimal", UiOverrides::default()));
        assert!(!m.borders && !m.icons && m.scrollbar && m.column_colors && m.popup);
        assert_eq!(m.mode_line, ModeLine::Plain);
        assert!(!m.header_rule && !m.roomy);
    }

    #[test]
    fn overrides_win_over_the_preset() {
        let ui = UiOverrides {
            borders: Some(true),
            icons: Some(true),
            scrollbar: Some(false),
            column_colors: Some(false),
            popup: Some(false),
            mode_line: Some("plain".into()),
        };
        let o = UiOptions::resolve(&display("clean", ui));
        assert!(o.borders && o.icons && !o.scrollbar && !o.column_colors && !o.popup);
        assert_eq!(o.mode_line, ModeLine::Plain);
        assert!(o.header_rule, "preset-fixed properties stay with the preset");
    }

    #[test]
    fn unknown_style_resolves_to_clean() {
        assert_eq!(UiOptions::resolve(&display("bogus", UiOverrides::default())).style, UiStyle::Clean);
    }

    #[test]
    fn style_names_round_trip() {
        for n in STYLES {
            assert_eq!(UiStyle::parse(n).unwrap().name(), *n);
        }
        assert!(UiStyle::parse("nope").is_none());
    }
}
