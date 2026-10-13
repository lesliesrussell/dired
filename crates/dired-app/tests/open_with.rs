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
    assert_eq!(got[0].1.path, PathBuf::from("/w/pic.png"));
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
    assert_eq!(got[0].1.path, PathBuf::from("/w/sub"));
}
