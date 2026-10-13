// dired-7qs
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dired_app::buffers::{BufferKind, PreviewBuffer};
use dired_app::dirbuf::DirBuffer;
use dired_app::{App, AppEvent, OpenOutcome, OpenRequest, Opener, Services};
use dired_config::{Config, Diagnostic, OpenWithEntry};
use dired_domain::EntryState;
use dired_fs::{FileSystem, LocalFs};
use dired_testkit::{MockClock, MockFs, MockRunner, MockTrash};
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Default)]
pub struct MockOpener {
    pub opened: Mutex<Vec<OpenRequest>>,
    // dired-0cz
    pub opened_with: Mutex<Vec<(String, Vec<std::path::PathBuf>)>>,
}

impl Opener for MockOpener {
    fn open(&self, req: &OpenRequest) -> Result<OpenOutcome, String> {
        self.opened.lock().unwrap().push(req.clone());
        Ok(OpenOutcome::Detached("mock".into()))
    }
    // dired-0cz
    fn open_with(&self, entry: &OpenWithEntry, paths: &[std::path::PathBuf]) -> Vec<Result<OpenOutcome, String>> {
        self.opened_with.lock().unwrap().push((entry.name.clone(), paths.to_vec()));
        vec![Ok(OpenOutcome::Detached(entry.name.clone()))]
    }
}

pub struct H {
    pub app: App,
    pub rx: UnboundedReceiver<AppEvent>,
    pub opener: Arc<MockOpener>,
    pub runner: Arc<MockRunner>,
    pub trash: Arc<MockTrash>,
    // dired-tlu
    pub clock: Arc<MockClock>,
}

pub fn harness(fs: Arc<dyn FileSystem>, trash: Arc<MockTrash>) -> H {
    harness_with(fs, trash, Config::defaults(), vec![])
}

pub fn harness_with(fs: Arc<dyn FileSystem>, trash: Arc<MockTrash>, config: Config, diags: Vec<Diagnostic>) -> H {
    let opener = Arc::new(MockOpener::default());
    let runner = Arc::new(MockRunner::default());
    let clock = Arc::new(MockClock::default());
    let svc = Services {
        fs,
        trash: trash.clone(),
        runner: runner.clone(),
        opener: opener.clone(),
        clock: clock.clone(),
        state_dir: None,
    };
    let mut app = App::new(config, diags, svc);
    let rx = app.take_events();
    H { app, rx, opener, runner, trash, clock }
}

pub fn mock() -> (Arc<MockFs>, H) {
    let fs = Arc::new(MockFs::new());
    let trash = Arc::new(MockTrash::with_fs(fs.clone()));
    let h = harness(fs.clone(), trash);
    (fs, h)
}

pub fn local() -> (tempfile::TempDir, H) {
    let t = tempfile::tempdir().unwrap();
    let h = harness(Arc::new(LocalFs), Arc::new(MockTrash::default()));
    (t, h)
}

impl H {
    /// Process events until every background task has finished.
    pub async fn settle(&mut self) {
        let fut = async {
            while self.app.pending_tasks() > 0 {
                let ev = self.rx.recv().await.expect("event channel open");
                self.app.handle_event(ev);
            }
        };
        tokio::time::timeout(Duration::from_secs(20), fut).await.expect("tasks settle");
    }

    pub fn press(&mut self, seq: &str) {
        self.app.press(seq);
    }

    pub fn typ(&mut self, s: &str) {
        self.app.type_text(s);
    }

    /// Answer the open prompt: clear it, type, RET.
    pub fn answer(&mut self, s: &str) {
        assert!(self.app.prompt().is_some(), "expected a prompt");
        self.press("C-a C-k");
        self.typ(s);
        self.press("RET");
    }

    pub fn dir(&mut self) -> &mut DirBuffer {
        self.app.active_mut().dir_mut().expect("directory buffer")
    }

    pub fn names(&mut self) -> Vec<String> {
        let d = self.dir();
        d.ensure_view();
        d.rows().iter().map(|r| d.display_name(r)).collect()
    }

    pub fn rows_with_depth(&mut self) -> Vec<(String, usize)> {
        let d = self.dir();
        d.ensure_view();
        d.rows().iter().map(|r| (d.entry(r).name.clone(), r.depth)).collect()
    }

    pub fn point_name(&mut self) -> String {
        self.dir().point_entry().map(|e| e.name).unwrap_or_default()
    }

    pub fn goto(&mut self, name: &str) {
        let names = self.names();
        let i = names.iter().position(|n| n == name).unwrap_or_else(|| panic!("{name} not in {names:?}"));
        self.dir().set_point(i);
    }

    pub fn state(&mut self, name: &str) -> EntryState {
        let d = self.dir();
        d.ensure_view();
        let key = d.rows().iter().map(|r| d.entry(r).key.clone()).find(|k| k.file_name().map(|f| f == name).unwrap_or(false)).expect("entry");
        d.state(&key)
    }

    pub fn preview(&mut self) -> &mut PreviewBuffer {
        let name = self.app.active().name.clone();
        match &mut self.app.active_mut().kind {
            BufferKind::Preview(p) => p,
            _ => panic!("active buffer is not a preview: {name}"),
        }
    }

    pub fn echo(&self) -> String {
        self.app.echo().map(|(m, _)| m.to_string()).unwrap_or_default()
    }
}

pub fn write(p: &Path, s: &str) {
    std::fs::write(p, s).unwrap();
}

pub fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}
