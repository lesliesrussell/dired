// dired-7qs
//! Application layer: buffers, commands, keymaps, minibuffer, and task
//! orchestration. The UI dispatches keys here; filesystem work runs in
//! background tasks through the adapter traits.

mod app;
pub mod buffers;
mod commands;
pub mod dirbuf;
pub mod fileview;
pub mod git;
pub mod jumps;
pub mod keymap;
pub mod minibuffer;
pub mod registry;
pub mod services;
pub mod tasks;
// dired-ta2
mod window_ops;
pub mod windows;

pub use app::{expand_path, normalize, App};
pub use commands::parse_rg_json;
pub use services::{builtin_open_with, terminal_argv, Effect, GitProbe, OpenOutcome, OpenRequest, Opener, Services, SystemGit, SystemOpener};
pub use tasks::AppEvent;
