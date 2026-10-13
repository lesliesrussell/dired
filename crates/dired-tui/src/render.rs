// dired-7qs
//! Virtualized rendering: only the viewport rows are formatted.

use dired_app::buffers::{BufferKind, LineStyle, TextLine};
use dired_app::dirbuf::{DirBuffer, Row};
use dired_app::App;
use dired_domain::{format_mtime, format_size, EntryState, FileKind};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use ratatui::Frame;
use std::time::SystemTime;

use dired_config::UiOptions;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;
use crate::widgets;

pub(crate) const MAX_CANDIDATES: usize = 6;

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
    Git, // dired-99k
}

impl Col {
    fn width(self) -> usize {
        match self {
            Col::Perms => 10,
            Col::Size => 6,
            Col::Mtime => 16,
            Col::Git => 3, // two status letters and a space
        }
    }
}

impl Col {
    // dired-p2d
    fn label(self) -> &'static str {
        match self {
            Col::Perms => "Mode",
            Col::Size => "Size",
            Col::Mtime => "Modified",
            Col::Git => "Git",
        }
    }
}

fn columns(cfg: &[String]) -> Vec<Col> {
    cfg.iter()
        .filter_map(|c| match c.as_str() {
            "permissions" | "perms" | "mode" => Some(Col::Perms),
            "size" => Some(Col::Size),
            "mtime" | "time" | "modified" => Some(Col::Mtime),
            "git" => Some(Col::Git), // dired-99k
            _ => None,
        })
        .collect()
}

// dired-p2d
/// Everything the drawing helpers need, resolved once per frame.
struct DrawCx<'a> {
    theme: &'a Theme,
    ui: &'a UiOptions,
    cols: &'a [Col],
    now: SystemTime,
}

