// dired-p2d
//! Pure, individually testable pieces of the styled UI.

use std::time::{Duration, SystemTime};

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use crate::theme::Theme;

/// Size color by magnitude: < 1 KiB dim, < 1 MiB normal, < 1 GiB warning, larger error.
pub fn size_style(theme: &Theme, size: u64) -> Style {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    match size {
        s if s < KIB => theme.dim,
        s if s < MIB => theme.normal,
        s if s < GIB => theme.warning,
        _ => theme.error,
    }
}

/// Modification-time color by age: < 1 day success, < 30 days normal,
/// < 1 year dim, older dim italic. Unknown times are dim.
pub fn age_style(theme: &Theme, modified: Option<SystemTime>, now: SystemTime) -> Style {
    const DAY: u64 = 86_400;
    let Some(t) = modified else { return theme.dim };
    let age = now.duration_since(t).unwrap_or(Duration::ZERO).as_secs();
    match age {
        a if a < DAY => theme.success,
        a if a < 30 * DAY => theme.normal,
        a if a < 365 * DAY => theme.dim,
        _ => theme.dim.add_modifier(Modifier::ITALIC),
    }
}

/// Draw a one-column vertical scrollbar into `area`. `total` rows of content,
/// `view` of them visible starting at `top`.
pub fn draw_scrollbar(f: &mut Frame, area: Rect, total: usize, top: usize, view: usize, theme: &Theme) {
    if area.width == 0 || area.height == 0 || total <= view {
        return;
    }
    // Positions run 0..=total-view so the thumb touches the bottom at the end.
    let mut state = ScrollbarState::new(total - view + 1).position(top).viewport_content_length(view);
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .thumb_symbol("█")
        .style(theme.dim)
        .thumb_style(theme.normal);
    f.render_stateful_widget(bar, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use dired_config::Glyphs;

    fn nord() -> Theme {
        Theme::builtin("nord", Glyphs::default())
    }

    #[test]
    fn size_tiers() {
        let t = nord();
        assert_eq!(size_style(&t, 0), t.dim);
        assert_eq!(size_style(&t, 1023), t.dim);
        assert_eq!(size_style(&t, 1024), t.normal);
        assert_eq!(size_style(&t, 1024 * 1024 - 1), t.normal);
        assert_eq!(size_style(&t, 1024 * 1024), t.warning);
        assert_eq!(size_style(&t, 1024 * 1024 * 1024), t.error);
    }

    #[test]
    fn age_tiers() {
        let t = nord();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(400 * 86_400);
        let ago = |d: u64| Some(now - Duration::from_secs(d * 86_400));
        assert_eq!(age_style(&t, ago(0), now), t.success);
        assert_eq!(age_style(&t, ago(2), now), t.normal);
        assert_eq!(age_style(&t, ago(100), now), t.dim);
        assert_eq!(age_style(&t, ago(399), now), t.dim.add_modifier(Modifier::ITALIC));
        assert_eq!(age_style(&t, None, now), t.dim);
        // A timestamp in the future counts as "just now".
        assert_eq!(age_style(&t, Some(now + Duration::from_secs(5)), now), t.success);
    }
}
