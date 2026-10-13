// dired-tlu
//! `% M` / `% D` / `% U`: mark, flag or unmark files by condition.

mod common;

use std::time::{Duration, UNIX_EPOCH};

use common::*;
use dired_domain::EntryState;

/// MockFs gives every node an mtime of 1_700_000_000 + its inode (a small
/// number), so a clock at that second plus an offset makes ages predictable.
fn set_now(h: &H, secs_after_epoch_1_7e9: u64) {
    *h.clock.0.lock().unwrap() = UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs_after_epoch_1_7e9);
}

async fn ready() -> H {
    let (fs, mut h) = mock();
    fs.write("/w/big.log", &"x".repeat(5000));
    fs.write("/w/small.log", "x".repeat(10).as_str());
    fs.write("/w/notes.txt", &"x".repeat(5000));
    fs.mkdir_p("/w/sub");
    set_now(&h, 10 * 86_400 + 1_000); // everything is about 10 days old
    h.app.open_dir("/w");
    h.settle().await;
    h
}

#[tokio::test]
async fn percent_m_marks_matching_files() {
    let mut h = ready().await;
    h.press("% M");
    h.answer("size>1K ext=log");
    assert_eq!(h.state("big.log"), EntryState::Marked);
    assert_eq!(h.state("small.log"), EntryState::Unmarked);
    assert_eq!(h.state("notes.txt"), EntryState::Unmarked);
    assert_eq!(h.echo(), "1 matching file(s) marked");
}

#[tokio::test]
async fn percent_d_flags_for_deletion_by_age() {
    let mut h = ready().await;
    h.press("% D");
    h.answer("type=file age>5d");
    for n in ["big.log", "small.log", "notes.txt"] {
        assert_eq!(h.state(n), EntryState::DeleteFlagged, "{n}");
    }
    assert_eq!(h.state("sub"), EntryState::Unmarked, "type=file leaves the directory alone");
    assert_eq!(h.echo(), "3 matching file(s) flagged for deletion");
}

#[tokio::test]
async fn age_follows_the_clock() {
    let mut h = ready().await;
    h.press("% M");
    h.answer("type=file age<5d");
    assert_eq!(h.echo(), "0 matching file(s) marked", "10 days old is not younger than 5 days");
    set_now(&h, 1_000); // now they are about 15 minutes old
    h.press("% M");
    h.answer("type=file age<1h");
    assert_eq!(h.echo(), "3 matching file(s) marked");
}

#[tokio::test]
async fn percent_u_unmarks_matching_files() {
    let mut h = ready().await;
    h.press("t"); // toggle: everything marked
    h.press("% U");
    h.answer("ext=txt");
    assert_eq!(h.state("notes.txt"), EntryState::Unmarked);
    assert_eq!(h.state("big.log"), EntryState::Marked);
    assert_eq!(h.echo(), "1 matching file(s) unmarked");
}

#[tokio::test]
async fn a_bad_term_changes_nothing_and_says_which() {
    let mut h = ready().await;
    h.press("% M");
    h.answer("ext=log size>10Q");
    assert!(h.echo().contains("bad term 'size>10Q'"), "{}", h.echo());
    assert_eq!(h.state("big.log"), EntryState::Unmarked, "nothing marked on a parse error");
}

#[tokio::test]
async fn the_parent_row_is_never_marked() {
    let mut h = ready().await;
    h.press("% M");
    h.answer("name~.");
    assert_eq!(h.echo(), "4 matching file(s) marked", "big.log small.log notes.txt sub, not ..");
    assert_eq!(h.dir().state(std::path::Path::new("..")), EntryState::Unmarked);
}

#[tokio::test]
async fn available_from_m_x_with_its_own_history() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("dired-mark-files-where");
    h.answer("ext=txt");
    assert_eq!(h.state("notes.txt"), EntryState::Marked);
    h.press("% M");
    h.press("<up>");
    assert_eq!(h.app.prompt().unwrap().input, "ext=txt", "predicate prompts keep their own history");
}
