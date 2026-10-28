// dired-9q8
//! "Quit here" (`C-c q`): leave dired and hand the directory to the shell.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::*;
use dired_testkit::{MockFs, MockTrash};

fn fixture() -> H {
    let fs = Arc::new(MockFs::new());
    fs.mkdir_p("/w/sub");
    fs.write("/w/a.txt", "A");
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    harness(fs, trash)
}

#[tokio::test]
async fn quits_and_records_the_buffers_directory() {
    let mut h = fixture();
    h.app.set_cwd_handoff(true);
    h.app.open_dir("/w/sub");
    h.settle().await;
    h.press("C-c q");
    assert!(h.app.should_quit());
    assert_eq!(h.app.exit_dir(), Some(PathBuf::from("/w/sub").as_path()));
}

#[tokio::test]
async fn uses_the_directory_not_the_file_at_point() {
    let mut h = fixture();
    h.app.set_cwd_handoff(true);
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("sub");
    h.press("C-c q");
    assert_eq!(h.app.exit_dir(), Some(PathBuf::from("/w").as_path()));
}

#[tokio::test]
async fn refuses_without_a_cwd_file() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-c q");
    assert!(!h.app.should_quit());
    assert_eq!(h.app.exit_dir(), None);
    assert!(h.echo().contains("--cwd-file"), "{}", h.echo());
}

#[tokio::test]
async fn plain_quit_leaves_no_directory() {
    let mut h = fixture();
    h.app.set_cwd_handoff(true);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-x C-c");
    assert!(h.app.should_quit());
    assert_eq!(h.app.exit_dir(), None);
}

#[tokio::test]
async fn is_available_from_m_x() {
    let mut h = fixture();
    h.app.set_cwd_handoff(true);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("M-x");
    h.answer("dired-quit-here");
    assert_eq!(h.app.exit_dir(), Some(PathBuf::from("/w").as_path()));
}
