// dired-2hn
//! `load-theme` takes a theme name with completion; `cycle-theme` still cycles.

mod common;

use common::*;
use dired_app::registry;
use dired_config::THEMES;

async fn ready() -> H {
    let (fs, mut h) = mock();
    fs.write("/w/a", "");
    h.app.open_dir("/w");
    h.settle().await;
    h
}

fn theme(h: &H) -> Option<String> {
    h.app.config.theme.name.clone()
}

#[test]
fn load_theme_is_its_own_command_not_an_alias() {
    assert_eq!(registry::lookup("load-theme").unwrap().name, "load-theme");
    assert_eq!(registry::lookup("cycle-theme").unwrap().name, "cycle-theme");
}

#[tokio::test]
async fn prompts_for_a_theme_and_lists_the_builtins() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("load-theme");
    let p = h.app.prompt().expect("load-theme must prompt for an argument");
    assert!(p.label.to_lowercase().contains("theme"), "{}", p.label);
    assert_eq!(p.candidates, THEMES.iter().map(|s| s.to_string()).collect::<Vec<_>>());
}

#[tokio::test]
async fn typing_a_name_loads_that_theme() {
    let mut h = ready().await;
    let gen = h.app.config_generation();
    h.press("M-x");
    h.answer("load-theme");
    h.answer("nord");
    assert_eq!(theme(&h).as_deref(), Some("nord"));
    assert_eq!(h.echo(), "Theme: nord");
    assert!(h.app.config_generation() > gen, "the UI re-reads the theme when the generation changes");
}

#[tokio::test]
async fn tab_completes_the_theme_name() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("load-theme");
    h.typ("gru");
    h.press("TAB");
    assert_eq!(h.app.prompt().unwrap().input, "gruvbox");
    h.press("RET");
    assert_eq!(theme(&h).as_deref(), Some("gruvbox"));
}

#[tokio::test]
async fn an_unknown_name_is_an_error_and_changes_nothing() {
    let mut h = ready().await;
    let before = theme(&h);
    h.press("M-x");
    h.answer("load-theme");
    h.typ("zzzz-nope");
    h.press("RET");
    assert_eq!(theme(&h), before);
    assert!(h.echo().contains("No such theme"), "{}", h.echo());
}

#[tokio::test]
async fn the_prompt_starts_on_the_current_theme_and_arrows_pick_another() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("load-theme");
    h.answer("dracula");
    h.press("M-x");
    h.answer("load-theme");
    let p = h.app.prompt().unwrap();
    assert_eq!(p.candidates[p.selected], "dracula", "starts on the active theme");
    h.press("C-p");
    h.press("RET");
    assert_eq!(theme(&h).as_deref(), Some("solarized-dark"), "the highlighted neighbour is loaded");
}

#[tokio::test]
async fn cycle_theme_still_cycles() {
    let mut h = ready().await;
    h.press("M-x");
    h.answer("cycle-theme");
    assert_eq!(theme(&h).as_deref(), Some("high-contrast"));
}
