// dired-99k
//! The git status column and its colors.

mod common;

use std::sync::Arc;

use common::*;
use dired_app::git::GitStatus;
use dired_testkit::MockFs;

const MINIMAL: &str = "version = 1\n[theme]\nname = \"nord\"\n[display]\nstyle = \"minimal\"\n";

fn fixture() -> Arc<MockFs> {
    let fs = Arc::new(MockFs::new());
    for n in ["a.txt", "b.txt", "c.txt", "clean.txt"] {
        fs.write(format!("/w/{n}"), "");
    }
    fs.write("/w/sub/inner.txt", "");
    fs
}

fn st(text: &str) -> GitStatus {
    GitStatus::parse(text.as_bytes())
}

fn repo() -> GitStatus {
    st("## main...o/main [ahead 1]\0 M a.txt\0A  b.txt\0?? c.txt\0 M sub/inner.txt\0")
}

/// The two git characters of a `minimal` row: after the four prefix columns.
fn cell(row: &str) -> String {
    row.chars().skip(4).take(2).collect()
}

fn row_of<'a>(rows: &'a [String], name: &str) -> &'a String {
    rows.iter().find(|r| r.contains(name)).unwrap_or_else(|| panic!("{name} not in\n{}", rows.join("\n")))
}

#[tokio::test]
async fn each_row_shows_its_two_letter_status() {
    let mut ui = Ui::open_with_git(fixture(), cfg(MINIMAL), 90, 12, "/w", Some(repo())).await;
    let rows = ui.draw();
    for (name, want) in [("a.txt", " M"), ("b.txt", "A "), ("c.txt", "??"), ("clean.txt", "  "), ("sub/", " M")] {
        assert_eq!(cell(row_of(&rows, name)), want, "{name}\n{}", rows.join("\n"));
    }
    assert_eq!(cell(row_of(&rows, "../")), "  ", "the parent row has no status");
}

#[tokio::test]
async fn outside_a_repository_there_is_no_column_at_all() {
    let mut ui = Ui::open_with_git(fixture(), cfg(MINIMAL), 90, 12, "/w", None).await;
    let rows = ui.draw();
    assert_eq!(cell(row_of(&rows, "a.txt")), "-r", "permissions start where the git column would have been");
}

#[tokio::test]
async fn the_column_is_opt_in_through_columns_and_git_can_be_disabled() {
    let no_col = format!("{MINIMAL}columns = [\"permissions\", \"size\", \"mtime\"]\n");
    let mut ui = Ui::open_with_git(fixture(), cfg(&no_col), 90, 12, "/w", Some(repo())).await;
    assert_eq!(cell(row_of(&ui.draw(), "a.txt")), "-r");
    let off = format!("{MINIMAL}\n[git]\nenabled = false\n");
    let mut ui = Ui::open_with_git(fixture(), cfg(&off), 90, 12, "/w", Some(repo())).await;
    assert_eq!(cell(row_of(&ui.draw(), "a.txt")), "-r", "never asked, so nothing to show");
}

#[tokio::test]
async fn letters_are_colored_by_meaning() {
    let c = cfg(MINIMAL);
    let theme = theme_of(&c);
    let mut ui = Ui::open_with_git(fixture(), c, 90, 12, "/w", Some(repo())).await;
    ui.app.press("M->"); // point on the last row, so the rows under test are not highlighted
    let rows = ui.draw();
    let y = |name: &str| rows.iter().position(|r| r.contains(name)).unwrap() as u16;
    assert_eq!(ui.cell_fg(5, y("a.txt")), theme.warning.fg, "unstaged change");
    assert_eq!(ui.cell_fg(4, y("b.txt")), theme.success.fg, "staged");
    assert_eq!(ui.cell_fg(4, y("c.txt")), theme.dim.fg, "untracked");
    assert_eq!(ui.cell_fg(5, y("c.txt")), theme.dim.fg);
}

