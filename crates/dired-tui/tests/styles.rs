// dired-p2d
//! Per-style rendering tests (ratatui TestBackend).

mod common;

use std::sync::Arc;

use common::*;
use dired_testkit::MockFs;

fn char_index(row: &str, needle: &str) -> usize {
    let byte = row.find(needle).unwrap_or_else(|| panic!("`{needle}` not in `{row}`"));
    row[..byte].chars().count()
}

#[tokio::test]
async fn size_column_is_colored_by_magnitude_unless_disabled() {
    for (extra, colored) in [("", true), ("[display.ui]\ncolumn_colors = false\n", false)] {
        let fs = Arc::new(MockFs::new());
        fs.write("/w/big.bin", &"x".repeat(5000)); // 4.9K: the "normal" tier
        fs.write("/w/z.txt", "");
        let c = cfg(&format!("version = 1\n[theme]\nname = \"nord\"\n{extra}"));
        let theme = theme_of(&c);
        let mut ui = Ui::open(fs, c, 80, 10, "/w").await;
        ui.app.press("M->"); // point on z.txt, so big.bin's row is not highlighted
        let rows = ui.draw();
        let (y, row) = rows.iter().enumerate().find(|(_, r)| r.contains("big.bin")).expect("row");
        let x = char_index(row, &dired_domain::format_size(5000)) as u16;
        let want = if colored { theme.normal.fg } else { theme.dim.fg };
        assert_eq!(ui.cell_fg(x, y as u16), want, "colored={colored}\n{}", rows.join("\n"));
    }
}

fn files(n: usize) -> Arc<MockFs> {
    let fs = Arc::new(MockFs::new());
    for i in 0..n {
        fs.write(format!("/w/f{i:02}"), "");
    }
    fs
}

fn last_col(rows: &[String], from: usize, to: usize) -> String {
    rows[from..to].iter().map(|r| r.chars().last().unwrap()).collect()
}

#[tokio::test]
async fn scrollbar_appears_only_when_content_overflows_and_is_enabled() {
    // 40 files in a 12-row terminal: overflow.
    let mut ui = Ui::open(files(40), cfg("version = 1\n"), 60, 12, "/w").await;
    let rows = ui.draw();
    assert!(last_col(&rows, 1, 10).contains('█'), "thumb expected:\n{}", rows.join("\n"));
    // Disabled by override.
    let mut ui = Ui::open(files(40), cfg("version = 1\n[display.ui]\nscrollbar = false\n"), 60, 12, "/w").await;
    let rows = ui.draw();
    assert!(!last_col(&rows, 1, 10).contains('█'), "{}", rows.join("\n"));
    // Everything fits: no scrollbar.
    let mut ui = Ui::open(files(3), cfg("version = 1\n"), 60, 12, "/w").await;
    let rows = ui.draw();
    assert!(!last_col(&rows, 1, 10).contains('█'), "{}", rows.join("\n"));
}

#[tokio::test]
async fn scrollbar_thumb_reaches_the_bottom_at_the_end() {
    let mut ui = Ui::open(files(40), cfg("version = 1\n"), 60, 12, "/w").await;
    ui.app.press("M->");
    let rows = ui.draw();
    let col = last_col(&rows, 2, 10); // rows below the 2-row clean header
    assert!(col.ends_with('█'), "thumb should touch the bottom: {col:?}\n{}", rows.join("\n"));
}

#[tokio::test]
async fn completion_popup_floats_above_the_minibuffer() {
    let mut ui = Ui::open(files(3), cfg("version = 1\n"), 60, 14, "/w").await;
    ui.app.press("M-x");
    let rows = ui.draw();
    let joined = rows.join("\n");
    assert!(rows[13].starts_with("M-x"), "{joined}");
    assert!(rows[5].starts_with('╭') && rows[12].starts_with('╰'), "{joined}");
    assert!(rows[6].contains("dired-open-directory"), "first candidate:\n{joined}");

    // popup = false: plain lines, no box.
    let mut ui = Ui::open(files(3), cfg("version = 1\n[display.ui]\npopup = false\n"), 60, 14, "/w").await;
    ui.app.press("M-x");
    let rows = ui.draw();
    assert!(!rows.join("\n").contains('╭'), "{}", rows.join("\n"));
    assert!(rows[7].contains("dired-open-directory"), "{}", rows.join("\n"));
}

