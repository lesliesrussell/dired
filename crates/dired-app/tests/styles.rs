// dired-p2d
//! `cycle-style` and chrome-aware scrolling.

mod common;

use common::*;
use dired_config::Config;

#[tokio::test]
async fn cycle_style_walks_the_presets() {
    let (fs, mut h) = mock();
    fs.write("/w/a", "");
    h.app.open_dir("/w");
    h.settle().await;
    assert_eq!(h.app.config.display.style, "clean");
    for expect in ["minimal", "rich", "clean"] {
        h.press("M-x");
        h.answer("cycle-style");
        assert_eq!(h.app.config.display.style, expect);
        assert_eq!(h.echo(), format!("Style: {expect}"));
    }
}

#[tokio::test]
async fn cycle_style_mentions_overrides() {
    let (c, d) = Config::parse("version = 1\n[display.ui]\nborders = true\n", "t");
    assert!(d.is_empty());
    let fs = std::sync::Arc::new(dired_testkit::MockFs::new());
    fs.write("/w/a", "");
    let trash = std::sync::Arc::new(dired_testkit::MockTrash::with_fs(fs.clone()));
    let mut h = harness_with(fs, trash, c, vec![]);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("M-x");
    h.answer("cycle-style");
    assert_eq!(h.echo(), "Style: minimal (with overrides)");
}

#[tokio::test]
async fn chrome_rows_shrink_the_scroll_window() {
    let (fs, mut h) = mock();
    for i in 0..40 {
        fs.write(format!("/w/f{i:02}"), "");
    }
    h.app.open_dir("/w");
    h.settle().await;
    h.app.set_chrome(0, 1);
    h.app.set_window_area(80, 12);
    assert_eq!(h.app.viewport(), 11);
    h.app.set_chrome(2, 2);
    h.app.set_window_area(80, 12);
    assert_eq!(h.app.viewport(), 9);
    // Point stays inside the rows the renderer can show: viewport minus header rows.
    h.press("M->");
    h.app.prepare_frame();
    let (top, point) = {
        let d = h.dir();
        (d.top, d.point)
    };
    assert!(point >= top && point < top + 7, "top {top} point {point}");
}

#[tokio::test]
async fn paging_keeps_one_line_of_overlap_whatever_the_header_height() {
    for (hdr, expect) in [(1u16, 9usize), (2, 8)] {
        let (fs, mut h) = mock();
        for i in 0..60 {
            fs.write(format!("/w/f{i:02}"), "");
        }
        h.app.open_dir("/w");
        h.settle().await;
        h.app.set_chrome(0, hdr);
        h.app.set_window_area(80, 12); // viewport 11: 11 - hdr visible rows
        h.app.prepare_frame();
        let before = h.dir().point;
        h.press("C-v");
        let after = h.dir().point;
        assert_eq!(after - before, expect, "header rows {hdr}: one line must stay visible");
    }
}
