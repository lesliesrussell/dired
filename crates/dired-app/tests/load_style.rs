// dired-5xb
//! `load-style` takes a style name with completion; `cycle-style` still cycles.

mod common;

use common::*;
use dired_app::registry;
use dired_config::{Config, STYLES};

async fn ready() -> H {
    let (fs, mut h) = mock();
    fs.write("/w/a", "");
    h.app.open_dir("/w");
    h.settle().await;
    h
}

fn style(h: &H) -> String {
    h.app.config.display.style.clone()
}

#[test]
fn load_style_is_a_registered_command() {
    assert_eq!(registry::lookup("load-style").unwrap().name, "load-style");
    assert_eq!(registry::lookup("cycle-style").unwrap().name, "cycle-style");
}

#[tokio::test]
async fn prompts_for_a_style_and_lists_the_presets() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("load-style");
    let p = h.app.prompt().expect("load-style must prompt for an argument");
    assert!(p.label.to_lowercase().contains("style"), "{}", p.label);
    assert_eq!(p.candidates, STYLES.iter().map(|s| s.to_string()).collect::<Vec<_>>());
}

#[tokio::test]
async fn typing_a_name_loads_that_style() {
    let mut h = ready().await;
    let gen = h.app.config_generation();
    h.press("M-x");
    h.answer("load-style");
    h.answer("rich");
    assert_eq!(style(&h), "rich");
    assert_eq!(h.echo(), "Style: rich");
    assert!(h.app.config_generation() > gen, "the UI re-reads the style when the generation changes");
}

#[tokio::test]
async fn tab_completes_the_style_name() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("load-style");
    h.typ("min");
    h.press("TAB");
    assert_eq!(h.app.prompt().unwrap().input, "minimal");
    h.press("RET");
    assert_eq!(style(&h), "minimal");
}

#[tokio::test]
async fn an_unknown_name_is_an_error_and_changes_nothing() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("load-style");
    h.typ("glam");
    h.press("RET");
    assert_eq!(style(&h), "clean");
    assert!(h.echo().contains("No such style"), "{}", h.echo());
}

#[tokio::test]
async fn the_prompt_starts_on_the_current_style_and_arrows_pick_another() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("load-style");
    let p = h.app.prompt().unwrap();
    assert_eq!(p.candidates[p.selected], "clean", "starts on the active style");
    h.press("C-p");
    h.press("RET");
    assert_eq!(style(&h), "rich", "the highlighted neighbour is loaded");
}

#[tokio::test]
async fn overrides_are_mentioned_like_cycle_style_does() {
    let (c, d) = Config::parse("version = 1\n[display.ui]\nborders = true\n", "t");
    assert!(d.is_empty());
    let fs = std::sync::Arc::new(dired_testkit::MockFs::new());
    fs.write("/w/a", "");
    let trash = std::sync::Arc::new(dired_testkit::MockTrash::with_fs(fs.clone()));
    let mut h = harness_with(fs, trash, c, vec![]);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("M-x");
    h.answer("load-style");
    h.answer("minimal");
    assert_eq!(h.echo(), "Style: minimal (with overrides)");
}

#[tokio::test]
async fn cycle_style_still_cycles() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("cycle-style");
    assert_eq!(style(&h), "minimal");
}
