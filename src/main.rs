// dired-7qs
//! `dired` — a terminal directory editor with Emacs Dired's interaction model.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use dired_app::{App, Services, SystemOpener};
use dired_config::Config;
use dired_domain::clock::SystemClock;
use dired_fs::{LocalFs, SystemTrash};
use dired_process::LocalRunner;

const USAGE: &str = "\
usage: dired [OPTIONS] [PATH]

Open PATH (default: the current directory) in a Dired-style directory buffer.

options:
  --check-config   validate the configuration file and exit
  -h, --help       show this help (inside dired, press ? for key bindings)
  -V, --version    show version

environment:
  DIRED_CONFIG     configuration file (default: $XDG_CONFIG_HOME/dired/config.toml)
  DIRED_STATE_DIR  history, journal, and diagnostics directory
  NO_COLOR         use the monochrome theme";

fn main() -> ExitCode {
    // Capture the local UTC offset before any threads start.
    dired_domain::init_local_offset();
    let mut path: Option<PathBuf> = None;
    let mut check = false;
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            Some("-h") | Some("--help") => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            Some("-V") | Some("--version") => {
                println!("dired {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            Some("--check-config") => check = true,
            Some(s) if s.starts_with('-') && s != "-" => {
                eprintln!("dired: unknown option {s}\n\n{USAGE}");
                return ExitCode::from(2);
            }
            _ if path.is_some() => {
                eprintln!("dired: only one PATH may be given");
                return ExitCode::from(2);
            }
            _ => path = Some(PathBuf::from(arg)),
        }
    }
    let config_path = dired_config::config_path();
    let (config, diags) = match &config_path {
        Some(p) => Config::load(p),
        None => (Config::defaults(), Vec::new()),
    };
    if check {
        let shown = config_path.map(|p| p.display().to_string()).unwrap_or_else(|| "(none)".into());
        let (_, keymap_diags) = dired_app::keymap::Keymaps::with_user(&config.keymap, "keymap");
        let all: Vec<_> = diags.iter().chain(keymap_diags.iter()).collect();
        for d in &all {
            eprintln!("{d}");
        }
        println!("{shown}: {} problem(s)", all.len());
        return if all.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let target = dired_app::expand_path(&path.unwrap_or(cwd.clone()).to_string_lossy(), &cwd);

    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("dired: cannot start runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = rt.block_on(async move {
        let svc = Services {
            git: Arc::new(dired_app::SystemGit::default()), // dired-99k
            fs: Arc::new(LocalFs),
            trash: Arc::new(SystemTrash),
            runner: Arc::new(LocalRunner),
            opener: Arc::new(SystemOpener { rules: config.opener.clone(), terminal: config.terminal.command.clone() }),
            clock: Arc::new(SystemClock),
            state_dir: dired_config::state_dir(),
        };
        let mut app = App::new(config, diags, svc);
        app.open_path(&target, false);
        dired_tui::run(app).await
    });
    rt.shutdown_timeout(std::time::Duration::from_millis(200));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dired: {e}");
            ExitCode::FAILURE
        }
    }
}
