// dired-ta2
//! Emacs-style windows: a binary layout tree of splits whose leaves each
//! display one buffer with their own point and scroll position.

use std::path::PathBuf;

use crate::buffers::BufferId;

pub type WindowId = u64;

/// Smallest window, including its mode line.
pub const MIN_HEIGHT: u16 = 4;
pub const MIN_WIDTH: u16 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDir {
    /// Stacked: the new window goes below (`C-x 2`).
    Below,
    /// Side by side: the new window goes to the right (`C-x 3`).
    Right,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// Where a window's view of its buffer was when it was last selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewState {
    pub point: usize,
    pub top: usize,
    /// Identity of the entry at point (directory buffers).
    pub key: Option<PathBuf>,
    /// Output buffers: keep point on the last line as output arrives.
    pub follow: bool,
}

#[derive(Debug, Clone)]
pub struct Window {
    pub id: WindowId,
    pub buffer: BufferId,
    /// Snapshot used while the window is not selected.
    pub view: ViewState,
    /// Set when the window was popped up to show a preview; closing its
    /// buffer then deletes the window.
    pub popup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    Leaf(WindowId),
    Split { dir: SplitDir, a: Box<Layout>, b: Box<Layout> },
}

impl Layout {
    /// Replace leaf `target` by a split of `target` and `new`.
    pub fn split(&mut self, target: WindowId, new: WindowId, dir: SplitDir) -> bool {
        match self {
            Layout::Leaf(id) if *id == target => {
                *self = Layout::Split {
                    dir,
                    a: Box::new(Layout::Leaf(target)),
                    b: Box::new(Layout::Leaf(new)),
                };
                true
            }
            Layout::Leaf(_) => false,
            Layout::Split { a, b, .. } => a.split(target, new, dir) || b.split(target, new, dir),
        }
    }

    /// Remove leaf `target`; its sibling takes the parent's place. Returns
    /// the first leaf of that sibling (the natural window to select next).
    pub fn remove(&mut self, target: WindowId) -> Option<WindowId> {
        let Layout::Split { a, b, .. } = self else { return None };
        let sibling = if **a == Layout::Leaf(target) {
            Some((**b).clone())
        } else if **b == Layout::Leaf(target) {
            Some((**a).clone())
        } else {
            None
        };
        if let Some(s) = sibling {
            let first = s.leaves()[0];
            *self = s;
            return Some(first);
        }
        a.remove(target).or_else(|| b.remove(target))
    }

    pub fn leaves(&self) -> Vec<WindowId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<WindowId>) {
        match self {
            Layout::Leaf(id) => out.push(*id),
            Layout::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    /// Screen rectangles for every leaf. Side-by-side windows are separated
    /// by one column, which the renderer draws as a vertical rule.
    pub fn rects(&self, area: Rect) -> Vec<(WindowId, Rect)> {
        let mut out = Vec::new();
        self.place(area, &mut out);
        out
    }

    fn place(&self, r: Rect, out: &mut Vec<(WindowId, Rect)>) {
        match self {
            Layout::Leaf(id) => out.push((*id, r)),
            Layout::Split { dir: SplitDir::Below, a, b } => {
                let top = r.h.div_ceil(2);
                a.place(Rect { h: top, ..r }, out);
                b.place(Rect { y: r.y + top, h: r.h - top, ..r }, out);
            }
            Layout::Split { dir: SplitDir::Right, a, b } => {
                let left = r.w.saturating_sub(1) / 2;
                a.place(Rect { w: left, ..r }, out);
                b.place(Rect { x: r.x + left + 1, w: r.w.saturating_sub(left + 1), ..r }, out);
            }
        }
    }
}

/// Whether a window of this size can be split in `dir`.
pub fn can_split(r: Rect, dir: SplitDir) -> bool {
    match dir {
        SplitDir::Below => r.h >= 2 * MIN_HEIGHT,
        SplitDir::Right => r.w > 2 * MIN_WIDTH,
    }
}

/// Emacs's `split-window-sensibly`, simplified: side by side on wide
/// windows, otherwise stacked.
pub fn sensible_split(r: Rect) -> Option<SplitDir> {
    if r.w >= 100 && can_split(r, SplitDir::Right) {
        Some(SplitDir::Right)
    } else if can_split(r, SplitDir::Below) {
        Some(SplitDir::Below)
    } else if can_split(r, SplitDir::Right) {
        Some(SplitDir::Right)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect { x: 0, y: 0, w: 80, h: 24 };

    #[test]
    fn split_and_geometry() {
        let mut l = Layout::Leaf(1);
        assert!(l.split(1, 2, SplitDir::Right));
        assert_eq!(
            l.rects(AREA),
            vec![(1, Rect { x: 0, y: 0, w: 39, h: 24 }), (2, Rect { x: 40, y: 0, w: 40, h: 24 })]
        );
        assert!(l.split(2, 3, SplitDir::Below));
        assert_eq!(l.leaves(), vec![1, 2, 3]);
        let r = l.rects(AREA);
        assert_eq!(r[1].1, Rect { x: 40, y: 0, w: 40, h: 12 });
        assert_eq!(r[2].1, Rect { x: 40, y: 12, w: 40, h: 12 });
    }

    #[test]
    fn remove_promotes_sibling() {
        let mut l = Layout::Leaf(1);
        l.split(1, 2, SplitDir::Below);
        l.split(2, 3, SplitDir::Right);
        assert_eq!(l.remove(2), Some(3));
        assert_eq!(l.leaves(), vec![1, 3]);
        assert_eq!(l.remove(1), Some(3));
        assert_eq!(l, Layout::Leaf(3));
        assert_eq!(l.remove(3), None, "the sole window is never removed");
    }

    #[test]
    fn sensible_and_minimums() {
        assert_eq!(sensible_split(Rect { w: 160, h: 40, ..AREA }), Some(SplitDir::Right));
        assert_eq!(sensible_split(AREA), Some(SplitDir::Below));
        assert_eq!(sensible_split(Rect { w: 30, h: 6, ..AREA }), None);
    }
}
