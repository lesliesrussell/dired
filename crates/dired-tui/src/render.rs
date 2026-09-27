// dired-7qs
//! Virtualized rendering: only the viewport rows are formatted.

use dired_app::buffers::{BufferKind, LineStyle, TextLine};
use dired_app::dirbuf::{DirBuffer, Row};
use dired_app::App;
use dired_domain::{format_mtime, format_size, EntryState, FileKind};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;

const MAX_CANDIDATES: usize = 6;

/// Escape control characters (ESC, BEL, CR, …) so filenames and process
/// output can never inject terminal sequences.
pub fn sanitize(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.chars().any(|c| c.is_control()) {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(dired_domain::display_name(std::ffi::OsStr::new(s)))
}

/// Truncate to `width` display columns, marking the cut with `…`.
/// Input is sanitized first.
pub fn truncate(s: &str, width: usize) -> String {
    let s = &*sanitize(s);
    if s.width() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

fn pad_right(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Perms,
    Size,
    Mtime,
}

impl Col {
    fn width(self) -> usize {
        match self {
            Col::Perms => 10,
            Col::Size => 6,
            Col::Mtime => 16,
        }
    }
}

fn columns(cfg: &[String]) -> Vec<Col> {
    cfg.iter()
        .filter_map(|c| match c.as_str() {
            "permissions" | "perms" | "mode" => Some(Col::Perms),
            "size" => Some(Col::Size),
            "mtime" | "time" | "modified" => Some(Col::Mtime),
            _ => None,
        })
        .collect()
}

pub fn draw(f: &mut Frame, app: &mut App, theme: &Theme) {
    let area = f.area();
    if app.buffers().is_empty() {
        f.render_widget(Paragraph::new(Span::styled("  Loading…", theme.dim)), area);
        return;
    }
    let cand_lines = app
        .prompt()
        .map(|p| p.candidates.len().min(MAX_CANDIDATES) as u16)
        .unwrap_or(0);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(cand_lines),
            Constraint::Length(1),
        ])
        .split(area);
    let body = chunks[0];
    app.set_viewport(body.height as usize);
    app.prepare_frame();
    let status = app.status_line();
    let hint = app.key_hint();
    let cols = columns(&app.config.display.columns);
    let mut cursor: Option<Position> = None;

    match &mut app.active_mut().kind {
        BufferKind::Dir(d) => cursor = draw_dir(f, body, d, theme, &cols),
        BufferKind::Text(t) => {
            let (point, top) = (t.point, t.top);
            draw_lines(f, body, &t.lines, top, Some(point), theme);
        }
        BufferKind::Preview(p) => {
            let (lines, first) = p.lines();
            draw_lines(f, body, &lines, p.top, Some(first + p.point), theme);
        }
    }

    // Status line (hint replaces the right part while a prefix is pending).
    let status_text = match hint {
        Some(h) => format!(" {}  │ {}", truncate(&status, (area.width as usize) / 2), h),
        None => format!(" {status}"),
    };
    f.render_widget(
        Paragraph::new(pad_right(&truncate(&status_text, area.width as usize), area.width as usize))
            .style(theme.status),
        chunks[1],
    );

    // Completion candidates.
    if let Some(p) = app.prompt() {
        if !p.candidates.is_empty() {
            let lines: Vec<Line> = p
                .candidates
                .iter()
                .take(MAX_CANDIDATES)
                .enumerate()
                .map(|(i, c)| {
                    let st = if i == p.selected { theme.point } else { theme.dim };
                    Line::from(Span::styled(truncate(c, area.width as usize), st))
                })
                .collect();
            f.render_widget(Paragraph::new(lines), chunks[2]);
        }
    }

    // Minibuffer / echo area.
    let mb = chunks[3];
    if let Some(p) = app.prompt() {
        let label_w = p.label.width();
        let before: String = sanitize(&p.input.chars().take(p.cursor).collect::<String>()).into_owned();
        let line = Line::from(vec![
            Span::styled(sanitize(&p.label).into_owned(), theme.prompt),
            Span::raw(sanitize(&p.input).into_owned()),
        ]);
        f.render_widget(Paragraph::new(line), mb);
        let x = (label_w + before.width()).min(mb.width.saturating_sub(1) as usize) as u16;
        cursor = Some(Position::new(mb.x + x, mb.y));
    } else if let Some((m, err)) = app.echo() {
        let st = if err { theme.error } else { theme.normal };
        f.render_widget(Paragraph::new(Span::styled(truncate(m, mb.width as usize), st)), mb);
    }
    if let Some(c) = cursor {
        f.set_cursor_position(c);
    }
}

fn line_style(s: LineStyle, t: &Theme) -> Style {
    match s {
        LineStyle::Normal => t.normal,
        LineStyle::Header => t.header,
        LineStyle::Dim => t.dim,
        LineStyle::Warning => t.warning,
        LineStyle::Error => t.error,
        LineStyle::Conflict => t.conflict,
        LineStyle::Success => t.success,
        LineStyle::Match => t.matched,
    }
}

fn draw_lines(f: &mut Frame, area: Rect, lines: &[TextLine], top: usize, point: Option<usize>, theme: &Theme) {
    let w = area.width as usize;
    let out: Vec<Line> = lines
        .iter()
        .enumerate()
        .skip(top)
        .take(area.height as usize)
        .map(|(i, l)| {
            let focused = point == Some(i);
            let glyph = if focused { &theme.glyphs.focused } else { " " };
            let text = truncate(&format!("{glyph}{}", l.text), w);
            let mut st = line_style(l.style, theme);
            if focused {
                st = st.patch(theme.point);
                return Line::from(Span::styled(pad_right(&text, w), st));
            }
            Line::from(Span::styled(text, st))
        })
        .collect();
    f.render_widget(Paragraph::new(out), area);
}