#[tokio::test]
async fn mode_line_is_segmented_in_clean_and_plain_in_minimal() {
    let fs = files(3);
    let mut ui = Ui::open(fs.clone(), cfg("version = 1\n"), 80, 10, "/w").await;
    let rows = ui.draw();
    let mode = &rows[8];
    assert!(mode.contains(" │ ") && mode.contains("/4"), "clean: {mode:?}"); // "..", f00..f02 -> n/4
    let mut ui = Ui::open(fs, cfg("version = 1\n[display]\nstyle = \"minimal\"\n"), 80, 10, "/w").await;
    let rows = ui.draw();
    let mode = &rows[8];
    assert!(!mode.contains(" │ "), "minimal: {mode:?}");
    assert!(mode.contains("marked:"), "minimal keeps the old text: {mode:?}");
}

#[tokio::test]
async fn mono_theme_with_segmented_mode_line_does_not_break() {
    let c = cfg("version = 1\n[theme]\nname = \"mono\"\n[display.ui]\nicons = true\n");
    let mut ui = Ui::open(files(3), c, 80, 10, "/w").await;
    let rows = ui.draw();
    assert!(!rows[8].contains('\u{e0b0}'), "{:?}", rows[8]);
}

#[tokio::test]
async fn clean_draws_a_rule_under_the_header_and_minimal_does_not() {
    let fs = files(3);
    let mut ui = Ui::open(fs.clone(), cfg("version = 1\n"), 60, 10, "/w").await;
    let rows = ui.draw();
    assert!(rows[0].contains("/w"), "{}", rows.join("\n"));
    assert!(rows[1].chars().all(|c| c == '─'), "rule row: {:?}", rows[1]);
    assert!(rows[2].contains(".."), "{}", rows.join("\n"));
    let mut ui = Ui::open(fs, cfg("version = 1\n[display]\nstyle = \"minimal\"\n"), 60, 10, "/w").await;
    let rows = ui.draw();
    assert!(rows[1].contains(".."), "minimal has no rule:\n{}", rows.join("\n"));
}

#[tokio::test]
async fn clean_adds_a_left_gutter_and_wider_column_gaps() {
    let fs = files(1);
    let mut clean = Ui::open(fs.clone(), cfg("version = 1\n"), 80, 10, "/w").await;
    let mut mini = Ui::open(fs, cfg("version = 1\n[display]\nstyle = \"minimal\"\n"), 80, 10, "/w").await;
    let c = clean.draw();
    let m = mini.draw();
    let cr = c.iter().find(|r| r.contains("f00")).unwrap();
    let mr = m.iter().find(|r| r.contains("f00")).unwrap();
    // One extra gutter column plus one extra column per metadata column (3).
    assert_eq!(char_index(cr, "f00"), char_index(mr, "f00") + 1 + 3, "{cr:?} vs {mr:?}");
}

#[tokio::test]
async fn last_entry_stays_visible_in_every_style() {
    for style in ["minimal", "clean"] {
        let mut ui =
            Ui::open(files(40), cfg(&format!("version = 1\n[display]\nstyle = \"{style}\"\n")), 60, 12, "/w").await;
        ui.app.press("M->");
        let rows = ui.draw();
        assert!(rows.iter().any(|r| r.contains("f39")), "{style}:\n{}", rows.join("\n"));
    }
}

