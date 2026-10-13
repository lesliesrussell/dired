// dired-99k
//! Git state in directory buffers: when git is asked, and what the mode line says.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::*;
use dired_app::git::GitStatus;
use dired_app::AppEvent;
use dired_config::Config;
use dired_testkit::{MockFs, MockTrash};

fn status(text: &str) -> GitStatus {
    GitStatus::parse(text.as_bytes())
}

fn harness_cfg(config: Config) -> (Arc<MockFs>, H) {
    let fs = Arc::new(MockFs::new());
    fs.write("/w/a.txt", "");
    fs.mkdir_p("/w/sub");
    fs.write("/w/sub/inner.txt", "");
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    let h = harness_with(fs.clone(), trash, config, vec![]);
    (fs, h)
}

fn calls(h: &H) -> Vec<PathBuf> {
    h.git.calls.lock().unwrap().clone()
}

fn mode_line(h: &mut H) -> String {
    h.app.status_line()
}

#[tokio::test]
async fn opening_a_directory_asks_git_once_and_the_mode_line_shows_the_branch() {
    let (_fs, mut h) = harness_cfg(Config::defaults());
    *h.git.answer.lock().unwrap() = Some(status("## main...o/main [ahead 1]\0 M a.txt\0"));
    h.app.open_dir("/w");
    h.settle().await;
    assert_eq!(calls(&h), [PathBuf::from("/w")]);
    assert!(mode_line(&mut h).contains("  git:main*↑1"), "{}", mode_line(&mut h));
    let st = h.dir().git.clone().expect("status stored on the buffer");
    assert_eq!(st.xy_for(std::path::Path::new("a.txt")).map(|x| x.text()).as_deref(), Some(" M"));
}

#[tokio::test]
async fn outside_a_repository_there_is_no_git_segment() {
    let (_fs, mut h) = harness_cfg(Config::defaults());
    h.app.open_dir("/w");
    h.settle().await;
    assert_eq!(calls(&h).len(), 1, "git was asked");
    assert!(!mode_line(&mut h).contains("git:"), "{}", mode_line(&mut h));
    assert!(h.dir().git.is_none());
}

#[tokio::test]
async fn disabled_in_the_config_git_is_never_run() {
    let (c, d) = Config::parse("version = 1\n[git]\nenabled = false\n", "t");
    assert!(d.is_empty());
    let (_fs, mut h) = harness_cfg(c);
    *h.git.answer.lock().unwrap() = Some(status("## main\0"));
    h.app.open_dir("/w");
    h.settle().await;
    assert!(calls(&h).is_empty());
    assert!(!mode_line(&mut h).contains("git:"));
}

#[tokio::test]
async fn reverting_asks_again_and_picks_up_changes() {
    let (_fs, mut h) = harness_cfg(Config::defaults());
    *h.git.answer.lock().unwrap() = Some(status("## main\0"));
    h.app.open_dir("/w");
    h.settle().await;
    assert!(mode_line(&mut h).contains("git:main") && !mode_line(&mut h).contains("git:main*"));
    *h.git.answer.lock().unwrap() = Some(status("## main\0 M a.txt\0"));
    h.press("g");
    h.settle().await;
    assert_eq!(calls(&h).len(), 2);
    assert!(mode_line(&mut h).contains("git:main*"), "{}", mode_line(&mut h));
    // And the repository can disappear (git removed, directory left a repo).
    *h.git.answer.lock().unwrap() = None;
    h.press("g");
    h.settle().await;
    assert!(!mode_line(&mut h).contains("git:"));
}

#[tokio::test]
async fn inserting_a_subdirectory_does_not_ask_git_again() {
    let (_fs, mut h) = harness_cfg(Config::defaults());
    *h.git.answer.lock().unwrap() = Some(status("## main\0 M sub/inner.txt\0"));
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("sub");
    h.press("i");
    h.settle().await;
    assert_eq!(calls(&h).len(), 1, "one status covers the whole tree below");
}

#[tokio::test]
async fn a_late_answer_for_an_older_request_is_ignored() {
    let (_fs, mut h) = harness_cfg(Config::defaults());
    *h.git.answer.lock().unwrap() = Some(status("## main\0"));
    h.app.open_dir("/w");
    h.settle().await;
    let buf = h.app.active_id();
    h.app.handle_event(AppEvent::GitReady { buf, gen: 0, status: Some(status("## stale\0")) });
    assert!(mode_line(&mut h).contains("git:main"), "{}", mode_line(&mut h));
    assert!(!mode_line(&mut h).contains("stale"));
}

#[tokio::test]
async fn an_answer_for_a_closed_buffer_is_dropped_quietly() {
    let (_fs, mut h) = harness_cfg(Config::defaults());
    h.app.handle_event(AppEvent::GitReady { buf: 9999, gen: 1, status: Some(status("## x\0")) });
    h.app.open_dir("/w");
    h.settle().await;
}
