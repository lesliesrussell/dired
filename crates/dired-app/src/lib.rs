// dired-7qs
//! Application layer: buffers, commands, keymaps, minibuffer, and task
//! orchestration. The UI dispatches keys here; filesystem work runs in
//! background tasks through the adapter traits.

mod app;
pub mod buffers;
mod commands;
pub mod dirbuf;
pub mod keymap;
pub mod minibuffer;
pub mod registry;
pub mod services;
pub mod tasks;

pub use app::{expand_path, normalize, App};
pub use commands::parse_rg_json;
pub use services::{Effect, OpenOutcome, OpenRequest, Opener, Services, SystemOpener};
pub use tasks::AppEvent;