/// With a Wdired edit active the terminal cursor must sit on the character
/// being edited. `snippets` are config fragments to try.
async fn wdired_cursor_tracks_name_start(snippets: &[&str]) {
    for snippet in snippets {
        let mut ui = Ui::open(files(5), cfg(&format!("version = 1\n{snippet}")), 80, 12, "/w").await;
        ui.app.press("n n");
        ui.app.press("C-x C-q");
        let rows = ui.draw();
        let col = ui.app.active().dir().unwrap().wdired.as_ref().expect("wdired").col;
        let pos = ui.term.get_cursor_position().unwrap();
        let row = &rows[pos.y as usize];
        let name_start = char_index(row, "f0");
        // `col` starts as usize::MAX ("end of name"); the renderer clamps it to the name length (3).
        assert_eq!(pos.x as usize, name_start + col.min(3), "{snippet}\n{}", rows.join("\n"));
    }
}

#[tokio::test]
async fn wdired_cursor_tracks_name_start_minimal_and_clean() {
    wdired_cursor_tracks_name_start(&["[display]\nstyle = \"minimal\"\n", "[display]\nstyle = \"clean\"\n"]).await;
}

const RICH: &str = "version = 1\n[display]\nstyle = \"rich\"\n";

#[tokio::test]
async fn rich_draws_a_titled_rounded_frame_with_column_labels() {
    let mut ui = Ui::open(files(3), cfg(RICH), 60, 12, "/w").await;
    let rows = ui.draw();
    let joined = rows.join("\n");
    assert!(rows[0].starts_with('╭') && rows[0].ends_with('╮'), "{joined}");
    assert!(rows[0].contains("/w"), "title carries the path:\n{joined}");
    assert!(rows[1].starts_with('│') && rows[1].ends_with('│'), "{joined}");
    assert!(rows[1].contains("Size") && rows[1].contains("Name"), "column labels:\n{joined}");
    assert!(rows[9].starts_with('╰') && rows[9].ends_with('╯'), "{joined}");
    assert!(rows[10].contains("/4"), "mode line sits below the frame (n/4: .., f00..f02):\n{joined}");
}

#[tokio::test]
async fn rich_keeps_the_last_entry_visible_and_scrolls_inside_the_frame() {
    let mut ui = Ui::open(files(40), cfg(RICH), 60, 12, "/w").await;
    ui.app.press("M->");
    let rows = ui.draw();
    assert!(rows.iter().any(|r| r.contains("f39")), "{}", rows.join("\n"));
    // The scrollbar sits on the right border, between the corners.
    assert!(rows[2..9].iter().any(|r| r.ends_with('█')), "{}", rows.join("\n"));
}

#[tokio::test]
async fn two_rich_windows_get_two_frames() {
    let fs = files(3);
    fs.write("/o/x", "");
    let mut ui = Ui::open(fs, cfg(RICH), 100, 12, "/w").await;
    ui.app.press("C-x 4 d");
    ui.app.press("C-a C-k");
    ui.app.type_text("/o");
    ui.app.press("RET");
    ui.settle().await;
    let rows = ui.draw();
    assert_eq!(rows[0].matches('╭').count(), 2, "{}", rows.join("\n"));
    assert!(rows[0].contains("/w") && rows[0].contains("/o"), "{}", rows.join("\n"));
}

#[tokio::test]
async fn borders_can_be_turned_on_in_clean_and_off_in_rich() {
    let mut ui = Ui::open(files(3), cfg("version = 1\n[display.ui]\nborders = true\n"), 60, 12, "/w").await;
    assert!(ui.draw()[0].starts_with('╭'));
    let off = "version = 1\n[display]\nstyle = \"rich\"\n[display.ui]\nborders = false\n";
    let mut ui = Ui::open(files(3), cfg(off), 60, 12, "/w").await;
    assert!(!ui.draw()[0].starts_with('╭'));
}