#[tokio::test]
async fn conflicts_and_deletions_are_shown_as_errors() {
    let c = cfg(MINIMAL);
    let theme = theme_of(&c);
    let s = st("## m\0UU a.txt\0 D b.txt\0D  c.txt\0");
    let mut ui = Ui::open_with_git(fixture(), c, 90, 12, "/w", Some(s)).await;
    ui.app.press("M->");
    let rows = ui.draw();
    let y = |name: &str| rows.iter().position(|r| r.contains(name)).unwrap() as u16;
    assert_eq!(cell(row_of(&rows, "a.txt")), "UU");
    assert_eq!(ui.cell_fg(4, y("a.txt")), theme.error.fg);
    assert_eq!(ui.cell_fg(5, y("b.txt")), theme.error.fg);
    assert_eq!(ui.cell_fg(4, y("c.txt")), theme.error.fg);
}

#[tokio::test]
async fn the_mode_line_shows_the_branch() {
    let mut ui = Ui::open_with_git(fixture(), cfg(MINIMAL), 100, 10, "/w", Some(repo())).await;
    let rows = ui.draw();
    assert!(rows[8].contains("git:main*↑1"), "{:?}", rows[8]);
}

#[tokio::test]
async fn narrow_terminals_drop_git_last() {
    let mut ui = Ui::open_with_git(fixture(), cfg(MINIMAL), 25, 12, "/w", Some(repo())).await;
    let rows = ui.draw();
    let r = row_of(&rows, "a.txt");
    assert_eq!(cell(r), " M", "git is the last column to go:\n{}", rows.join("\n"));
    assert!(!r.contains("-rw"), "permissions went first");
    let mut ui = Ui::open_with_git(fixture(), cfg(MINIMAL), 15, 12, "/w", Some(repo())).await;
    let rows = ui.draw();
    assert_ne!(cell(row_of(&rows, "a.txt")), " M", "at 15 columns even git makes way for names");
}

#[tokio::test]
async fn the_clean_style_shifts_the_column_by_its_gutter() {
    let c = cfg("version = 1\n[display]\nstyle = \"clean\"\n");
    let mut ui = Ui::open_with_git(fixture(), c, 90, 12, "/w", Some(repo())).await;
    let rows = ui.draw();
    let r = row_of(&rows, "a.txt");
    let got: String = r.chars().skip(5).take(2).collect();
    assert_eq!(got, " M", "{r:?}");
}

#[tokio::test]
async fn rich_labels_the_column() {
    let c = cfg("version = 1\n[display]\nstyle = \"rich\"\n");
    let mut ui = Ui::open_with_git(fixture(), c, 100, 12, "/w", Some(repo())).await;
    let rows = ui.draw();
    assert!(rows[1].contains("Git") && rows[1].contains("Mode"), "{}", rows[1]);
}

#[tokio::test]
async fn a_buffer_inside_a_repository_uses_its_prefix() {
    let s = st("## main\0 M sub/inner.txt\0").with_prefix("sub/");
    let mut ui = Ui::open_with_git(fixture(), cfg(MINIMAL), 90, 12, "/w/sub", Some(s)).await;
    assert_eq!(cell(row_of(&ui.draw(), "inner.txt")), " M");
}

#[tokio::test]
async fn inserted_subdirectories_show_their_own_entries() {
    let mut ui = Ui::open_with_git(fixture(), cfg(MINIMAL), 90, 14, "/w", Some(repo())).await;
    ui.draw();
    ui.app.press("n"); // from `..` onto `sub`, the first entry (directories sort first)
    ui.app.press("i");
    ui.settle().await;
    let rows = ui.draw();
    assert_eq!(cell(row_of(&rows, "inner.txt")), " M", "{}", rows.join("\n"));
}

#[tokio::test]
async fn the_terminal_cursor_still_tracks_the_name_in_wdired_with_the_git_column() {
    for style in ["minimal", "clean", "rich"] {
        let c = cfg(&format!("version = 1\n[display]\nstyle = \"{style}\"\n"));
        let mut ui = Ui::open_with_git(fixture(), c, 90, 14, "/w", Some(repo())).await;
        ui.app.press("n n n"); // onto a.txt, b.txt or so
        ui.app.press("C-x C-q");
        let rows = ui.draw();
        let col = ui.app.active().dir().unwrap().wdired.as_ref().expect("wdired").col;
        let pos = ui.term.get_cursor_position().unwrap();
        let row = &rows[pos.y as usize];
        let start = row.find(".txt").map(|i| row[..i].chars().count() - 1).expect("a file name on the cursor row");
        assert_eq!(pos.x as usize, start + col.min(5), "{style}\n{}", rows.join("\n"));
    }
}
