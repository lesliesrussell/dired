// dired-z5y
//! "Open terminal here" (`C-c t`) and "shell here" (`C-c s`).

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::*;
use dired_app::Effect;
use dired_testkit::{MockFs, MockTrash};

fn fixture() -> H {
    let fs = Arc::new(MockFs::new());
    fs.mkdir_p("/w/sub");
    fs.write("/w/a.txt", "A");
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    harness(fs, trash)
}

#[tokio::test]
async fn new_terminal_opens_in_the_buffers_directory() {
    let mut h = fixture();
    h.app.open_dir("/w/sub");
    h.settle().await;
    h.press("C-c t");
    assert_eq!(*h.opener.terminals.lock().unwrap(), [PathBuf::from("/w/sub")]);
    assert!(h.echo().contains("/w/sub"), "{}", h.echo());
}

#[tokio::test]
async fn new_terminal_uses_the_directory_not_the_file_at_point() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("a.txt");
    h.press("C-c t");
    assert_eq!(*h.opener.terminals.lock().unwrap(), [PathBuf::from("/w")]);
}

#[tokio::test]
async fn shell_here_suspends_the_ui_in_the_directory() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-c s");
    let effects = h.app.take_effects();
    let [Effect::Foreground(spec)] = effects.as_slice() else { panic!("{effects:?}") };
    assert_eq!(spec.cwd, PathBuf::from("/w"));
    assert_eq!(spec.program, std::ffi::OsString::from("mock-shell"));
}

#[tokio::test]
async fn commands_are_available_from_m_x() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.press("M-x");
    h.answer("dired-open-terminal");
    assert_eq!(h.opener.terminals.lock().unwrap().len(), 1);
    h.press("M-x");
    h.answer("dired-shell-here");
    assert_eq!(h.app.take_effects().len(), 1);
}

#[tokio::test]
async fn launch_failure_is_reported_not_fatal() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    *h.opener.terminal_error.lock().unwrap() = Some("cannot run kitty: not found".into());
    h.press("C-c t");
    assert_eq!(h.echo(), "cannot run kitty: not found");
}

#[tokio::test]
async fn buffers_without_a_directory_say_so() {
    let mut h = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.press("?");
    h.press("M-x");
    h.answer("dired-open-terminal");
    assert!(h.opener.terminals.lock().unwrap().is_empty());
    assert_eq!(h.echo(), "dired-open-terminal needs a directory buffer");
}
