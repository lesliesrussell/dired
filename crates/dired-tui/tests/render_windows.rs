// dired-ta2
//! Renders a split screen through ratatui's TestBackend.

use std::sync::Arc;

use dired_app::{App, OpenOutcome, OpenRequest, Opener, Services};
use dired_config::Config;
use dired_testkit::{MockClock, MockFs, MockRunner, MockTrash};
use dired_tui::render::draw;
use dired_tui::theme::Theme;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

struct NoOpen;
impl Opener for NoOpen {
    fn open(&self, _: &OpenRequest) -> Result<OpenOutcome, String> {
        Ok(OpenOutcome::Detached("none".into()))
    }
    // dired-0cz
    fn open_with(&self, _: &dired_config::OpenWithEntry, _: &[std::path::PathBuf]) -> Vec<Result<OpenOutcome, String>> {
        vec![Ok(OpenOutcome::Detached("none".into()))]
    }
}

fn screen(term: &Terminal<TestBackend>) -> Vec<String> {
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
        .collect()
}

#[tokio::test]
async fn side_by_side_windows_render_with_separator_and_mode_lines() {
    let fs = Arc::new(MockFs::new());
    fs.write("/left/alpha", "");
    fs.write("/right/omega", "");
    let svc = Services {
        fs: fs.clone(),
        trash: Arc::new(MockTrash::default()),
        runner: Arc::new(MockRunner::default()),
        opener: Arc::new(NoOpen),
        clock: Arc::new(MockClock::default()),
        state_dir: None,
    };
    let mut app = App::new(Config::defaults(), vec![], svc);
    let mut rx = app.take_events();
    let settle = |app: &mut App, rx: &mut tokio::sync::mpsc::UnboundedReceiver<dired_app::AppEvent>| {
        while let Ok(ev) = rx.try_recv() {
            app.handle_event(ev);
        }
    };
    app.open_dir("/left");
    let mut term = Terminal::new(TestBackend::new(100, 12)).unwrap();
    // First frame sizes the windows (100 columns allows a side-by-side split).
    term.draw(|f| draw(f, &mut app, &Theme::from_config(&Default::default()))).unwrap();
    app.press("C-x 4 d");
    app.press("C-a C-k");
    app.type_text("/right");
    app.press("RET");
    for _ in 0..50 {
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        settle(&mut app, &mut rx);
        if app.pending_tasks() == 0 {
            break;
        }
    }
    term.draw(|f| draw(f, &mut app, &Theme::from_config(&Default::default()))).unwrap();
    let s = screen(&term);
    let joined = s.join("\n");
    assert!(joined.contains("alpha") && joined.contains("omega"), "{joined}");
    // A vertical rule separates the windows on every body row.
    assert!(s[..10].iter().all(|row| row.chars().nth(49) == Some('│')), "{joined}");
    // Two mode lines on the row above the minibuffer.
    let mode = &s[10];
    assert!(mode.contains("left/") && mode.contains("right/"), "{mode}");
}
