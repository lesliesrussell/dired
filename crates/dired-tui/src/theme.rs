// dired-7qs
// dired-7yu
//! Themes. State is never conveyed by color alone: glyphs carry it too.

use dired_config::{Glyphs, StyleSpec, ThemeConfig};
use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub normal: Style,
    pub directory: Style,
    pub symlink: Style,
    pub executable: Style,
    pub special: Style,
    pub marked: Style,
    pub flagged: Style,
    pub error: Style,
    pub changed: Style,
    pub header: Style,
    pub dim: Style,
    pub warning: Style,
    pub conflict: Style,
    pub success: Style,
    pub matched: Style,
    pub status: Style,
    // dired-ta2: mode line of unselected windows
    pub inactive: Style,
    pub point: Style,
    pub prompt: Style,
    pub glyphs: Glyphs,
}

fn truecolor() -> bool {
    std::env::var("COLORTERM")
        .map(|v| v.contains("truecolor") || v.contains("24bit"))
        .unwrap_or(false)
}

/// Nearest of the 16 ANSI colors, for terminals without truecolor.
fn nearest_ansi(r: u8, g: u8, b: u8) -> Color {
    const TABLE: [(Color, (i32, i32, i32)); 16] = [
        (Color::Black, (0, 0, 0)),
        (Color::Red, (205, 0, 0)),
        (Color::Green, (0, 205, 0)),
        (Color::Yellow, (205, 205, 0)),
        (Color::Blue, (0, 0, 238)),
        (Color::Magenta, (205, 0, 205)),
        (Color::Cyan, (0, 205, 205)),
        (Color::Gray, (229, 229, 229)),
        (Color::DarkGray, (127, 127, 127)),
        (Color::LightRed, (255, 0, 0)),
        (Color::LightGreen, (0, 255, 0)),
        (Color::LightYellow, (255, 255, 0)),
        (Color::LightBlue, (92, 92, 255)),
        (Color::LightMagenta, (255, 0, 255)),
        (Color::LightCyan, (0, 255, 255)),
        (Color::White, (255, 255, 255)),
    ];
    let (r, g, b) = (r as i32, g as i32, b as i32);
    TABLE
        .iter()
        .min_by_key(|(_, (cr, cg, cb))| (r - cr).pow(2) + (g - cg).pow(2) + (b - cb).pow(2))
        .map(|(c, _)| *c)
        .unwrap()
}

/// Parse a color name or `#rrggbb`, degrading hex to ANSI without truecolor.
pub fn parse_color(s: &str) -> Option<Color> {
    parse_color_with(s, truecolor())
}

fn parse_color_with(s: &str, truecolor: bool) -> Option<Color> {
    let s = s.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let v = u32::from_str_radix(hex, 16).ok()?;
            let (r, g, b) = ((v >> 16) as u8, (v >> 8) as u8, v as u8);
            return Some(if truecolor { Color::Rgb(r, g, b) } else { nearest_ansi(r, g, b) });
        }
        return None;
    }
    Some(match s.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "white" => Color::White,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "reset" | "default" => Color::Reset,
        _ => return None,
    })
}

const B: Modifier = Modifier::BOLD;
const I: Modifier = Modifier::ITALIC;
const U: Modifier = Modifier::UNDERLINED;
const N: Modifier = Modifier::empty();

/// A hex palette: `(fg, bg, modifiers)` per role, in [`ROLE_ORDER`].
type Palette = [(&'static str, &'static str, Modifier); 19];

const ROLE_ORDER: [&str; 19] = [
    "normal", "directory", "symlink", "executable", "special", "marked", "flagged", "error",
    "changed", "header", "dim", "warning", "conflict", "success", "matched", "status", "point",
    "prompt", "inactive",
];

const NORD: Palette = [
    ("#d8dee9", "", N),
    ("#88c0d0", "", B),
    ("#8fbcbb", "", I),
    ("#a3be8c", "", N),
    ("#616e88", "", N),
    ("#ebcb8b", "", B),
    ("#bf616a", "", B),
    ("#bf616a", "", N),
    ("#b48ead", "", I),
    ("#81a1c1", "", B),
    ("#616e88", "", N),
    ("#d08770", "", N),
    ("#b48ead", "", B),
    ("#a3be8c", "", N),
    ("#e5e9f0", "", N),
    ("#eceff4", "#3b4252", N),
    ("#eceff4", "#4c566a", B),
    ("#88c0d0", "", B),
    ("#7b88a1", "#2e3440", N),
];

const GRUVBOX: Palette = [
    ("#ebdbb2", "", N),
    ("#83a598", "", B),
    ("#8ec07c", "", I),
    ("#b8bb26", "", N),
    ("#928374", "", N),
    ("#fabd2f", "", B),
    ("#fb4934", "", B),
    ("#fb4934", "", N),
    ("#d3869b", "", I),
    ("#fe8019", "", B),
    ("#928374", "", N),
    ("#fe8019", "", N),
    ("#d3869b", "", B),
    ("#b8bb26", "", N),
    ("#ebdbb2", "", N),
    ("#ebdbb2", "#3c3836", N),
    ("#fbf1c7", "#504945", B),
    ("#fabd2f", "", B),
    ("#928374", "#282828", N),
];