fn draw_dir(f: &mut Frame, area: Rect, d: &mut DirBuffer, theme: &Theme, cfg_cols: &[Col]) -> Option<Position> {
    let w = area.width as usize;
    let mut lines = Vec::with_capacity(area.height as usize);
    let mut header = format!("  {}:", d.root.display());
    let scan = d.scan_summary();
    if scan != "done" {
        header.push_str(&format!("  [{scan}]"));
    }
    lines.push(Line::from(Span::styled(truncate(&header, w), theme.header)));
    let height = area.height.saturating_sub(1) as usize;
    let rows: Vec<Row> = d.rows().iter().skip(d.top).take(height).cloned().collect();

    // Drop low-priority metadata before truncating names.
    let prefix_w = 4;
    let longest = rows
        .iter()
        .map(|r| d.display_name(r).width() + r.depth * 2 + 2)
        .max()
        .unwrap_or(10)
        .min(40);
    let mut cols: Vec<Col> = cfg_cols.to_vec();
    let used = |cols: &[Col]| prefix_w + cols.iter().map(|c| c.width() + 1).sum::<usize>() + 1;
    for drop in [Col::Perms, Col::Mtime, Col::Size] {
        if w >= used(&cols) + longest.max(12) {
            break;
        }
        cols.retain(|c| *c != drop);
    }
    let name_w = w.saturating_sub(used(&cols));
    let mut cursor = None;

    for (i, r) in rows.iter().enumerate() {
        let idx = d.top + i;
        let e = d.entry(r);
        let focused = idx == d.point;
        let state = d.state(&e.key);
        let edited = d.wdired.as_ref().map(|wd| wd.edits.contains_key(&e.key)).unwrap_or(false);
        let annotation = d.annotations.get(&e.key).cloned();
        let g = &theme.glyphs;
        let focus_glyph = if focused { g.focused.as_str() } else { " " };
        let state_glyph = match state {
            EntryState::Marked => g.marked.as_str(),
            EntryState::DeleteFlagged => g.flagged.as_str(),
            EntryState::Unmarked => " ",
        };
        let extra = if annotation.is_some() {
            g.error.as_str()
        } else if edited {
            g.changed.as_str()
        } else {
            " "
        };
        let state_style = match state {
            EntryState::Marked => theme.marked,
            EntryState::DeleteFlagged => theme.flagged,
            EntryState::Unmarked => theme.normal,
        };
        let mut spans = vec![
            Span::raw(focus_glyph.to_string()),
            Span::styled(state_glyph.to_string(), state_style),
            Span::styled(extra.to_string(), if annotation.is_some() { theme.error } else { theme.changed }),
            Span::raw(" "),
        ];
        let meta = e.meta.as_ref();
        for c in &cols {
            let text = match (c, meta) {
                (_, None) if e.special => " ".repeat(c.width()),
                (Col::Perms, Some(m)) => pad_right(&m.permissions_string(), c.width()),
                (Col::Size, Some(m)) => format!("{:>6}", format_size(m.size)),
                (Col::Mtime, Some(m)) => pad_right(&format_mtime(m.modified), c.width()),
                (_, None) => pad_right("…", c.width()),
            };
            spans.push(Span::styled(text, theme.dim));
            spans.push(Span::raw(" "));
        }
        let indent = "  ".repeat(r.depth);
        let name = d.display_name(r);
        let indicator = if d.wdired.is_some() { "" } else { e.indicator() };
        let target = meta
            .and_then(|m| m.symlink_target.as_ref())
            .map(|t| format!(" -> {}", t.display()))
            .unwrap_or_default();
        let name_style = if e.special {
            theme.special
        } else if edited {
            theme.changed
        } else {
            match e.kind {
                FileKind::Dir => theme.directory,
                FileKind::Symlink => theme.symlink,
                _ if meta.map(|m| m.executable()).unwrap_or(false) => theme.executable,
                _ => theme.normal,
            }
        };
        let name_room = name_w.saturating_sub(indent.width());
        let shown = truncate(&format!("{name}{indicator}"), name_room);
        let rest_room = name_room.saturating_sub(shown.width());
        let mut tail = truncate(&target, rest_room);
        if let Some(a) = &annotation {
            let room = rest_room.saturating_sub(tail.width());
            if room > 4 {
                tail.push_str(&truncate(&format!("  {} {a}", g.error), room));
            }
        }
        if focused {
            if let Some(wd) = &d.wdired {
                let before: String = name.chars().take(wd.col.min(name.chars().count())).collect();
                let x = spans.iter().map(|s| s.content.width()).sum::<usize>() + indent.width() + before.width();
                cursor = Some(Position::new(area.x + x.min(w.saturating_sub(1)) as u16, area.y + 1 + i as u16));
            }
        }
        spans.push(Span::raw(indent));
        spans.push(Span::styled(shown, name_style));
        spans.push(Span::styled(tail, if annotation.is_some() { theme.error } else { theme.dim }));
        let mut line = Line::from(spans);
        if focused {
            let used: usize = line.spans.iter().map(|s| s.content.width()).sum();
            if used < w {
                line.spans.push(Span::raw(" ".repeat(w - used)));
            }
            line = line.patch_style(theme.point);
        }
        lines.push(line);
    }
    if rows.is_empty() && !d.is_scanning() {
        lines.push(Line::from(Span::styled("  (empty)", theme.dim)));
    }
    f.render_widget(Paragraph::new(lines), area);
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_by_display_width() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(truncate("日本語ファイル", 5), "日本…");
        assert!(truncate("日本語ファイル", 5).width() <= 5);
        assert_eq!(truncate("a\x1b]52;c;x\x07b", 40), "a\\x1b]52;c;x\\x07b");
    }
}
