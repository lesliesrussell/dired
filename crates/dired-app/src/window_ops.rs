// dired-ta2
//! Window management on the application: selection, splitting, deletion,
//! per-window views, and the "other window" used by `C-x 4` and dwim copy.

use std::path::PathBuf;

use crate::app::App;
use crate::buffers::{BufferId, BufferKind};
use crate::windows::{can_split, sensible_split, Layout, Rect, SplitDir, ViewState, Window, WindowId};

fn clamp_top(top: usize, point: usize, height: usize) -> usize {
    let h = height.max(1);
    if point < top {
        point
    } else if point >= top + h {
        point + 1 - h
    } else {
        top
    }
}

impl App {
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    pub fn selected_window(&self) -> WindowId {
        self.selected
    }

    pub fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub(crate) fn window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// Size of the screen area available to windows (excluding the
    /// minibuffer). Sets the selected window's page size.
    pub fn set_window_area(&mut self, w: u16, h: u16) {
        self.area = Rect { x: 0, y: 0, w, h };
        if let Some(r) = self.selected_rect() {
            // dired-p2d
            self.viewport = (r.h as usize).saturating_sub(1 + self.border_rows).max(1);
        }
    }

    /// Every window with its rectangle and whether it is selected.
    pub fn window_rects(&self) -> Vec<(WindowId, Rect, bool)> {
        self.layout
            .rects(self.area)
            .into_iter()
            .map(|(id, r)| (id, r, id == self.selected))
            .collect()
    }

    pub(crate) fn selected_rect(&self) -> Option<Rect> {
        self.layout.rects(self.area).into_iter().find(|(id, _)| *id == self.selected).map(|(_, r)| r)
    }

    pub(crate) fn snapshot(&self, buf: BufferId) -> ViewState {
        match self.buffer(buf).map(|b| &b.kind) {
            Some(BufferKind::Dir(d)) => {
                let (point, top, key) = d.view_state();
                ViewState { point, top, key, follow: false }
            }
            Some(BufferKind::Text(t)) => ViewState { point: t.point, top: t.top, key: None, follow: t.follow },
            Some(BufferKind::Preview(p)) => ViewState { point: p.point, top: p.top, key: None, follow: false },
            None => ViewState::default(),
        }
    }

    pub(crate) fn apply_view(&mut self, buf: BufferId, v: &ViewState) {
        match self.buffer_mut(buf).map(|b| &mut b.kind) {
            Some(BufferKind::Dir(d)) => d.set_view_state(v.point, v.top, v.key.clone()),
            Some(BufferKind::Text(t)) => {
                let last = t.lines.len().saturating_sub(1);
                // A window that was following output keeps following it.
                t.follow = v.follow || v.point >= last;
                t.point = if t.follow { last } else { v.point.min(last) };
                t.top = v.top;
            }
            Some(BufferKind::Preview(p)) => {
                p.point = v.point.min(p.plan.items.len().saturating_sub(1));
                p.top = v.top;
            }
            None => {}
        }
    }

    fn touch_mru(&mut self, id: BufferId) {
        self.mru.retain(|b| *b != id);
        if self.buffer(id).is_none() {
            return; // never resurrect a closed buffer
        }
        self.mru.insert(0, id);
    }

    /// Make `w` the selected window, saving the old window's view and
    /// restoring the new one's.
    pub(crate) fn select_window(&mut self, w: WindowId) {
        if w == self.selected || self.window(w).is_none() {
            return;
        }
        if let Some(cur) = self.window(self.selected).map(|x| x.buffer) {
            let v = self.snapshot(cur);
            if let Some(win) = self.window_mut(self.selected) {
                win.view = v;
            }
        }
        self.selected = w;
        let (buf, v) = {
            let win = self.window(w).unwrap();
            (win.buffer, win.view.clone())
        };
        self.apply_view(buf, &v);
        self.active = buf;
        self.touch_mru(buf);
    }

    /// Show `buf` in the selected window (Emacs `switch-to-buffer`).
    pub(crate) fn show_in_selected(&mut self, buf: BufferId) {
        if self.windows.is_empty() {
            let id = self.next_win;
            self.next_win += 1;
            self.windows.push(Window { id, buffer: buf, view: ViewState::default(), popup: false });
            self.layout = Layout::Leaf(id);
            self.selected = id;
        } else {
            let v = self.snapshot(buf);
            let sel = self.selected;
            let win = self.window_mut(sel).unwrap();
            if win.buffer != buf {
                win.buffer = buf;
                win.popup = false;
                win.view = v;
            }
        }
        self.active = buf;
        self.touch_mru(buf);
    }