const SOLARIZED_DARK: Palette = [
    ("#93a1a1", "", N),
    ("#268bd2", "", B),
    ("#2aa198", "", I),
    ("#859900", "", N),
    ("#586e75", "", N),
    ("#b58900", "", B),
    ("#dc322f", "", B),
    ("#dc322f", "", N),
    ("#6c71c4", "", I),
    ("#268bd2", "", B),
    ("#586e75", "", N),
    ("#cb4b16", "", N),
    ("#d33682", "", B),
    ("#859900", "", N),
    ("#93a1a1", "", N),
    ("#eee8d5", "#073642", N),
    ("#fdf6e3", "#586e75", B),
    ("#2aa198", "", B),
    ("#586e75", "#002b36", N),
];

const DRACULA: Palette = [
    ("#f8f8f2", "", N),
    ("#bd93f9", "", B),
    ("#8be9fd", "", I),
    ("#50fa7b", "", N),
    ("#6272a4", "", N),
    ("#f1fa8c", "", B),
    ("#ff5555", "", B),
    ("#ff5555", "", N),
    ("#ff79c6", "", I),
    ("#bd93f9", "", B),
    ("#6272a4", "", N),
    ("#ffb86c", "", N),
    ("#ff79c6", "", B),
    ("#50fa7b", "", N),
    ("#f8f8f2", "", N),
    ("#f8f8f2", "#44475a", N),
    ("#f8f8f2", "#6272a4", B),
    ("#8be9fd", "", B),
    ("#6272a4", "#282a36", N),
];

impl Theme {
    fn blank(name: &str, glyphs: Glyphs) -> Theme {
        let s = Style::default();
        Theme {
            name: name.to_string(),
            normal: s,
            directory: s,
            symlink: s,
            executable: s,
            special: s,
            marked: s,
            flagged: s,
            error: s,
            changed: s,
            header: s,
            dim: s,
            warning: s,
            conflict: s,
            success: s,
            matched: s,
            status: s,
            inactive: s,
            point: s,
            prompt: s,
            glyphs,
        }
    }

    pub fn slot_mut(&mut self, role: &str) -> Option<&mut Style> {
        Some(match role {
            "normal" => &mut self.normal,
            "directory" => &mut self.directory,
            "symlink" => &mut self.symlink,
            "executable" => &mut self.executable,
            "special" => &mut self.special,
            "marked" => &mut self.marked,
            "flagged" => &mut self.flagged,
            "error" => &mut self.error,
            "changed" => &mut self.changed,
            "header" => &mut self.header,
            "dim" => &mut self.dim,
            "warning" => &mut self.warning,
            "conflict" => &mut self.conflict,
            "success" => &mut self.success,
            "matched" => &mut self.matched,
            "status" => &mut self.status,
            "inactive" => &mut self.inactive,
            "point" => &mut self.point,
            "prompt" => &mut self.prompt,
            _ => return None,
        })
    }

    fn from_palette(name: &str, glyphs: Glyphs, p: &Palette, truecolor: bool) -> Theme {
        let mut t = Theme::blank(name, glyphs);
        for (role, (fg, bg, m)) in ROLE_ORDER.iter().zip(p.iter()) {
            let mut st = Style::default().add_modifier(*m);
            if let Some(c) = parse_color_with(fg, truecolor) {
                st = st.fg(c);
            }
            if let Some(c) = parse_color_with(bg, truecolor) {
                st = st.bg(c);
            }
            *t.slot_mut(role).unwrap() = st;
        }
        t
    }

    fn default_theme(glyphs: Glyphs) -> Theme {
        let s = Style::default();
        Theme {
            name: "default".into(),
            normal: s,
            directory: s.fg(Color::Blue).add_modifier(B),
            symlink: s.fg(Color::Cyan),
            executable: s.fg(Color::Green),
            special: s.fg(Color::DarkGray),
            marked: s.fg(Color::Yellow).add_modifier(B),
            flagged: s.fg(Color::Red).add_modifier(B),
            error: s.fg(Color::Red),
            changed: s.fg(Color::Magenta).add_modifier(I),
            header: s.add_modifier(B),
            dim: s.fg(Color::DarkGray),
            warning: s.fg(Color::Yellow),
            conflict: s.fg(Color::Magenta).add_modifier(B),
            success: s.fg(Color::Green),
            matched: s,
            status: s.add_modifier(Modifier::REVERSED),
            inactive: s.fg(Color::DarkGray).add_modifier(Modifier::REVERSED),
            point: s.add_modifier(Modifier::REVERSED),
            prompt: s.fg(Color::Cyan).add_modifier(B),
            glyphs,
        }
    }

