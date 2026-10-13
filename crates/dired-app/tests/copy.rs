// dired-vhh
//! `w` / `W`: copy file names / full paths to the clipboard.

mod common;

use common::*;
use dired_app::Effect;

async fn ready() -> H {
    let (fs, mut h) = mock();
    for n in ["alpha.txt", "beta.txt", "gamma.txt"] {
        fs.write(format!("/w/{n}"), "");
    }
    fs.mkdir_p("/w/sub");
    fs.write("/w/sub/inner.txt", "");
    h.app.open_dir("/w");
    h.settle().await;
    h
}

fn clipboard(h: &mut H) -> Vec<String> {
    h.app
        .take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Clipboard(t) => Some(t),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn w_copies_the_name_at_point() {
    let mut h = ready().await;
    h.goto("beta.txt");
    h.press("w");
    assert_eq!(clipboard(&mut h), ["beta.txt"]);
    assert_eq!(h.echo(), "Copied beta.txt");
}

#[tokio::test]
async fn capital_w_copies_the_full_path() {
    let mut h = ready().await;
    h.goto("beta.txt");
    h.press("W");
    assert_eq!(clipboard(&mut h), ["/w/beta.txt"]);
    assert_eq!(h.echo(), "Copied /w/beta.txt");
}

#[tokio::test]
async fn marked_entries_are_copied_one_per_line_and_win_over_point() {
    let mut h = ready().await;
    h.goto("alpha.txt");
    h.press("m");
    h.goto("gamma.txt");
    h.press("m");
    h.goto("beta.txt"); // point is elsewhere
    h.press("w");
    assert_eq!(clipboard(&mut h), ["alpha.txt\ngamma.txt"]);
    assert_eq!(h.echo(), "Copied 2 names");
    h.press("W");
    assert_eq!(clipboard(&mut h), ["/w/alpha.txt\n/w/gamma.txt"]);
    assert_eq!(h.echo(), "Copied 2 paths");
}

#[tokio::test]
async fn an_inserted_subdirectory_entry_copies_its_file_name_and_real_path() {
    let mut h = ready().await;
    h.goto("sub");
    h.press("i");
    h.settle().await;
    h.goto("inner.txt");
    h.press("w");
    assert_eq!(clipboard(&mut h), ["inner.txt"]);
    h.press("W");
    assert_eq!(clipboard(&mut h), ["/w/sub/inner.txt"]);
}

#[tokio::test]
async fn the_parent_row_and_empty_listings_copy_nothing() {
    let mut h = ready().await;
    h.goto("..");
    h.press("w");
    assert!(clipboard(&mut h).is_empty());
    assert_eq!(h.echo(), "No file on this line");
}

#[tokio::test]
async fn the_commands_are_available_from_m_x() {
    let mut h = ready().await;
    h.goto("alpha.txt");
    h.press("M-x");
    h.answer("dired-copy-path");
    assert_eq!(clipboard(&mut h), ["/w/alpha.txt"]);
}