    /// Select a window already showing `buf`, else show it in the selected
    /// window (Emacs `pop-to-buffer`).
    pub(crate) fn pop_to(&mut self, buf: BufferId) {
        if self.window(self.selected).map(|w| w.buffer) == Some(buf) {
            self.active = buf;
            return self.touch_mru(buf);
        }
        match self.windows.iter().find(|w| w.buffer == buf).map(|w| w.id) {
            Some(w) => self.select_window(w),
            None => self.focus(buf),
        }
    }

    pub(crate) fn split_selected(&mut self, dir: SplitDir, popup: bool) -> Result<WindowId, String> {
        let r = self.selected_rect().unwrap_or(self.area);
        if !can_split(r, dir) {
            return Err(format!("Window {}x{} is too small to split", r.w, r.h));
        }
        let id = self.next_win;
        self.next_win += 1;
        let buf = self.active;
        let view = self.snapshot(buf);
        self.layout.split(self.selected, id, dir);
        self.windows.push(Window { id, buffer: buf, view, popup });
        Ok(id)
    }

    /// Remove window `w` from the layout (its buffer stays open).
    pub(crate) fn delete_window_raw(&mut self, w: WindowId) {
        if self.windows.len() <= 1 {
            return;
        }
        let next = self.layout.remove(w);
        self.windows.retain(|x| x.id != w);
        if self.selected == w {
            if let Some(n) = next {
                self.selected = n;
                let (buf, v) = {
                    let win = self.window(n).unwrap();
                    (win.buffer, win.view.clone())
                };
                self.apply_view(buf, &v);
                self.active = buf;
                self.touch_mru(buf);
            }
        }
    }

    pub(crate) fn delete_other_windows(&mut self) {
        let keep = self.selected;
        self.windows.retain(|w| w.id == keep);
        self.layout = Layout::Leaf(keep);
    }

    /// The next window after the selected one, in layout order.
    pub(crate) fn next_window(&self) -> Option<WindowId> {
        let leaves = self.layout.leaves();
        if leaves.len() < 2 {
            return None;
        }
        let i = leaves.iter().position(|w| *w == self.selected).unwrap_or(0);
        Some(leaves[(i + 1) % leaves.len()])
    }

    /// The other window, splitting sensibly when there is only one.
    pub(crate) fn ensure_other_window(&mut self) -> Result<WindowId, String> {
        if let Some(w) = self.next_window() {
            return Ok(w);
        }
        let r = self.selected_rect().unwrap_or(self.area);
        let dir = sensible_split(r).ok_or_else(|| format!("Window {}x{} is too small to split", r.w, r.h))?;
        self.split_selected(dir, false)
    }

    /// Directory shown in another window (Emacs `dired-dwim-target`).
    pub(crate) fn dwim_target(&self) -> Option<PathBuf> {
        let current = self.buffer(self.active).and_then(|b| b.dir()).map(|d| d.root.clone());
        let leaves = self.layout.leaves();
        let start = leaves.iter().position(|w| *w == self.selected)?;
        (1..leaves.len())
            .map(|k| leaves[(start + k) % leaves.len()])
            .filter_map(|w| self.window(w))
            .filter_map(|w| self.buffer(w.buffer).and_then(|b| b.dir()))
            .map(|d| d.root.clone())
            .find(|root| Some(root) != current.as_ref())
    }

    /// Resolved point and scroll for drawing window `w` with `height` body
    /// rows. The selected window uses its buffer's live state.
    pub fn render_view(&mut self, w: WindowId, height: usize) -> ViewState {
        let Some(win) = self.window(w).cloned() else { return ViewState::default() };
        if w == self.selected {
            return self.snapshot(win.buffer);
        }
        let mut v = win.view.clone();
        let hdr = self.dir_header_rows; // dired-p2d
        match self.buffer_mut(win.buffer).map(|b| &mut b.kind) {
            Some(BufferKind::Dir(d)) => {
                d.ensure_view();
                let n = d.rows().len();
                let found = v.key.as_ref().and_then(|k| d.find_row(k));
                v.point = found.unwrap_or(v.point).min(n.saturating_sub(1));
                v.top = clamp_top(v.top, v.point, height.saturating_sub(hdr));
            }
            Some(BufferKind::Text(t)) => {
                let last = t.lines.len().saturating_sub(1);
                v.point = if t.follow { last } else { v.point.min(last) };
                v.top = clamp_top(v.top, v.point, height);
            }
            Some(BufferKind::Preview(p)) => {
                let (_, first) = p.lines();
                v.point = v.point.min(p.plan.items.len().saturating_sub(1));
                if v.point == 0 {
                    v.top = 0;
                }
                v.top = clamp_top(v.top, first + v.point, height);
            }
            None => {}
        }
        if let Some(win) = self.window_mut(w) {
            win.view.top = v.top;
        }
        v
    }
}
