// dired-6ul
//! "Git UI here" (`C-c G`): suspend dired and run lazygit (or the configured
//! program) in the repository of the current directory.

mod common;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use common::*;
use dired_app::git::GitStatus;
use dired_app::Effect;
use dired_config::Config;
use dired_testkit::{MockFs, MockTrash};

fn harness_cfg(config: Config, in_repo: bool) -> H {
    let fs = Arc::new(MockFs::new());
    fs.mkdir_p("/w/sub");
    fs.write("/w/a.txt", "A");
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    let h = harness_with(fs, trash, config, vec![]);
    if in_repo {
        *h.git.answer.lock().unwrap() = Some(GitStatus::parse(b"## main\0 M a.txt\0"));
    }
    h
}

fn config(toml: &str) -> Config {
    let (c, d) = Config::parse(toml, "t");
    assert!(d.is_empty(), "{d:?}");
    c
}

#[tokio::test]
async fn runs_lazygit_in_the_buffers_directory_by_default() {
    let mut h = harness_cfg(Config::defaults(), true);
    h.app.open_dir("/w/sub");
    h.settle().await;
    h.press("C-c G");
    let effects = h.app.take_effects();
    let [Effect::Foreground(spec)] = effects.as_slice() else { panic!("{effects:?}") };
    assert_eq!(spec.program, OsString::from("lazygit"));
    assert!(spec.args.is_empty());
    assert_eq!(spec.cwd, PathBuf::from("/w/sub"));
}

#[tokio::test]
async fn the_program_and_its_arguments_are_configurable() {
    let mut h = harness_cfg(config("version = 1\n[git]\nui = [\"tig\", \"status\"]\n"), true);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-c G");
    let effects = h.app.take_effects();
    let [Effect::Foreground(spec)] = effects.as_slice() else { panic!("{effects:?}") };
    assert_eq!(spec.program, OsString::from("tig"));
    assert_eq!(spec.args, [OsString::from("status")]);
}

#[tokio::test]
async fn outside_a_repository_it_says_so_and_runs_nothing() {
    let mut h = harness_cfg(Config::defaults(), false);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-c G");
    assert!(h.app.take_effects().is_empty());
    assert_eq!(h.echo(), "Not in a git repository");
}

#[tokio::test]
async fn with_git_state_disabled_the_program_still_runs() {
    // Nothing was asked of git, so "no status" does not mean "no repository".
    let mut h = harness_cfg(config("version = 1\n[git]\nenabled = false\n"), false);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-c G");
    assert!(matches!(h.app.take_effects().as_slice(), [Effect::Foreground(_)]));
}

#[tokio::test]
async fn is_available_from_m_x_and_needs_a_directory_buffer() {
    let mut h = harness_cfg(Config::defaults(), true);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("M-x");
    h.answer("dired-git-ui");
    assert_eq!(h.app.take_effects().len(), 1);
    h.press("?");
    h.press("M-x");
    h.answer("dired-git-ui");
    assert!(h.app.take_effects().is_empty());
    assert_eq!(h.echo(), "dired-git-ui needs a directory buffer");
}

#[tokio::test]
async fn the_example_config_does_not_shadow_the_key() {
    let (config, diags) = Config::parse(include_str!("../../../examples/config.toml"), "example");
    let fs = Arc::new(MockFs::new());
    fs.mkdir_p("/w");
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    let mut h = harness_with(fs, trash, config, diags);
    *h.git.answer.lock().unwrap() = Some(GitStatus::parse(b"## main\0"));
    h.app.open_dir("/w");
    h.settle().await;
    h.press("C-c G");
    assert!(matches!(h.app.take_effects().as_slice(), [Effect::Foreground(_)]));
}