pub fn draw(f: &mut Frame, app: &mut App, theme: &Theme) {
    let area = f.area();
    if app.buffers().is_empty() {
        f.render_widget(Paragraph::new(Span::styled("  Loading…", theme.dim)), area);
        return;
    }
    // dired-p2d
    let ui = UiOptions::resolve(&app.config.display);
    // dired-mv2: a popup needs room for a box above the minibuffer; very short
    // terminals fall back to inline candidate lines.
    let use_popup = ui.popup && area.height >= 4;
    let cand_lines = if use_popup {
        0 // the popup floats over the windows
    } else {
        app.prompt().map(|p| p.candidates.len().min(MAX_CANDIDATES) as u16).unwrap_or(0)
    };
    // dired-ta2: windows fill the screen above the minibuffer; each has a
    // mode line, and side-by-side windows are separated by a rule.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(cand_lines), Constraint::Length(1)])
        .split(area);
    let warea = chunks[0];
    // dired-p2d
    app.set_chrome(ui.border_rows(), ui.dir_header_rows());
    app.set_window_area(warea.width, warea.height);
    app.prepare_frame();
    let hint = app.key_hint();
    let cols = columns(&app.config.display.columns);
    let cx = DrawCx { theme, ui: &ui, cols: &cols, now: SystemTime::now() };
    let mut cursor: Option<Position> = None;

    for (wid, r, selected) in app.window_rects() {
        // dired-p2d
        if let Some(c) = draw_window(f, app, &cx, wid, r, selected, &hint, warea) {
            cursor = Some(c);
        }
    }

    // Completion candidates.
    if let Some(p) = app.prompt() {
        if !p.candidates.is_empty() {
            if use_popup {
                // dired-p2d
                widgets::draw_popup(f, area, chunks[2], &p.candidates, p.selected, MAX_CANDIDATES, theme);
            } else {
                // dired-t1z: scroll with the selection
                let start = widgets::popup_window(p.selected, MAX_CANDIDATES);
                let lines: Vec<Line> = p
                    .candidates
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(MAX_CANDIDATES)
                    .map(|(i, c)| {
                        let st = if i == p.selected { theme.point } else { theme.dim };
                        Line::from(Span::styled(truncate(c, area.width as usize), st))
                    })
                    .collect();
                f.render_widget(Paragraph::new(lines), chunks[1]);
            }
        }
    }

    // Minibuffer / echo area.
    let mb = chunks[2];
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

// dired-p2d
/// Draw one window: frame, body, scrollbar and mode line. Returns the cursor
/// position when this is the selected window and a Wdired edit is active.
#[allow(clippy::too_many_arguments)]
fn draw_window(
    f: &mut Frame,
    app: &mut App,
    cx: &DrawCx,
    wid: dired_app::windows::WindowId,
    r: dired_app::windows::Rect,
    selected: bool,
    hint: &Option<String>,
    warea: Rect,
) -> Option<Position> {
    let theme = cx.theme;
    let rect = Rect::new(warea.x + r.x, warea.y + r.y, r.w, r.h);
    if rect.width == 0 || rect.height == 0 {
        return None;
    }
    // Side-by-side windows are separated by a rule, unless frames do it.
    if r.x > 0 && !cx.ui.borders {
        let rule: Vec<Line> = (0..rect.height).map(|_| Line::from(Span::styled("│", theme.dim))).collect();
        f.render_widget(Paragraph::new(rule), Rect::new(rect.x - 1, rect.y, 1, rect.height));
    }
    let outer = Rect::new(rect.x, rect.y, rect.width, rect.height.saturating_sub(1));
    let mode = Rect::new(rect.x, rect.y + rect.height.saturating_sub(1), rect.width, 1);
    let body = if cx.ui.borders { Block::bordered().inner(outer) } else { outer };
    let buf = app.window(wid).map(|w| w.buffer)?;
    let view = app.render_view(wid, body.height as usize);
    let status = app.status_for(buf);
    let name = app.buffer(buf).map(|b| b.name.clone()).unwrap_or_default();

    // Content extent: (total rows, rows visible), the border title, and the
    // preview lines (computed once and reused for drawing).
    let dir_hdr = cx.ui.dir_header_rows();
    let mut preview: Option<(Vec<TextLine>, usize)> = None;
    let mut hdr_rows = 0;
    let mut has_pos = true;
    let mut title = format!(" {name} ");
    let (total, shown) = match app.buffer_mut(buf).map(|b| &mut b.kind) {
        Some(BufferKind::Dir(d)) => {
            d.ensure_view();
            hdr_rows = dir_hdr;
            let scan = d.scan_summary();
            let state = if scan != "done" { format!("  [{scan}]") } else { String::new() };
            title = format!(" {}{state} ", d.root.display());
            (d.rows().len(), (body.height as usize).saturating_sub(dir_hdr as usize))
        }
        Some(BufferKind::Text(t)) => (t.lines.len(), body.height as usize),
        Some(BufferKind::Preview(p)) => {
            has_pos = false;
            let (lines, first) = p.lines();
            let n = lines.len();
            preview = Some((lines, first));
            (n, body.height as usize)
        }
        None => (0, 0),
    };

    // Frame first, so the content and scrollbar draw over it.
    if cx.ui.borders && outer.width >= 2 && outer.height >= 2 {
        let accent = if selected { theme.header } else { theme.dim };
        let title = truncate(&title, (outer.width as usize).saturating_sub(4));
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(accent)
            .title(Line::from(Span::styled(title, accent)));
        f.render_widget(block, outer);
    }

    // Scrollbar: on the right border when framed, else in a reserved column.
    let show_sb = cx.ui.scrollbar && total > shown && body.width > 8 && shown > 0;
    let (body, sb) = if show_sb && cx.ui.borders {
        let top = outer.y + 1 + hdr_rows;
        let h = outer.height.saturating_sub(2 + hdr_rows);
        (body, Some(Rect::new(outer.x + outer.width - 1, top, 1, h)))
    } else if show_sb {
        let sb = Rect::new(body.x + body.width - 1, body.y + hdr_rows, 1, body.height.saturating_sub(hdr_rows));
        (Rect::new(body.x, body.y, body.width - 1, body.height), Some(sb))
    } else {
        (body, None)
    };

    let mut cursor = None;
    if body.width > 0 && body.height > 0 {
        match app.buffer_mut(buf).map(|b| &mut b.kind) {
            Some(BufferKind::Dir(d)) => {
                let c = draw_dir(f, body, d, cx, view.point, view.top, selected);
                if selected {
                    cursor = c;
                }
            }
            Some(BufferKind::Text(t)) => draw_lines(f, body, &t.lines, view.top, Some(view.point), theme, selected),
            Some(BufferKind::Preview(_)) => {
                if let Some((lines, first)) = &preview {
                    draw_lines(f, body, lines, view.top, Some(first + view.point), theme, selected);
                }
            }
            None => {}
        }
    }
    if let Some(sb) = sb {
        widgets::draw_scrollbar(f, sb, total, view.top, shown, theme);
    }

    // Mode line (the prefix-key hint shows in the selected window's).
    let w = mode.width as usize;
    match cx.ui.mode_line {
        dired_config::ModeLine::Segmented => {
            let pos = if total == 0 || !has_pos { None } else { Some((view.point + 1, total)) };
            let line =
                widgets::segmented_mode_line(theme, &name, &status, pos, hint.as_deref(), w, selected, cx.ui.icons);
            f.render_widget(Paragraph::new(line), mode);
        }
        dired_config::ModeLine::Plain => {
            let text = match (hint, selected) {
                (Some(h), true) => format!(" {}  │ {}", truncate(&status, w / 2), h),
                _ => format!(" {status}"),
            };
            let style = if selected { theme.status } else { theme.inactive };
            f.render_widget(Paragraph::new(pad_right(&truncate(&text, w), w)).style(style), mode);
        }
    }
    cursor
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

fn draw_lines(f: &mut Frame, area: Rect, lines: &[TextLine], top: usize, point: Option<usize>, theme: &Theme, active: bool) {
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
            // Only the selected window highlights point; others show the glyph.
            if focused && active {
                st = st.patch(theme.point);
                return Line::from(Span::styled(pad_right(&text, w), st));
            }
            Line::from(Span::styled(text, st))
        })
        .collect();
    f.render_widget(Paragraph::new(out), area);
}

