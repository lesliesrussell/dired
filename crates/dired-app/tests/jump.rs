// dired-s23
//! The jump picker (`J`), bookmarks (`C-x r m`, `C-x r d`) and visit tracking.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::*;
use dired_config::Config;
use dired_testkit::{MockFs, MockTrash};

fn fs_with(dirs: &[&str]) -> Arc<MockFs> {
    let fs = Arc::new(MockFs::new());
    for d in dirs {
        fs.mkdir_p(d);
        fs.write(format!("{d}/file"), "");
    }
    fs
}

fn harness(fs: &Arc<MockFs>) -> H {
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    harness_with(fs.clone(), trash, Config::defaults(), vec![])
}

async fn visit(h: &mut H, dirs: &[&str]) {
    for d in dirs {
        h.app.open_dir(d);
        h.settle().await;
    }
}

fn candidates(h: &H) -> Vec<String> {
    h.app.prompt().expect("prompt").candidates.clone()
}

fn root(h: &mut H) -> PathBuf {
    h.dir().root.clone()
}

#[tokio::test]
async fn the_picker_lists_visited_directories_best_first_without_the_current_one() {
    let fs = fs_with(&["/d1", "/d2", "/d3"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/d1", "/d2", "/d1", "/d3"]).await; // d1 twice, d2 once; now in d3
    h.press("J");
    assert_eq!(candidates(&h), ["  /d1", "  /d2"]);
}

#[tokio::test]
async fn choosing_a_directory_opens_it() {
    let fs = fs_with(&["/d1", "/d2"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/d1", "/d2"]).await;
    h.press("J");
    h.answer("  /d1");
    h.settle().await;
    assert_eq!(root(&mut h), PathBuf::from("/d1"));
}

#[tokio::test]
async fn typing_narrows_by_name_or_path_and_c_n_picks() {
    let fs = fs_with(&["/work/alpha", "/work/beta", "/other"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/work/alpha", "/work/beta", "/other"]).await;
    h.press("J");
    h.typ("alp");
    assert_eq!(candidates(&h), ["  /work/alpha"]);
    h.press("C-a C-k");
    h.typ("work");
    assert_eq!(candidates(&h).len(), 2);
    h.press("C-n");
    h.press("RET");
    h.settle().await;
    assert_eq!(root(&mut h), PathBuf::from("/work/beta"));
}

#[tokio::test]
async fn bookmarks_come_first_are_named_after_the_directory_and_open_it() {
    let fs = fs_with(&["/d1", "/d2", "/d3"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/d1"]).await;
    h.press("C-x r m");
    assert_eq!(h.app.prompt().unwrap().input, "d1", "defaults to the directory name");
    h.press("RET");
    assert_eq!(h.echo(), "Bookmarked d1 → /d1");
    visit(&mut h, &["/d2", "/d2", "/d3"]).await;
    h.press("J");
    assert_eq!(candidates(&h), ["★ d1  /d1", "  /d2"], "a bookmarked directory is listed once");
    h.press("RET");
    h.settle().await;
    assert_eq!(root(&mut h), PathBuf::from("/d1"));
}

#[tokio::test]
async fn setting_an_existing_name_updates_it_and_empty_names_are_refused() {
    let fs = fs_with(&["/d1", "/d2"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/d1"]).await;
    h.press("C-x r m");
    h.answer("proj");
    visit(&mut h, &["/d2"]).await;
    h.press("C-x r m");
    h.answer("proj");
    assert_eq!(h.echo(), "Bookmark proj updated → /d2");
    h.press("C-x r m");
    h.answer("   ");
    assert!(h.echo().contains("name"), "{}", h.echo());
    h.press("J");
    assert_eq!(candidates(&h)[0], "★ proj  /d2");
    assert_eq!(candidates(&h).iter().filter(|c| c.starts_with('★')).count(), 1);
}

#[tokio::test]
async fn bookmarks_can_be_deleted_from_a_picker() {
    let fs = fs_with(&["/d1", "/d2"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/d1"]).await;
    h.press("C-x r m");
    h.answer("one");
    h.press("C-x r d");
    assert_eq!(candidates(&h), ["one"]);
    h.answer("one");
    assert_eq!(h.echo(), "Deleted bookmark one");
    h.press("C-x r d");
    assert!(h.app.prompt().is_none(), "nothing to delete");
    assert_eq!(h.echo(), "No bookmarks");
}

#[tokio::test]
async fn a_missing_directory_is_reported_and_forgotten() {
    let fs = fs_with(&["/here"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/gone", "/here"]).await; // /gone is visited but does not exist
    h.press("J");
    assert_eq!(candidates(&h), ["  /gone"]);
    h.press("RET");
    h.settle().await;
    assert!(h.echo().contains("no longer exists"), "{}", h.echo());
    assert_eq!(root(&mut h), PathBuf::from("/here"), "stays where it was");
    h.press("J");
    assert!(h.app.prompt().is_none() || candidates(&h).is_empty(), "forgotten");
}

#[tokio::test]
async fn going_up_with_caret_counts_as_a_visit() {
    let fs = fs_with(&["/a/b", "/z"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/a/b"]).await;
    h.press("^");
    h.settle().await;
    visit(&mut h, &["/z"]).await;
    h.press("J");
    let c = candidates(&h);
    assert!(c.contains(&"  /a".to_string()) && c.contains(&"  /a/b".to_string()), "{c:?}");
}

#[tokio::test]
async fn available_from_m_x() {
    let fs = fs_with(&["/d1", "/d2"]);
    let mut h = harness(&fs);
    visit(&mut h, &["/d1", "/d2"]).await;
    h.press("M-x");
    h.answer("jump-to-directory");
    assert_eq!(candidates(&h), ["  /d1"]);
}

#[tokio::test]
async fn bookmarks_survive_a_restart_and_visits_follow_persist_history() {
    let state = tempfile::tempdir().unwrap();
    for persist in [true, false] {
        let fs = fs_with(&["/d1", "/d2"]);
        let trash = Arc::new(MockTrash::with_fs(fs.clone()));
        let text = format!("version = 1\n[safety]\npersist_history = {persist}\n");
        let cfg = || Config::parse(&text, "t").0;
        let mut h = harness_state(fs.clone(), trash.clone(), cfg(), vec![], Some(state.path().to_path_buf()));
        visit(&mut h, &["/d1", "/d2"]).await;
        h.press("C-x r m");
        h.answer("two");
        drop(h);

        // A new session: open somewhere else and look at the picker.
        let mut h = harness_state(fs.clone(), trash, cfg(), vec![], Some(state.path().to_path_buf()));
        visit(&mut h, &["/d2"]).await; // already in /d2: only others are listed
        h.press("J");
        let c = candidates(&h);
        assert!(c.contains(&"★ two  /d2".to_string()), "persist={persist}: {c:?}");
        assert_eq!(c.contains(&"  /d1".to_string()), persist, "persist={persist}: {c:?}");
        std::fs::remove_file(state.path().join("jumps.json")).unwrap();
    }
}

#[tokio::test]
async fn turning_persist_history_off_ignores_saved_visits_but_keeps_bookmarks() {
    let state = tempfile::tempdir().unwrap();
    let fs = fs_with(&["/d1", "/d2"]);
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    let session = |persist: bool| {
        let text = format!("version = 1\n[safety]\npersist_history = {persist}\n");
        harness_state(fs.clone(), trash.clone(), Config::parse(&text, "t").0, vec![], Some(state.path().to_path_buf()))
    };
    // First session saves visits and a bookmark.
    let mut h = session(true);
    visit(&mut h, &["/d1", "/d2"]).await;
    h.press("C-x r m");
    h.answer("two");
    drop(h);
    // The file really holds the visit...
    assert!(std::fs::read_to_string(state.path().join("jumps.json")).unwrap().contains("/d1"));
    // ...but a session with persistence off does not read it back.
    let mut h = session(false);
    visit(&mut h, &["/d2"]).await;
    h.press("J");
    let c = candidates(&h);
    assert!(c.contains(&"★ two  /d2".to_string()), "{c:?}");
    assert!(!c.contains(&"  /d1".to_string()), "{c:?}");
}

// dired-2fs
fn root_of_window(h: &H, win: u64) -> PathBuf {
    let buf = h.app.window(win).unwrap().buffer;
    h.app.buffer(buf).unwrap().dir().expect("a directory buffer").root.clone()
}

#[tokio::test]
async fn jumping_to_the_other_window_splits_when_there_is_only_one() {
    let fs = fs_with(&["/d1", "/d2"]);
    let mut h = harness(&fs);
    h.app.set_window_area(100, 30);
    visit(&mut h, &["/d1", "/d2"]).await;
    let first = h.app.selected_window();
    assert_eq!(h.app.windows().len(), 1);
    h.press("C-x 4 J");
    assert_eq!(candidates(&h), ["  /d1"]);
    h.press("RET");
    h.settle().await;
    assert_eq!(h.app.windows().len(), 2);
    assert_ne!(h.app.selected_window(), first, "the new window is selected");
    assert_eq!(root(&mut h), PathBuf::from("/d1"));
    assert_eq!(root_of_window(&h, first), PathBuf::from("/d2"), "the original window is untouched");
}

#[tokio::test]
async fn an_existing_other_window_is_reused() {
    let fs = fs_with(&["/d1", "/d2", "/d3"]);
    let mut h = harness(&fs);
    h.app.set_window_area(100, 30);
    visit(&mut h, &["/d1", "/d2"]).await;
    h.press("C-x 3");
    let before = h.app.windows().len();
    visit(&mut h, &["/d3"]).await; // the selected window now shows /d3
    let sel = h.app.selected_window();
    h.press("C-x 4 J");
    h.answer("  /d1");
    h.settle().await;
    assert_eq!(h.app.windows().len(), before, "no further split");
    assert_ne!(h.app.selected_window(), sel);
    assert_eq!(root(&mut h), PathBuf::from("/d1"));
    assert_eq!(root_of_window(&h, sel), PathBuf::from("/d3"));
}

#[tokio::test]
async fn a_missing_target_neither_splits_nor_moves() {
    let fs = fs_with(&["/here"]);
    let mut h = harness(&fs);
    h.app.set_window_area(100, 30);
    visit(&mut h, &["/gone", "/here"]).await;
    h.press("C-x 4 J");
    h.press("RET");
    h.settle().await;
    assert!(h.echo().contains("no longer exists"), "{}", h.echo());
    assert_eq!(h.app.windows().len(), 1);
    assert_eq!(root(&mut h), PathBuf::from("/here"));
}

#[tokio::test]
async fn the_command_is_in_m_x_and_plain_j_still_uses_the_same_window() {
    let fs = fs_with(&["/d1", "/d2"]);
    let mut h = harness(&fs);
    h.app.set_window_area(100, 30);
    visit(&mut h, &["/d1", "/d2"]).await;
    h.press("M-x");
    h.answer("jump-to-directory-other-window");
    assert_eq!(candidates(&h), ["  /d1"]);
    h.press("C-g");
    h.press("J");
    h.press("RET");
    h.settle().await;
    assert_eq!(h.app.windows().len(), 1, "J does not split");
    assert_eq!(root(&mut h), PathBuf::from("/d1"));
}