/// Review focus 1. If a size panics in `minimal` too, the cause predates this
/// work: drop that size from the list and file a bead instead of fixing it here.
#[tokio::test]
async fn tiny_terminals_never_panic() {
    for style in ["minimal", "clean", "rich"] {
        for (w, h) in [(1u16, 1u16), (2, 2), (3, 3), (5, 4), (10, 4), (20, 3), (40, 5)] {
            let c = cfg(&format!("version = 1\n[display]\nstyle = \"{style}\"\n[display.ui]\nicons = true\n"));
            let mut ui = Ui::open(files(30), c, w, h, "/w").await;
            ui.draw();
            ui.app.press("M-x"); // popup in a tiny space
            ui.draw();
            ui.app.press("C-g");
            ui.app.press("M->");
            ui.draw();
        }
    }
}

/// Review focus 5: control characters in paths and names never reach the screen.
#[tokio::test]
async fn titles_and_popup_are_sanitized() {
    let fs = Arc::new(MockFs::new());
    fs.write("/we\u{1b}ird/\u{1b}]52;c;x\u{7}evil", "");
    for style in ["minimal", "clean", "rich"] {
        let c = cfg(&format!("version = 1\n[display]\nstyle = \"{style}\"\n"));
        let mut ui = Ui::open(fs.clone(), c, 80, 12, "/we\u{1b}ird").await;
        ui.app.press("C-x C-f"); // pre-filled path prompt plus completion popup
        let joined = ui.draw().join("\n");
        assert!(!joined.contains('\u{1b}') && !joined.contains('\u{7}'), "{style}:\n{joined}");
    }
}

#[tokio::test]
async fn wdired_cursor_tracks_name_start_in_rich() {
    wdired_cursor_tracks_name_start(&["[display]\nstyle = \"rich\"\n"]).await;
}

#[tokio::test]
async fn icons_follow_the_override_and_the_glyph_config() {
    let fs = files(1);
    fs.mkdir_p("/w/sub");
    let on = "version = 1\n[display.ui]\nicons = true\n[theme.glyphs]\nicon_dir = \"D\"\nicon_file = \"F\"\n";
    let mut ui = Ui::open(fs.clone(), cfg(on), 60, 12, "/w").await;
    let rows = ui.draw();
    let joined = rows.join("\n");
    assert!(rows.iter().any(|r| r.contains("D sub")), "{joined}");
    assert!(rows.iter().any(|r| r.contains("F f00")), "{joined}");

    let mut ui = Ui::open(fs.clone(), cfg("version = 1\n"), 60, 12, "/w").await;
    let rows = ui.draw();
    assert!(!rows.iter().any(|r| r.contains("D sub") || r.contains("F f00")), "icons are off in clean");

    // Rich enables icons; Nerd Font defaults appear.
    let mut ui = Ui::open(fs, cfg(RICH), 60, 12, "/w").await;
    let rows = ui.draw();
    assert!(rows.iter().any(|r| r.contains('\u{f07b}')), "{}", rows.join("\n"));
}

#[tokio::test]
async fn long_names_still_truncate_with_icons() {
    let fs = Arc::new(MockFs::new());
    fs.write(format!("/w/{}", "n".repeat(100)), "");
    let mut ui = Ui::open(fs, cfg(RICH), 50, 12, "/w").await;
    let rows = ui.draw();
    let row = rows.iter().find(|r| r.contains("nnnn")).expect("row");
    assert!(row.contains('…'), "{row:?}");
    assert!(row.chars().count() <= 50);
}

#[tokio::test]
async fn wdired_cursor_tracks_name_start_with_icons_everywhere() {
    wdired_cursor_tracks_name_start(&[
        "[display]\nstyle = \"minimal\"\n[display.ui]\nicons = true\n",
        "[display]\nstyle = \"clean\"\n[display.ui]\nicons = true\n",
        "[display]\nstyle = \"rich\"\n",
    ])
    .await;
}