    fn high_contrast(glyphs: Glyphs) -> Theme {
        let s = Style::default().fg(Color::White).bg(Color::Black);
        Theme {
            name: "high-contrast".into(),
            normal: s,
            directory: s.fg(Color::LightCyan).add_modifier(B),
            symlink: s.fg(Color::LightCyan).add_modifier(U),
            executable: s.fg(Color::LightGreen).add_modifier(B),
            special: s,
            marked: s.fg(Color::LightYellow).add_modifier(B | U),
            flagged: s.fg(Color::LightRed).add_modifier(B | U),
            error: s.fg(Color::LightRed).add_modifier(B),
            changed: s.fg(Color::LightMagenta).add_modifier(B),
            header: s.add_modifier(B | U),
            dim: s,
            warning: s.fg(Color::LightYellow).add_modifier(B),
            conflict: s.fg(Color::LightMagenta).add_modifier(B),
            success: s.fg(Color::LightGreen).add_modifier(B),
            matched: s,
            status: Style::default().fg(Color::Black).bg(Color::White).add_modifier(B),
            inactive: Style::default().fg(Color::White).bg(Color::DarkGray),
            point: Style::default().fg(Color::Black).bg(Color::LightYellow).add_modifier(B),
            prompt: s.fg(Color::LightYellow).add_modifier(B),
            glyphs,
        }
    }

    fn mono(glyphs: Glyphs) -> Theme {
        let mut t = Theme::blank("mono", glyphs);
        t.directory = t.directory.add_modifier(B);
        t.symlink = t.symlink.add_modifier(I);
        t.marked = t.marked.add_modifier(B);
        t.flagged = t.flagged.add_modifier(B | U);
        t.error = t.error.add_modifier(B);
        t.changed = t.changed.add_modifier(I);
        t.header = t.header.add_modifier(B);
        t.warning = t.warning.add_modifier(B);
        t.conflict = t.conflict.add_modifier(B | U);
        t.status = t.status.add_modifier(Modifier::REVERSED);
        t.inactive = t.inactive.add_modifier(U);
        t.point = t.point.add_modifier(Modifier::REVERSED);
        t.prompt = t.prompt.add_modifier(B);
        t
    }

    pub fn builtin(name: &str, glyphs: Glyphs) -> Theme {
        let tc = truecolor();
        match name {
            "high-contrast" => Theme::high_contrast(glyphs),
            "mono" => Theme::mono(glyphs),
            "nord" => Theme::from_palette(name, glyphs, &NORD, tc),
            "gruvbox" => Theme::from_palette(name, glyphs, &GRUVBOX, tc),
            "solarized-dark" => Theme::from_palette(name, glyphs, &SOLARIZED_DARK, tc),
            "dracula" => Theme::from_palette(name, glyphs, &DRACULA, tc),
            _ => Theme::default_theme(glyphs),
        }
    }

    pub fn from_config(cfg: &ThemeConfig) -> Theme {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let name = cfg.name.as_deref().unwrap_or(if no_color { "mono" } else { "default" });
        let mut t = Theme::builtin(name, cfg.glyphs.clone());
        if no_color && name == "mono" {
            return t; // NO_COLOR: ignore color overrides too
        }
        for (role, color) in &cfg.colors {
            if let (Some(c), Some(slot)) = (parse_color(color), t.slot_mut(role)) {
                *slot = slot.fg(c);
            }
        }
        for (role, spec) in &cfg.styles {
            if let Some(slot) = t.slot_mut(role) {
                *slot = apply_spec(*slot, spec);
            }
        }
        t
    }
}

fn apply_spec(mut st: Style, spec: &StyleSpec) -> Style {
    if let Some(c) = spec.fg.as_deref().and_then(parse_color) {
        st = st.fg(c);
    }
    if let Some(c) = spec.bg.as_deref().and_then(parse_color) {
        st = st.bg(c);
    }
    for (flag, m) in [
        (spec.bold, B),
        (spec.italic, I),
        (spec.underline, U),
        (spec.reversed, Modifier::REVERSED),
        (spec.dim, Modifier::DIM),
    ] {
        st = match flag {
            Some(true) => st.add_modifier(m),
            Some(false) => st.remove_modifier(m),
            None => st,
        };
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_builds_and_roles_resolve() {
        for name in dired_config::THEMES {
            let t = Theme::builtin(name, Glyphs::default());
            assert_eq!(&t.name, name);
        }
        let mut t = Theme::builtin("nord", Glyphs::default());
        for role in dired_config::STYLE_ROLES {
            assert!(t.slot_mut(role).is_some(), "{role}");
        }
        assert_eq!(ROLE_ORDER.as_slice(), dired_config::STYLE_ROLES);
    }

    #[test]
    fn palettes_degrade_without_truecolor() {
        let t = Theme::from_palette("nord", Glyphs::default(), &NORD, false);
        assert!(!matches!(t.directory.fg, Some(Color::Rgb(..))));
        let t = Theme::from_palette("nord", Glyphs::default(), &NORD, true);
        assert_eq!(t.directory.fg, Some(Color::Rgb(0x88, 0xc0, 0xd0)));
    }

    #[test]
    fn style_specs_override() {
        let spec = StyleSpec { bg: Some("red".into()), bold: Some(false), underline: Some(true), ..Default::default() };
        let st = apply_spec(Style::default().add_modifier(B), &spec);
        assert_eq!(st.bg, Some(Color::Red));
        assert!(st.add_modifier.contains(U) && !st.add_modifier.contains(B));
    }
}
