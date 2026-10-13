// dired-blw
//! Refreshing a directory buffer: on demand (`g`) and after a foreground
//! program (an editor) returns. Uses the real filesystem and the real git.

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use common::*;
use dired_app::SystemGit;
use dired_config::Config;
use dired_fs::LocalFs;
use dired_testkit::MockTrash;

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn git_available() -> bool {
    Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// A repository with a committed README.md, open in a real-git harness.
async fn repo() -> Option<(tempfile::TempDir, H)> {
    if !git_available() {
        eprintln!("git not installed; skipping");
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "trunk"]);
    std::fs::write(dir.path().join("README.md"), "one\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    let mut h = harness_git(
        Arc::new(LocalFs),
        Arc::new(MockTrash::default()),
        Config::defaults(),
        vec![],
        None,
        Some(Arc::new(SystemGit::default())),
    );
    h.app.open_dir(dir.path());
    h.settle().await;
    Some((dir, h))
}

fn readme_status(h: &mut H) -> Option<String> {
    h.dir().git.as_ref().and_then(|g| g.xy_for(Path::new("README.md"))).map(|x| x.text())
}

fn readme_size(h: &mut H) -> Option<u64> {
    h.dir().ensure_view();
    let d = h.dir();
    d.visible_entries().into_iter().find(|e| e.name == "README.md").and_then(|e| e.meta.map(|m| m.size))
}

#[tokio::test]
async fn g_picks_up_an_edit_in_the_listing_and_the_git_column() {
    let Some((dir, mut h)) = repo().await else { return };
    assert_eq!(readme_status(&mut h), None, "clean to begin with");
    assert_eq!(readme_size(&mut h), Some(4));
    std::fs::write(dir.path().join("README.md"), "one\ntwo\nthree\n").unwrap(); // edited elsewhere
    h.press("g");
    h.settle().await;
    assert_eq!(readme_status(&mut h).as_deref(), Some(" M"), "git column after g");
    assert_eq!(readme_size(&mut h), Some(14), "size after g");
}

#[tokio::test]
async fn returning_from_a_foreground_program_refreshes_what_is_on_screen() {
    let Some((dir, mut h)) = repo().await else { return };
    assert_eq!(readme_status(&mut h), None);
    // The user edits README.md in an editor that dired suspended itself for.
    std::fs::write(dir.path().join("README.md"), "one\ntwo\nthree\n").unwrap();
    h.app.foreground_finished();
    h.settle().await;
    assert_eq!(readme_status(&mut h).as_deref(), Some(" M"), "git column without pressing g");
    assert_eq!(readme_size(&mut h), Some(14), "size without pressing g");
}

#[tokio::test]
async fn every_directory_window_is_refreshed_not_only_the_selected_one() {
    let Some((dir, mut h)) = repo().await else { return };
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("x.txt"), "x").unwrap();
    h.press("C-x 4 d");
    h.press("C-a C-k");
    h.typ(&other.path().display().to_string());
    h.press("RET");
    h.settle().await;
    assert_eq!(h.app.windows().len(), 2);
    std::fs::write(dir.path().join("README.md"), "changed\n").unwrap();
    std::fs::write(other.path().join("new.txt"), "n").unwrap();
    h.app.foreground_finished();
    h.settle().await;
    // The selected window shows `other`; the repository sits in the other window.
    let names = h.names();
    assert!(names.iter().any(|n| n == "new.txt"), "selected window's listing: {names:?}");
    h.press("C-x o");
    assert_eq!(readme_status(&mut h).as_deref(), Some(" M"), "the other window's git column");
}

#[tokio::test]
async fn it_does_nothing_harmful_with_no_directory_buffers() {
    let (_fs, mut h) = mock();
    h.app.foreground_finished(); // no buffers at all
    h.settle().await;
}