#[allow(clippy::too_many_arguments)]
fn draw_dir(
    f: &mut Frame,
    area: Rect,
    d: &mut DirBuffer,
    cx: &DrawCx,
    point: usize,
    top: usize,
    active: bool,
) -> Option<Position> {
    let theme = cx.theme; // dired-p2d
    let cfg_cols = cx.cols;
    let w = area.width as usize;
    // dired-p2d
    let hdr_rows = cx.ui.dir_header_rows();
    let gutter = if cx.ui.roomy { 1 } else { 0 };
    let gap = if cx.ui.roomy { 2 } else { 1 };
    let mut lines = Vec::with_capacity(area.height as usize);
    let height = area.height.saturating_sub(hdr_rows) as usize;
    let rows: Vec<Row> = d.rows().iter().skip(top).take(height).cloned().collect();

    // Drop low-priority metadata before truncating names.
    let prefix_w = 4 + gutter; // dired-p2d
    let longest = rows
        .iter()
        .map(|r| d.display_name(r).width() + r.depth * 2 + 2)
        .max()
        .unwrap_or(10)
        .min(40);
    let mut cols: Vec<Col> = cfg_cols.to_vec();
    if d.git.is_none() {
        cols.retain(|c| *c != Col::Git); // dired-99k: no column outside a repository
    }
    let used = |cols: &[Col]| prefix_w + cols.iter().map(|c| c.width() + gap).sum::<usize>() + 1;
    for drop in [Col::Perms, Col::Mtime, Col::Size, Col::Git] {
        if w >= used(&cols) + longest.max(12) {
            break;
        }
        cols.retain(|c| *c != drop);
    }
    let name_w = w.saturating_sub(used(&cols));
    // dired-p2d
    lines.extend(dir_header_lines(d, cx, w, prefix_w, &cols, gap));
    let mut cursor = None;

    for (i, r) in rows.iter().enumerate() {
        let idx = top + i;
        let e = d.entry(r);
        let focused = idx == point;
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
        let mut spans = Vec::new();
        if gutter > 0 {
            spans.push(Span::raw(" ".repeat(gutter))); // dired-p2d
        }
        spans.extend([
            Span::raw(focus_glyph.to_string()),
            Span::styled(state_glyph.to_string(), state_style),
            Span::styled(extra.to_string(), if annotation.is_some() { theme.error } else { theme.changed }),
            Span::raw(" "),
        ]);
        let meta = e.meta.as_ref();
        for c in &cols {
            // dired-99k: the status letters, each colored by what it means
            if *c == Col::Git {
                let xy = if e.special { None } else { d.git.as_ref().and_then(|g| g.xy_for(&e.key)) };
                let (x, y) = xy.map(|v| (v.x, v.y)).unwrap_or((' ', ' '));
                spans.push(Span::styled(x.to_string(), widgets::git_style(theme, x, true)));
                spans.push(Span::styled(y.to_string(), widgets::git_style(theme, y, false)));
                spans.push(Span::raw(" ".repeat(c.width() - 2 + gap)));
                continue;
            }
            let text = match (c, meta) {
                (_, None) if e.special => " ".repeat(c.width()),
                (Col::Perms, Some(m)) => pad_right(&m.permissions_string(), c.width()),
                (Col::Size, Some(m)) => format!("{:>6}", format_size(m.size)),
                (Col::Mtime, Some(m)) => pad_right(&format_mtime(m.modified), c.width()),
                (_, None) => pad_right("…", c.width()),
                (Col::Git, _) => unreachable!("handled above"), // dired-99k
            };
            // dired-p2d
            let style = match (c, meta, cx.ui.column_colors) {
                (Col::Size, Some(m), true) => widgets::size_style(theme, m.size),
                (Col::Mtime, Some(m), true) => widgets::age_style(theme, m.modified, cx.now),
                _ => theme.dim,
            };
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(" ".repeat(gap))); // dired-p2d
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
        // dired-p2d
        let icon = if cx.ui.icons {
            let exec = meta.map(|m| m.executable()).unwrap_or(false);
            Some(widgets::icon_for(g, e.special, e.kind, exec).to_string())
        } else {
            None
        };
        let icon_w = icon.as_ref().map(|i| i.width() + 1).unwrap_or(0);
        let name_room = name_w.saturating_sub(indent.width() + icon_w);
        let shown = truncate(&format!("{name}{indicator}"), name_room);
        let rest_room = name_room.saturating_sub(shown.width());
        let mut tail = truncate(&target, rest_room);
        if let Some(a) = &annotation {
            let room = rest_room.saturating_sub(tail.width());
            if room > 4 {
                tail.push_str(&truncate(&format!("  {} {a}", g.error), room));
            }
        }
        if focused && active {
            if let Some(wd) = &d.wdired {
                let before: String = name.chars().take(wd.col.min(name.chars().count())).collect();
                let x = spans.iter().map(|s| s.content.width()).sum::<usize>() + indent.width() + icon_w + before.width();
                cursor = Some(Position::new(area.x + x.min(w.saturating_sub(1)) as u16, area.y + hdr_rows + i as u16));
            }
        }
        spans.push(Span::raw(indent));
        if let Some(i) = icon {
            spans.push(Span::styled(format!("{i} "), name_style)); // dired-p2d
        }
        spans.push(Span::styled(shown, name_style));
        spans.push(Span::styled(tail, if annotation.is_some() { theme.error } else { theme.dim }));
        let mut line = Line::from(spans);
        if focused && active {
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

// dired-p2d
/// The directory header: the path (with scan state) and, in `clean`, a rule.
fn dir_header_lines(
    d: &DirBuffer,
    cx: &DrawCx,
    w: usize,
    prefix_w: usize,
    cols: &[Col],
    gap: usize,
) -> Vec<Line<'static>> {
    let theme = cx.theme;
    if cx.ui.borders {
        // The path is in the border title; the header row labels the columns.
        let mut s = " ".repeat(prefix_w);
        for c in cols {
            // dired-mv2: sizes are right-aligned, so their label is too
            if *c == Col::Size {
                s.push_str(&format!("{:>w$}", c.label(), w = c.width()));
            } else {
                s.push_str(&pad_right(c.label(), c.width()));
            }
            s.push_str(&" ".repeat(gap));
        }
        s.push_str("Name");
        let mut out = vec![Line::from(Span::styled(truncate(&s, w), theme.header))];
        if cx.ui.header_rule {
            out.push(Line::from(Span::styled("─".repeat(w), theme.dim)));
        }
        return out;
    }
    let gutter = if cx.ui.roomy { " " } else { "" };
    let mut header = format!("{gutter}  {}:", d.root.display());
    let scan = d.scan_summary();
    if scan != "done" {
        header.push_str(&format!("  [{scan}]"));
    }
    let mut out = vec![Line::from(Span::styled(truncate(&header, w), theme.header))];
    if cx.ui.header_rule {
        out.push(Line::from(Span::styled("─".repeat(w), theme.dim)));
    }
    out
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
