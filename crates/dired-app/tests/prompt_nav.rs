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

// dired-bmd
async fn path_prompt_after_tab() -> H {
    let (fs, mut h) = mock();
    fs.write("/w/alpha", "");
    fs.write("/w/alpine", "");
    fs.write("/w/beta", "");
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-x C-f");
    h.press("C-a C-k");
    h.typ("/w/al");
    h.press("TAB");
    h.settle().await;
    h
}

#[tokio::test]
async fn path_candidates_after_tab_can_be_selected_line_by_line() {
    let mut h = path_prompt_after_tab().await;
    assert_eq!(h.app.prompt().unwrap().candidates, ["/w/alpha", "/w/alpine"]);
    assert_eq!(selected(&h), 0);
    h.press("C-n");
    assert_eq!(selected(&h), 1);
    h.press("C-n");
    assert_eq!(selected(&h), 1, "clamps at the bottom");
    h.press("C-p");
    assert_eq!(selected(&h), 0);
    h.press("<down>");
    assert_eq!(selected(&h), 1);
    h.press("<up>");
    assert_eq!(selected(&h), 0);
}

#[tokio::test]
async fn ret_accepts_the_picked_path() {
    let mut h = path_prompt_after_tab().await;
    h.press("C-n");
    h.press("RET");
    h.settle().await; // find-file stats the path in the background before visiting
    let opened = h.opener.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].path, std::path::PathBuf::from("/w/alpine"));
}

#[tokio::test]
async fn tab_inserts_the_picked_path() {
    let mut h = path_prompt_after_tab().await;
    h.press("C-n");
    h.press("TAB");
    h.settle().await;
    assert_eq!(h.app.prompt().unwrap().input, "/w/alpine");
}

#[tokio::test]
async fn editing_after_a_pick_drops_the_pick() {
    let mut h = path_prompt_after_tab().await;
    h.press("C-n");
    h.typ("x");
    assert!(h.app.prompt().unwrap().candidates.is_empty(), "stale candidates are cleared on edit");
    assert!(!h.app.prompt().unwrap().picked);
}

#[tokio::test]
async fn path_prompts_without_candidates_keep_up_down_history() {
    let (fs, mut h) = mock();
    fs.write("/w/alpha", "");
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-x C-f");
    h.press("C-a C-k");
    h.typ("/w/alpha");
    h.press("RET");
    h.press("C-x C-f");
    h.press("C-a C-k");
    h.press("<up>");
    assert_eq!(h.app.prompt().unwrap().input, "/w/alpha");
}
