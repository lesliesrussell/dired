// dired-t1z
//! Selecting among completion candidates with C-n / C-p / Down / Up.

mod common;

use std::sync::Arc;

use common::*;
use dired_config::Config;
use dired_testkit::{MockFs, MockTrash};

fn selected(h: &H) -> usize {
    h.app.prompt().expect("prompt").selected
}

async fn mx() -> H {
    let (fs, mut h) = mock();
    fs.write("/w/a", "");
    h.app.open_dir("/w");
    h.settle().await;
    h.press("M-x");
    h
}

#[tokio::test]
async fn ctrl_n_and_ctrl_p_move_the_selection_and_clamp() {
    let mut h = mx().await;
    assert_eq!(selected(&h), 0);
    h.press("C-n");
    h.press("C-n");
    assert_eq!(selected(&h), 2);
    h.press("C-p");
    assert_eq!(selected(&h), 1);
    h.press("C-p");
    h.press("C-p");
    assert_eq!(selected(&h), 0, "clamps at the top");
}

#[tokio::test]
async fn arrow_keys_do_the_same() {
    let mut h = mx().await;
    h.press("<down>");
    h.press("<down>");
    h.press("<down>");
    assert_eq!(selected(&h), 3);
    h.press("<up>");
    assert_eq!(selected(&h), 2);
}

#[tokio::test]
async fn the_selection_clamps_at_the_last_candidate_and_scrolls_past_six() {
    let mut h = mx().await;
    let n = h.app.prompt().unwrap().candidates.len();
    assert!(n > 8, "M-x lists every command");
    for _ in 0..8 {
        h.press("C-n");
    }
    assert_eq!(selected(&h), 8, "beyond the six visible lines");
    for _ in 0..n + 5 {
        h.press("C-n");
    }
    assert_eq!(selected(&h), n - 1, "clamps at the bottom");
}

#[tokio::test]
async fn typing_resets_the_selection_to_the_best_match() {
    let mut h = mx().await;
    h.press("C-n");
    h.press("C-n");
    h.typ("cyc");
    assert_eq!(selected(&h), 0);
}

#[tokio::test]
async fn history_stays_on_meta_p_and_meta_n() {
    let mut h = mx().await;
    h.answer("cycle-style");
    h.press("M-x");
    h.press("M-p");
    assert_eq!(h.app.prompt().unwrap().input, "cycle-style");
}

#[tokio::test]
async fn prompts_without_candidates_keep_up_down_history() {
    let (fs, mut h) = mock();
    fs.write("/w/a", "");
    h.app.open_dir("/w");
    h.settle().await;
    h.press("/");
    h.answer("abc");
    h.press("/");
    h.press("<up>");
    assert_eq!(h.app.prompt().unwrap().input, "abc");
}

#[tokio::test]
async fn an_explicit_pick_wins_over_an_exact_match() {
    let (c, d) = Config::parse(
        r#"
version = 1
[[open_with]]
name = "Open"
command = ["a", "{file}"]
[[open_with]]
name = "Open in Editor"
command = ["b", "{file}"]
"#,
        "t",
    );
    assert!(d.is_empty(), "{d:?}");
    let fs = Arc::new(MockFs::new());
    fs.write("/w/x.txt", "");
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    let mut h = harness_with(fs, trash, c, vec![]);
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("x.txt");
    h.press("O");
    h.typ("Open"); // exactly the first candidate's name
    let cands = h.app.prompt().unwrap().candidates.clone();
    assert_eq!(&cands[..2], ["Open", "Open in Editor"], "{cands:?}");
    h.press("C-n");
    h.press("RET");
    let got = h.opener.opened_with.lock().unwrap().clone();
    assert_eq!(got[0].0, "Open in Editor");

    // Without navigating, exact input still means exactly that.
    h.press("O");
    h.typ("Open");
    h.press("RET");
    let got = h.opener.opened_with.lock().unwrap().clone();
    assert_eq!(got[1].0, "Open");
}
