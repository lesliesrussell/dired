// dired-7qs
//! Themes. State is never conveyed by color alone: glyphs carry it too.

use dired_config::{Glyphs, ThemeConfig};
use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone)]
pub struct Theme {
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

pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let v = u32::from_str_radix(hex, 16).ok()?;
            let (r, g, b) = ((v >> 16) as u8, (v >> 8) as u8, v as u8);
            return Some(if truecolor() { Color::Rgb(r, g, b) } else { nearest_ansi(r, g, b) });
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

impl Theme {
    fn default_theme(glyphs: Glyphs) -> Theme {
        let s = Style::default();
        Theme {
            normal: s,
            directory: s.fg(Color::Blue).add_modifier(Modifier::BOLD),
            symlink: s.fg(Color::Cyan),
            executable: s.fg(Color::Green),
            special: s.fg(Color::DarkGray),
            marked: s.fg(Color::Yellow).add_modifier(Modifier::BOLD),
            flagged: s.fg(Color::Red).add_modifier(Modifier::BOLD),
            error: s.fg(Color::Red),
            changed: s.fg(Color::Magenta).add_modifier(Modifier::ITALIC),
            header: s.add_modifier(Modifier::BOLD),
            dim: s.fg(Color::DarkGray),
            warning: s.fg(Color::Yellow),
            conflict: s.fg(Color::Magenta).add_modifier(Modifier::BOLD),
            success: s.fg(Color::Green),
            matched: s,
            status: s.add_modifier(Modifier::REVERSED),
            point: s.add_modifier(Modifier::REVERSED),
            prompt: s.fg(Color::Cyan).add_modifier(Modifier::BOLD),
            glyphs,
        }
    }

    fn high_contrast(glyphs: Glyphs) -> Theme {
        let s = Style::default().fg(Color::White).bg(Color::Black);
        let b = Modifier::BOLD;
        Theme {
            normal: s,
            directory: s.fg(Color::LightCyan).add_modifier(b),
            symlink: s.fg(Color::LightCyan).add_modifier(Modifier::UNDERLINED),
            executable: s.fg(Color::LightGreen).add_modifier(b),
            special: s,
            marked: s.fg(Color::LightYellow).add_modifier(b | Modifier::UNDERLINED),
            flagged: s.fg(Color::LightRed).add_modifier(b | Modifier::UNDERLINED),
            error: s.fg(Color::LightRed).add_modifier(b),
            changed: s.fg(Color::LightMagenta).add_modifier(b),
            header: s.add_modifier(b | Modifier::UNDERLINED),
            dim: s,
            warning: s.fg(Color::LightYellow).add_modifier(b),
            conflict: s.fg(Color::LightMagenta).add_modifier(b),
            success: s.fg(Color::LightGreen).add_modifier(b),
            matched: s,
            status: Style::default().fg(Color::Black).bg(Color::White).add_modifier(b),
            point: Style::default().fg(Color::Black).bg(Color::LightYellow).add_modifier(b),
            prompt: s.fg(Color::LightYellow).add_modifier(b),
            glyphs,
        }
    }

    fn mono(glyphs: Glyphs) -> Theme {
        let s = Style::default();
        Theme {
            normal: s,
            directory: s.add_modifier(Modifier::BOLD),
            symlink: s.add_modifier(Modifier::ITALIC),
            executable: s,
            special: s,
            marked: s.add_modifier(Modifier::BOLD),
            flagged: s.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            error: s.add_modifier(Modifier::BOLD),
            changed: s.add_modifier(Modifier::ITALIC),
            header: s.add_modifier(Modifier::BOLD),
            dim: s,
            warning: s.add_modifier(Modifier::BOLD),
            conflict: s.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            success: s,
            matched: s,
            status: s.add_modifier(Modifier::REVERSED),
            point: s.add_modifier(Modifier::REVERSED),
            prompt: s.add_modifier(Modifier::BOLD),
            glyphs,
        }
    }

    pub fn from_config(cfg: &ThemeConfig) -> Theme {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let name = cfg.name.as_deref().unwrap_or(if no_color { "mono" } else { "default" });
        let mut t = match name {
            "high-contrast" => Theme::high_contrast(cfg.glyphs.clone()),
            "mono" => Theme::mono(cfg.glyphs.clone()),
            _ => Theme::default_theme(cfg.glyphs.clone()),
        };
        for (role, color) in &cfg.colors {
            let Some(c) = parse_color(color) else { continue };
            let slot = match role.as_str() {
                "directory" => &mut t.directory,
                "symlink" => &mut t.symlink,
                "executable" => &mut t.executable,
                "marked" => &mut t.marked,
                "flagged" => &mut t.flagged,
                "error" => &mut t.error,
                "changed" => &mut t.changed,
                "header" => &mut t.header,
                "warning" => &mut t.warning,
                "conflict" => &mut t.conflict,
                "success" => &mut t.success,
                "prompt" => &mut t.prompt,
                "dim" => &mut t.dim,
                _ => continue,
            };
            *slot = slot.fg(c);
        }
        t
    }
}
