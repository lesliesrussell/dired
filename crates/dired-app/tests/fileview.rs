// dired-6xv
//! `v`: show the file at point in a read-only view window; `C-M-v` scrolls it.

mod common;

use std::sync::Arc;

use common::*;
use dired_app::buffers::BufferKind;
use dired_testkit::{MockFs, MockTrash};

const VIEW: &str = "*View*";

fn fixture() -> (Arc<MockFs>, H) {
    let fs = Arc::new(MockFs::new());
    fs.write("/w/a.txt", "hello\nworld\n");
    fs.write("/w/b.txt", "second file\n");
    let long: String = (0..1000).map(|i| format!("line {i}\n")).collect();
    fs.write("/w/long.txt", &long);
    fs.write_bytes("/w/bin.dat", &[0, 1, 2, 3, 0x41, 0xff, 0xfe]);
    fs.write("/w/sub/inner.txt", "x");
    fs.mkdir_p("/w/sub/deeper");
    fs.symlink("/w/link", "/w/a.txt");
    fs.write("/w/secret.txt", "nope");
    fs.fail_on("/w/secret.txt", "denied");
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    let mut h = harness_with(fs.clone(), trash, dired_config::Config::defaults(), vec![]);
    h.app.set_window_area(100, 30);
    (fs, h)
}

async fn ready() -> H {
    let (_fs, mut h) = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h
}

/// The lines of the view buffer.
fn view_text(h: &H) -> Vec<String> {
    let id = h.app.buffers().iter().find(|b| b.name == VIEW).map(|b| b.id).expect("a *View* buffer");
    match &h.app.buffer(id).unwrap().kind {
        BufferKind::Text(t) => t.lines.iter().map(|l| l.text.clone()).collect(),
        _ => panic!("*View* is a text buffer"),
    }
}

fn view_window(h: &H) -> Option<u64> {
    let id = h.app.buffers().iter().find(|b| b.name == VIEW)?.id;
    h.app.windows().iter().find(|w| w.buffer == id).map(|w| w.id)
}

#[tokio::test]
async fn v_shows_the_file_in_a_second_window_and_leaves_point_alone() {
    let mut h = ready().await;
    h.goto("a.txt");
    let selected = h.app.selected_window();
    h.press("v");
    assert_eq!(h.app.windows().len(), 2);
    assert_eq!(h.app.selected_window(), selected, "the dired window stays selected");
    assert_eq!(h.point_name(), "a.txt");
    h.settle().await;
    let t = view_text(&h);
    assert_eq!(t[0], "/w/a.txt");
    assert!(t.contains(&"hello".to_string()) && t.contains(&"world".to_string()), "{t:?}");
    assert_ne!(view_window(&h), Some(selected));
}

#[tokio::test]
async fn pressing_v_again_refreshes_the_same_window_in_place() {
    let mut h = ready().await;
    h.goto("a.txt");
    h.press("v");
    h.settle().await;
    let win = view_window(&h);
    h.goto("b.txt");
    h.press("v");
    h.settle().await;
    assert_eq!(h.app.windows().len(), 2);
    assert_eq!(view_window(&h), win);
    let t = view_text(&h);
    assert_eq!(t[0], "/w/b.txt");
    assert!(t.contains(&"second file".to_string()));
}

#[tokio::test]
async fn an_existing_view_window_is_reused_when_there_are_several_windows() {
    let mut h = ready().await;
    h.goto("a.txt");
    h.press("v");
    h.settle().await;
    let win = view_window(&h);
    h.press("C-x 2"); // a third window
    assert_eq!(h.app.windows().len(), 3);
    h.goto("b.txt");
    h.press("v");
    h.settle().await;
    assert_eq!(h.app.windows().len(), 3, "no further split");
    assert_eq!(view_window(&h), win, "the window already showing the view is reused");
    let view_id = h.app.buffers().iter().find(|b| b.name == VIEW).unwrap().id;
    let showing = h.app.windows().iter().filter(|w| w.buffer == view_id).count();
    assert_eq!(showing, 1, "the view must not be duplicated into a second window");
    assert_eq!(view_text(&h)[0], "/w/b.txt");
}

#[tokio::test]
async fn only_the_latest_request_wins() {
    let mut h = ready().await;
    h.goto("a.txt");
    h.press("v");
    h.goto("b.txt");
    h.press("v"); // before the first read finished
    h.settle().await;
    assert_eq!(view_text(&h)[0], "/w/b.txt");
}

#[tokio::test]
async fn binary_files_show_a_hex_dump() {
    let mut h = ready().await;
    h.goto("bin.dat");
    h.press("v");
    h.settle().await;
    let t = view_text(&h);
    assert!(t.iter().any(|l| l.starts_with("Binary file")), "{t:?}");
    assert!(t.iter().any(|l| l.starts_with("00000000  00 01 02 03 41 ff fe")), "{t:?}");
}

