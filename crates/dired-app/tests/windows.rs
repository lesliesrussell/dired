// dired-ta2
//! Emacs-style windows: splitting, selection, per-window point, deletion,
//! C-x 4 commands, dwim copy target, and popup previews.

mod common;

use common::*;
use dired_app::windows::Rect;
use dired_domain::EntryState;

fn two_dirs() -> (std::sync::Arc<dired_testkit::MockFs>, H) {
    let (fs, h) = mock();
    for n in ["a", "b", "c", "d"] {
        fs.write(format!("/left/{n}"), n);
    }
    fs.mkdir_p("/right/sub");
    fs.write("/right/x", "x");
    (fs, h)
}

#[tokio::test]
async fn split_select_and_independent_point() {
    let (_fs, mut h) = two_dirs();
    h.app.open_dir("/left");
    h.settle().await;
    h.goto("a");
    h.press("C-x 2");
    assert_eq!(h.app.windows().len(), 2);
    let w1 = h.app.selected_window();
    // Both windows show the same buffer; selection stays in the original.
    assert!(h.app.windows().iter().all(|w| w.buffer == h.app.active_id()));
    h.goto("c");
    h.press("C-x o");
    assert_ne!(h.app.selected_window(), w1);
    assert_eq!(h.point_name(), "a", "the new window kept its own point");
    h.press("C-n");
    assert_eq!(h.point_name(), "b");
    h.press("C-x o");
    assert_eq!(h.point_name(), "c", "back in the first window at its own point");
    // Marks live in the buffer, so both windows see them.
    h.press("m");
    h.press("C-x o");
    assert_eq!(h.state("c"), EntryState::Marked);
    // Geometry: stacked halves of the default 80x23 area.
    let rects: Vec<Rect> = h.app.window_rects().into_iter().map(|(_, r, _)| r).collect();
    assert_eq!(rects, vec![Rect { x: 0, y: 0, w: 80, h: 12 }, Rect { x: 0, y: 12, w: 80, h: 11 }]);
}

#[tokio::test]
async fn delete_windows() {
    let (_fs, mut h) = two_dirs();
    h.app.open_dir("/left");
    h.settle().await;
    h.press("C-x 0");
    assert!(h.echo().contains("sole window"));
    h.press("C-x 3");
    h.press("C-x 2");
    assert_eq!(h.app.windows().len(), 3);
    h.press("C-x 0");
    assert_eq!(h.app.windows().len(), 2);
    assert_eq!(h.app.buffers().len(), 1, "deleting a window keeps its buffer");
    h.press("C-x 3 C-x 1");
    assert_eq!(h.app.windows().len(), 1);
    // Too small to split.
    h.app.set_window_area(30, 6);
    h.press("C-x 2");
    assert!(h.echo().contains("too small"));
    assert_eq!(h.app.windows().len(), 1);
}

#[tokio::test]
async fn other_window_commands_and_dwim_copy_target() {
    let (fs, mut h) = two_dirs();
    h.app.set_window_area(120, 30);
    h.app.open_dir("/left");
    h.settle().await;
    // C-x 4 d splits (side by side on a wide screen) and opens there.
    h.press("C-x 4 d");
    h.answer("/right");
    h.settle().await;
    assert_eq!(h.app.windows().len(), 2);
    assert_eq!(h.dir().root, p("/right"));
    let rects: Vec<Rect> = h.app.window_rects().into_iter().map(|(_, r, _)| r).collect();
    assert_eq!(rects[0].w + 1 + rects[1].w, 120, "side by side with a separator");
    // Copy from the left window: the destination defaults to the right one.
    h.press("C-x o");
    assert_eq!(h.dir().root, p("/left"));
    h.goto("a");
    h.press("m");
    h.goto("b");
    h.press("m");
    h.press("C");
    assert_eq!(h.app.prompt().unwrap().input, "/right/");
    h.press("RET");
    h.settle().await;
    // The preview pops up below the source window; both directories stay visible.
    assert_eq!(h.app.windows().len(), 3);
    h.press("x");
    h.settle().await;
    assert_eq!(fs.read("/right/a").as_deref(), Some("a"));
    h.press("q");
    assert_eq!(h.app.windows().len(), 2, "the popup window closes with its preview");
    assert_eq!(h.dir().root, p("/left"));
    // The other window refreshed and shows the copies.
    h.press("C-x o");
    h.settle().await;
    assert!(h.names().contains(&"a".to_string()));
    // `o` visits a directory in the other window.
    h.goto("sub");
    h.press("o");
    h.settle().await;
    assert_eq!(h.dir().root, p("/right/sub"), "opened in, and selected, the other window");
    h.press("C-x o");
    assert_eq!(h.dir().root, p("/right"), "the original window is unchanged");
    // With dwim enabled the other window (/right/sub) is the default...
    h.goto("x");
    h.press("C");
    assert_eq!(h.app.prompt().unwrap().input, "/right/sub/");
    h.press("C-g");
    // ...and with it disabled the prompt starts at the current directory.
    h.app.config.display.dwim_target = false;
    h.press("C");
    assert_eq!(h.app.prompt().unwrap().input, "/right/");
    h.press("C-g");
}

