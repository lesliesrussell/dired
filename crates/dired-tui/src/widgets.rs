// dired-p2d
//! Pure, individually testable pieces of the styled UI.

use std::time::{Duration, SystemTime};

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::render::truncate;
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

// dired-p2d
/// First visible candidate so that `selected` is within `view_rows` rows.
pub fn popup_window(selected: usize, view_rows: usize) -> usize {
    selected.saturating_sub(view_rows.saturating_sub(1))
}

/// Rectangle for the popup box: as wide as the longest candidate plus
/// borders and padding, as tall as the shown candidates plus borders, its
/// bottom edge just above `anchor`. Height 0 when there is no room.
pub fn popup_rect(screen: Rect, anchor: Rect, candidates: &[String], max_rows: usize) -> Rect {
    let rows = candidates.len().min(max_rows) as u16;
    let longest = candidates.iter().map(|c| c.width()).max().unwrap_or(0);
    let w = (longest.min(u16::MAX as usize) as u16).saturating_add(4).min(screen.width);
    let room = anchor.y.saturating_sub(screen.y);
    let h = (rows + 2).min(room);
    Rect::new(screen.x, anchor.y - h, w, h)
}

/// Draw the candidates in a rounded box floating above `anchor`.
pub fn draw_popup(
    f: &mut Frame,
    screen: Rect,
    anchor: Rect,
    candidates: &[String],
    selected: usize,
    max_rows: usize,
    theme: &Theme,
) {
    let r = popup_rect(screen, anchor, candidates, max_rows);
    if r.height < 3 || r.width < 5 {
        return;
    }
    f.render_widget(Clear, r);
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(theme.dim);
    let inner = block.inner(r);
    f.render_widget(block, r);
    let view_rows = inner.height as usize;
    let start = popup_window(selected, view_rows);
    let text_w = inner.width.saturating_sub(2) as usize;
    let lines: Vec<Line> = candidates
        .iter()
        .enumerate()
        .skip(start)
        .take(view_rows)
        .map(|(i, c)| {
            let st = if i == selected { theme.point } else { theme.normal };
            let t = truncate(c, text_w);
            let pad = text_w.saturating_sub(t.width());
            Line::from(Span::styled(format!(" {t}{} ", " ".repeat(pad)), st))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
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

    #[test]
    fn popup_window_keeps_selection_visible() {
        assert_eq!(popup_window(0, 6), 0);
        assert_eq!(popup_window(5, 6), 0);
        assert_eq!(popup_window(6, 6), 1);
        assert_eq!(popup_window(40, 6), 35);
        assert_eq!(popup_window(3, 0), 3); // degenerate: never underflows
    }

    #[test]
    fn popup_rect_sits_above_the_anchor_and_fits_the_screen() {
        let screen = Rect::new(0, 0, 40, 20);
        let anchor = Rect::new(0, 19, 40, 1);
        let c: Vec<String> = vec!["alpha".into(), "be".into()];
        assert_eq!(popup_rect(screen, anchor, &c, 6), Rect::new(0, 15, 9, 4));
        let many: Vec<String> = (0..50).map(|i| format!("{i:0>60}")).collect();
        let r = popup_rect(screen, anchor, &many, 6);
        assert_eq!((r.width, r.height, r.bottom()), (40, 8, 19));
        // No room above the anchor: empty, not an underflow.
        let r = popup_rect(Rect::new(0, 0, 40, 1), Rect::new(0, 0, 40, 1), &c, 6);
        assert_eq!(r.height, 0);
    }
}
