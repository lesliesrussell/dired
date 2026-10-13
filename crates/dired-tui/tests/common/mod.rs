// dired-p2d
#![allow(dead_code)]

use std::sync::Arc;

use dired_app::{App, AppEvent, OpenOutcome, OpenRequest, Opener, Services};
use dired_config::{Config, OpenWithEntry};
use dired_testkit::{MockClock, MockFs, MockRunner, MockTrash};
use dired_tui::render::draw;
use dired_tui::theme::Theme;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use tokio::sync::mpsc::UnboundedReceiver;

pub struct NoOpen;

impl Opener for NoOpen {
    fn open(&self, _: &OpenRequest) -> Result<OpenOutcome, String> {
        Ok(OpenOutcome::Detached("none".into()))
    }
    fn open_with(&self, _: &OpenWithEntry, _: &OpenRequest) -> Result<OpenOutcome, String> {
        Ok(OpenOutcome::Detached("none".into()))
    }
}

pub fn screen(term: &Terminal<TestBackend>) -> Vec<String> {
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
        .collect()
}

/// Parse config text; the text must be free of diagnostics.
pub fn cfg(text: &str) -> Config {
    let (c, d) = Config::parse(text, "t");
    assert!(d.is_empty(), "{d:?}");
    c
}

pub fn theme_of(c: &Config) -> Theme {
    Theme::from_config(&c.theme)
}

pub struct Ui {
    pub app: App,
    pub term: Terminal<TestBackend>,
    pub fs: Arc<MockFs>,
    pub config: Config,
    rx: UnboundedReceiver<AppEvent>,
}

impl Ui {
    pub async fn open(fs: Arc<MockFs>, config: Config, w: u16, h: u16, dir: &str) -> Ui {
        let svc = Services {
            fs: fs.clone(),
            trash: Arc::new(MockTrash::default()),
            runner: Arc::new(MockRunner::default()),
            opener: Arc::new(NoOpen),
            clock: Arc::new(MockClock::default()),
            state_dir: None,
        };
        let mut app = App::new(config.clone(), vec![], svc);
        let rx = app.take_events();
        app.open_dir(dir);
        let term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut ui = Ui { app, term, fs, config, rx };
        ui.settle().await;
        ui.draw(); // first frame sizes the windows
        ui
    }

    pub async fn settle(&mut self) {
        for _ in 0..100 {
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            while let Ok(ev) = self.rx.try_recv() {
                self.app.handle_event(ev);
            }
            if self.app.pending_tasks() == 0 {
                break;
            }
        }
    }

    /// Draw one frame with the theme from the *app's current* config.
    pub fn draw(&mut self) -> Vec<String> {
        let theme = Theme::from_config(&self.app.config.theme);
        self.term.draw(|f| draw(f, &mut self.app, &theme)).unwrap();
        screen(&self.term)
    }

    pub fn cell_fg(&self, x: u16, y: u16) -> Option<Color> {
        self.term.backend().buffer()[(x, y)].style().fg
    }
}
