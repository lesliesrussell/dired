// dired-0cz
//! "Open with" (`O`): pick a program for the entry at point.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::*;
use dired_config::Config;
use dired_testkit::{MockFs, MockTrash};

fn config() -> Config {
    let (c, d) = Config::parse(
        r#"
version = 1
[[open_with]]
name = "Preview"
command = ["open", "-a", "Preview", "{file}"]
extensions = ["png", "JPG"]
[[open_with]]
name = "Neovim"
command = ["nvim", "{file}"]
terminal = true
"#,
        "t",
    );
    assert!(d.is_empty(), "{d:?}");
    c
}

fn fixture() -> H {
    let fs = Arc::new(MockFs::new());
    fs.write("/w/pic.png", "P");
    fs.write("/w/notes.txt", "N");
    fs.mkdir_p("/w/sub");
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    harness_with(fs, trash, config(), vec![])
}

#[tokio::test]
async fn offers_matching_candidates_and_opens_the_choice() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("pic.png");
    h.press("O");
    let p = h.app.prompt().expect("prompt");
    // Configured entries first (extension filter is case-insensitive), then built-ins.
    assert_eq!(p.candidates[..2], ["Preview", "Neovim"]);
    assert!(p.candidates.contains(&"System default".to_string()));
    h.answer("Preview");
    let got = h.opener.opened_with.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, "Preview");
    assert_eq!(got[0].1, [PathBuf::from("/w/pic.png")]);
    assert!(h.opener.opened.lock().unwrap().is_empty());
}

#[tokio::test]
async fn extension_filter_hides_non_matching_entries() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("notes.txt");
    h.press("O");
    let p = h.app.prompt().expect("prompt");
    assert!(!p.candidates.contains(&"Preview".to_string()), "{:?}", p.candidates);
    assert_eq!(p.candidates[0], "Neovim");
}

#[tokio::test]
async fn inexact_input_takes_the_selected_candidate() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("notes.txt");
    h.press("O");
    h.answer("neo");
    let got = h.opener.opened_with.lock().unwrap().clone();
    assert_eq!(got[0].0, "Neovim");
}

#[tokio::test]
async fn unknown_choice_is_an_error_and_nothing_runs() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("notes.txt");
    h.press("O");
    h.answer("zzzz-nope");
    assert!(h.opener.opened_with.lock().unwrap().is_empty());
    assert!(h.echo().contains("No such"), "{}", h.echo());
}

#[tokio::test]
async fn special_rows_are_refused() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("..");
    h.press("O");
    assert!(h.app.prompt().is_none());
}

#[tokio::test]
async fn directories_are_openable_with_unfiltered_candidates() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("sub");
    h.press("O");
    h.answer("System default");
    let got = h.opener.opened_with.lock().unwrap().clone();
    assert_eq!(got[0].1, [PathBuf::from("/w/sub")]);
}

// dired-vmx
fn multi_config() -> Config {
    let (c, d) = Config::parse(
        r#"
version = 1
[[open_with]]
name = "Preview"
command = ["open", "-a", "Preview", "{file}"]
extensions = ["png"]
[[open_with]]
name = "Viewer"
command = ["viewer", "{files}"]
"#,
        "t",
    );
    assert!(d.is_empty(), "{d:?}");
    c
}

fn multi_fixture() -> H {
    let fs = Arc::new(MockFs::new());
    for n in ["a.png", "b.png", "c.txt"] {
        fs.write(format!("/w/{n}"), "");
    }
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    harness_with(fs, trash, multi_config(), vec![])
}

#[tokio::test]
async fn marked_files_are_opened_together_and_win_over_point() {
    let mut h = multi_fixture();
    h.app.open_dir("/w");
    h.settle().await;
    for n in ["a.png", "b.png"] {
        h.goto(n);
        h.press("m");
    }
    h.goto("c.txt"); // point is on an unmarked file
    h.press("O");
    assert_eq!(h.app.prompt().unwrap().label, "Open 2 files with: ");
    h.answer("Preview");
    let got = h.opener.opened_with.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, "Preview");
    assert_eq!(got[0].1, [PathBuf::from("/w/a.png"), PathBuf::from("/w/b.png")]);
    assert_eq!(h.echo(), "Opened 2 files with Preview");
}

#[tokio::test]
async fn a_candidate_must_accept_every_selected_extension() {
    let mut h = multi_fixture();
    h.app.open_dir("/w");
    h.settle().await;
    for n in ["a.png", "c.txt"] {
        h.goto(n);
        h.press("m");
    }
    h.press("O");
    let c = h.app.prompt().unwrap().candidates.clone();
    assert!(!c.contains(&"Preview".to_string()), "png-only candidate hidden for a mixed selection: {c:?}");
    assert!(c.contains(&"Viewer".to_string()) && c.contains(&"System default".to_string()), "{c:?}");
    // With only pngs selected it is offered again.
    h.press("C-g");
    h.goto("c.txt");
    h.press("u"); // unmark c.txt
    h.press("O");
    assert!(h.app.prompt().unwrap().candidates.contains(&"Preview".to_string()));
}

#[tokio::test]
async fn nothing_marked_still_means_the_file_at_point() {
    let mut h = multi_fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("c.txt");
    h.press("O");
    assert_eq!(h.app.prompt().unwrap().label, "Open c.txt with: ");
    h.answer("Viewer");
    let got = h.opener.opened_with.lock().unwrap().clone();
    assert_eq!(got[0].1, [PathBuf::from("/w/c.txt")]);
}