#[tokio::test]
async fn long_files_are_cut_with_a_note() {
    let mut h = ready().await;
    h.goto("long.txt");
    h.press("v");
    h.settle().await;
    let t = view_text(&h);
    assert!(t.contains(&"line 0".to_string()) && t.contains(&"line 499".to_string()));
    assert!(!t.contains(&"line 500".to_string()), "cut at 500 lines");
    assert!(t.last().unwrap().starts_with("… truncated"), "{:?}", t.last());
}

#[tokio::test]
async fn directories_list_their_entries() {
    let mut h = ready().await;
    h.goto("sub");
    h.press("v");
    h.settle().await;
    let t = view_text(&h);
    assert!(t[1].contains("directory"), "{}", t[1]);
    assert!(t.contains(&"deeper/".to_string()) && t.contains(&"inner.txt".to_string()), "{t:?}");
}

#[tokio::test]
async fn symlinks_show_their_target_and_its_contents() {
    let mut h = ready().await;
    h.goto("link");
    h.press("v");
    h.settle().await;
    let t = view_text(&h);
    assert_eq!(t[0], "/w/link → /w/a.txt");
    assert!(t.contains(&"hello".to_string()), "{t:?}");
}

#[tokio::test]
async fn unreadable_files_say_why() {
    let mut h = ready().await;
    h.goto("secret.txt");
    h.press("v");
    h.settle().await;
    let t = view_text(&h);
    assert!(t.last().unwrap().starts_with("Cannot read:") && t.last().unwrap().contains("denied"), "{t:?}");
}

#[tokio::test]
async fn the_parent_row_is_refused_without_splitting() {
    let mut h = ready().await;
    h.goto("..");
    h.press("v");
    assert_eq!(h.app.windows().len(), 1);
    assert!(h.echo().contains("Nothing to view"), "{}", h.echo());
}

#[tokio::test]
async fn it_needs_a_directory_buffer_and_is_in_m_x() {
    let mut h = ready().await;
    h.goto("a.txt");
    h.press("M-x");
    h.answer("split-window-to-file-view");
    h.settle().await;
    assert_eq!(view_text(&h)[0], "/w/a.txt");
    // From the view buffer itself (not a directory) it refuses.
    h.press("C-x o");
    h.press("M-x");
    h.answer("split-window-to-file-view");
    assert!(h.echo().contains("directory"), "{}", h.echo());
}

fn view_top(h: &H) -> usize {
    let w = view_window(h).expect("view window");
    h.app.window(w).unwrap().view.top
}

#[tokio::test]
async fn ctrl_meta_v_scrolls_the_view_window_from_the_dired_window() {
    let mut h = ready().await;
    h.goto("long.txt");
    h.press("v");
    h.settle().await;
    assert_eq!(view_top(&h), 0);
    h.press("C-M-v");
    let first = view_top(&h);
    assert!(first > 5, "scrolled a page: {first}");
    h.press("C-M-v");
    assert!(view_top(&h) > first);
    assert_eq!(h.app.selected_window(), h.app.windows().iter().find(|w| Some(w.id) != view_window(&h)).unwrap().id);
    h.press("C-M-b");
    assert_eq!(view_top(&h), first, "scrolling back undoes a page");
    h.press("C-M-b");
    h.press("C-M-b");
    assert_eq!(view_top(&h), 0);
    assert_eq!(h.echo(), "Beginning of buffer");
}

#[tokio::test]
async fn scrolling_stops_at_the_end_and_needs_another_window() {
    let mut h = ready().await;
    h.press("C-M-v");
    assert_eq!(h.echo(), "No other window");
    h.goto("a.txt"); // short file: nothing to scroll
    h.press("v");
    h.settle().await;
    h.press("C-M-v");
    assert_eq!(h.echo(), "End of buffer");
    assert_eq!(view_top(&h), 0);
}

#[tokio::test]
async fn a_late_result_for_an_older_request_is_ignored() {
    let mut h = ready().await;
    h.goto("a.txt");
    h.press("v");
    h.settle().await;
    let before = view_text(&h);
    // Generation 0 is older than any real request.
    h.app.handle_event(dired_app::AppEvent::FileViewReady {
        gen: 0,
        lines: vec![dired_app::buffers::TextLine::normal("stale")],
    });
    assert_eq!(view_text(&h), before);
}

/// Safety: opening a FIFO would block forever, so the view must never try.
#[tokio::test]
async fn a_named_pipe_is_never_opened() {
    let (dir, mut h) = local();
    let fifo = dir.path().join("pipe");
    let ok = std::process::Command::new("mkfifo").arg(&fifo).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        eprintln!("mkfifo unavailable; skipping");
        return;
    }
    h.app.set_window_area(100, 30);
    h.app.open_dir(dir.path());
    h.settle().await;
    h.goto("pipe");
    h.press("v");
    h.settle().await; // would time out (20s) if the pipe were opened
    let t = view_text(&h);
    assert!(t.iter().any(|l| l.contains("Not a regular file")), "{t:?}");
}