#[tokio::test]
async fn killing_a_buffer_shown_in_two_windows() {
    let (_fs, mut h) = two_dirs();
    h.app.open_dir("/right");
    h.settle().await;
    h.app.open_dir("/left");
    h.settle().await;
    h.press("C-x 2");
    h.press("C-x k RET");
    assert_eq!(h.app.buffers().len(), 1);
    assert!(h.app.windows().iter().all(|w| w.buffer == h.app.active_id()));
    assert_eq!(h.dir().root, p("/right"));
    // C-x 4 b shows a buffer in the other window.
    h.press("?");
    h.press("C-x 4 b");
    h.answer("right/");
    assert_eq!(h.app.active().name, "right/");
    h.press("C-x o");
    assert_eq!(h.app.active().name, "*Help*");
}

#[tokio::test]
async fn review_regressions() {
    // Closing a buffer never leaves a dead buffer in a window (was a panic).
    let (_fs, mut h) = two_dirs();
    h.app.open_dir("/right");
    h.settle().await;
    h.app.open_dir("/left");
    h.settle().await;
    h.goto("a");
    h.press("m C");
    h.answer("/right/");
    h.settle().await;
    h.press("C-x 2 q");
    h.press("C-x k");
    h.answer("left/");
    for w in h.app.windows() {
        assert!(h.app.buffer(w.buffer).is_some(), "window shows a closed buffer");
    }
    let _ = h.app.active();

    // Output buffers keep following after leaving and re-entering the window.
    let (_fs, mut h) = two_dirs();
    h.runner.set_output((0..30).map(|i| format!("line {i}")).collect());
    h.app.open_dir("/left");
    h.settle().await;
    h.press("C-x 2");
    h.goto("a");
    h.press("&");
    h.answer("cat");
    h.press("C-x o");
    h.settle().await;
    h.press("C-x o");
    match &h.app.active().kind {
        dired_app::buffers::BufferKind::Text(t) => {
            assert!(t.follow, "still following");
            assert_eq!(t.point, t.lines.len() - 1);
        }
        _ => panic!("expected the process buffer"),
    }

    // C-x 4 f on a file opens it without splitting; on a directory it uses the other window.
    let (_fs, mut h) = two_dirs();
    h.app.open_dir("/left");
    h.settle().await;
    h.press("C-x 4 f");
    h.answer("/left/a");
    h.settle().await;
    assert_eq!(h.app.windows().len(), 1);
    assert_eq!(h.opener.opened.lock().unwrap().len(), 1);
    h.press("C-x 4 f C-e"); // prompt is pre-filled with /left/
    h.typ("/right"); // "/left//right": a second slash starts a new path
    h.press("RET");
    h.settle().await;
    assert_eq!(h.app.windows().len(), 2);
    assert_eq!(h.dir().root, p("/right"));
}

#[test]
fn typing_a_new_root_after_a_prefilled_path_starts_over() {
    use dired_app::expand_path;
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
    let base = p("/cwd");
    assert_eq!(expand_path("/left/~/backup", &base), home.join("backup"));
    assert_eq!(expand_path("/left/~", &base), home);
    assert_eq!(expand_path("/left//tmp/x", &base), p("/tmp/x"));
    assert_eq!(expand_path("/left/a~b", &base), p("/left/a~b"), "a ~ inside a name is literal");
    assert_eq!(expand_path("sub", &base), p("/cwd/sub"));
}
