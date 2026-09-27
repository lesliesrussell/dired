// dired-7qs
//! Acceptance tests T1–T16 from spec.md.

mod common;

use std::ffi::OsString;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use dired_app::buffers::{BufferKind, PreviewPhase};
use dired_config::Config;
use dired_domain::plan::Conflict;
use dired_domain::{EntryState, OpKind};
use dired_testkit::{FsOp, MockFs, MockTrash};

fn fixture() -> (Arc<MockFs>, H) {
    let (fs, h) = mock();
    fs.write("/w/a.txt", "A");
    fs.write("/w/b.txt", "B");
    fs.write("/w/c.log", "C");
    fs.mkdir_p("/w/sub");
    fs.write("/w/sub/inner.txt", "I");
    (fs, h)
}

#[tokio::test]
async fn t1_open_and_visit() {
    let (_fs, mut h) = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    assert_eq!(h.names(), vec!["..", "sub", "a.txt", "b.txt", "c.log"]);
    // Each row carries metadata once enrichment completes.
    h.goto("a.txt");
    assert!(h.dir().point_entry().unwrap().meta.is_some());
    h.press("RET");
    let opened = h.opener.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].path, p("/w/a.txt"));
    // Directory produces a directory buffer...
    h.goto("sub");
    h.press("RET");
    h.settle().await;
    assert_eq!(h.dir().root, p("/w/sub"));
    assert_eq!(h.names(), vec!["..", "inner.txt"]);
    // ...and visiting an already-open directory focuses it.
    h.press("C-x d");
    h.answer("/w");
    h.settle().await;
    assert_eq!(h.dir().root, p("/w"));
    assert_eq!(h.app.buffers().len(), 2);
    // `^` from sub focuses the parent with point on `sub`.
    h.press("C-x d");
    h.answer("/w/sub");
    h.press("^");
    h.settle().await;
    assert_eq!(h.dir().root, p("/w"));
    assert_eq!(h.point_name(), "sub");
}

#[tokio::test]
async fn t2_refresh_preserves_point() {
    let (t, mut h) = local();
    let root = t.path().to_path_buf();
    write(&root.join("b.txt"), "b");
    write(&root.join("c.txt"), "c");
    h.app.open_dir(&root);
    h.settle().await;
    h.goto("b.txt");
    let before = h.dir().point;
    write(&root.join("a.txt"), "a");
    h.press("g");
    h.settle().await;
    assert_eq!(h.point_name(), "b.txt");
    assert_eq!(h.dir().point, before + 1);
    assert!(h.names().contains(&"a.txt".to_string()));
}

#[tokio::test]
async fn t3_independent_buffers() {
    let (fs, mut h) = mock();
    for n in ["x1", "x2", "y1"] {
        fs.write(format!("/x/{n}"), n);
        fs.write(format!("/y/{n}"), n);
    }
    h.app.open_dir("/x");
    h.settle().await;
    h.goto("x2");
    h.press("m");
    h.press("s"); // sort: natural
    h.press("/");
    h.answer("x");
    h.app.open_dir("/y");
    h.settle().await;
    h.goto("y1");
    h.press("d");
    h.press("C-x M-o"); // hidden files off
    h.press("b");
    h.answer("x/");
    assert_eq!(h.dir().root, p("/x"));
    assert_eq!(h.state("x2"), EntryState::Marked);
    assert_eq!(h.dir().state(&p("y1")), EntryState::Unmarked);
    assert_eq!(h.dir().filter.as_deref(), Some("x"));
    assert_eq!(h.dir().sort.key, dired_domain::SortKey::Natural);
    assert!(h.dir().show_hidden);
    assert_eq!(h.names(), vec!["..", "x1", "x2"]);
    h.press("b");
    h.answer("y/");
    assert_eq!(h.dir().root, p("/y"));
    assert_eq!(h.state("y1"), EntryState::DeleteFlagged);
    assert_eq!(h.state("x2"), EntryState::Unmarked);
    assert_eq!(h.dir().filter, None);
    assert_eq!(h.dir().sort.key, dired_domain::SortKey::Name);
    assert!(!h.dir().show_hidden);
}

#[tokio::test]
async fn t4_mark_fallback() {
    let (fs, mut h) = fixture();
    fs.mkdir_p("/dst");
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("b.txt");
    h.press("C");
    h.answer("/dst");
    h.settle().await;
    let srcs: Vec<_> = h.preview().plan.items.iter().map(|i| i.src.clone()).collect();
    assert_eq!(srcs, vec![p("/w/b.txt")]);
    h.press("q");
    h.goto("a.txt");
    h.press("m");
    h.goto("c.log");
    h.press("m");
    h.goto("b.txt"); // point elsewhere: marks win
    h.press("C");
    h.answer("/dst");
    h.settle().await;
    let srcs: Vec<_> = h.preview().plan.items.iter().map(|i| i.src.clone()).collect();
    assert_eq!(srcs, vec![p("/w/a.txt"), p("/w/c.log")]);
}

#[tokio::test]
async fn t5_regex_marking() {
    let (_fs, mut h) = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.press("% m");
    h.answer(r"\.txt$");
    assert_eq!(h.state("a.txt"), EntryState::Marked);
    assert_eq!(h.state("b.txt"), EntryState::Marked);
    assert_eq!(h.state("c.log"), EntryState::Unmarked);
    assert_eq!(h.state("sub"), EntryState::Unmarked);
    assert_eq!(h.dir().counts().marked, 2);
    // `% d` flags; `U` clears marks but keeps flags.
    h.press("% d");
    h.answer("^c");
    h.press("U");
    assert_eq!(h.state("a.txt"), EntryState::Unmarked);
    assert_eq!(h.state("c.log"), EntryState::DeleteFlagged);
}

#[tokio::test]
async fn t6_delete_is_deferred() {
    let (t, mut h) = local();
    let root = t.path().to_path_buf();
    write(&root.join("f"), "keep");
    h.app.open_dir(&root);
    h.settle().await;
    h.goto("f");
    h.press("d");
    assert!(root.join("f").exists());
    assert_eq!(h.state("f"), EntryState::DeleteFlagged);
    h.press("x");
    h.settle().await;
    assert_eq!(h.preview().plan.kind, OpKind::Trash);
    assert_eq!(h.preview().plan.items.len(), 1);
    h.press("q"); // reject the preview
    h.settle().await;
    assert!(root.join("f").exists());
    assert!(h.trash.received().is_empty());
    assert_eq!(h.state("f"), EntryState::DeleteFlagged);
    // C-g also rejects.
    h.press("x");
    h.settle().await;
    h.press("C-g");
    h.settle().await;
    assert!(root.join("f").exists());
    assert!(h.trash.received().is_empty());
}

#[tokio::test]
async fn t7_trash_execution() {
    let (fs, mut h) = fixture();
    h.trash.fail_on("/w/b.txt");
    h.app.open_dir("/w");
    h.settle().await;
    for n in ["a.txt", "b.txt", "c.log"] {
        h.goto(n);
        h.press("d");
    }
    h.press("x");
    h.settle().await;
    assert!(h.preview().plan.backend.contains("mock trash"));
    h.press("x"); // confirm in preview
    h.settle().await;
    assert_eq!(h.trash.received(), vec![p("/w/a.txt"), p("/w/b.txt"), p("/w/c.log")]);
    match &h.preview().phase {
        PreviewPhase::Done(r) => {
            assert_eq!(r.succeeded(), 2);
            assert_eq!(r.failed(), 1);
        }
        other => panic!("not done: {other:?}"),
    }
    assert!(!fs.exists_now("/w/a.txt"));
    h.press("q");
    h.settle().await;
    assert_eq!(h.names(), vec!["..", "sub", "b.txt"]);
    assert_eq!(h.state("b.txt"), EntryState::DeleteFlagged);
    let key = p("b.txt");
    assert!(h.dir().annotations.get(&key).unwrap().contains("permission denied"));
    assert_eq!(h.app.journal().len(), 1);
}

#[tokio::test]
async fn t8_copy_conflict_preview() {
    let (fs, mut h) = fixture();
    fs.write("/dst/a.txt", "OLD");
    h.app.open_dir("/w");
    h.settle().await;
    h.press("% m");
    h.answer(r"^[ab]\.txt$");
    h.press("C");
    h.answer("/dst/");
    h.settle().await;
    {
        let pv = h.preview();
        assert_eq!(pv.plan.items[0].conflict, Some(Conflict::DestinationExists));
        assert_eq!(pv.plan.items[1].conflict, None);
        assert_eq!(pv.plan.unresolved(), 1);
    }
    fs.clear_ops();
    h.press("x");
    h.settle().await;
    assert!(matches!(h.preview().phase, PreviewPhase::Pending));
    assert!(fs.ops().is_empty(), "no execution before a policy is chosen");
    assert!(h.echo().contains("conflict"));
    // Skip all → executes the non-conflicting copy only.
    h.press("S");
    h.press("x");
    h.settle().await;
    assert_eq!(fs.read("/dst/a.txt").unwrap(), "OLD");
    assert_eq!(fs.read("/dst/b.txt").unwrap(), "B");
    // Overwrite requires an explicit typed confirmation.
    h.press("q");
    h.goto("a.txt");
    h.press("U");
    h.press("C");
    h.answer("/dst");
    h.settle().await;
    h.press("o");
    h.press("x");
    assert!(h.app.prompt().unwrap().label.contains("Overwrite 1"));
    h.answer("no");
    h.settle().await;
    assert_eq!(fs.read("/dst/a.txt").unwrap(), "OLD");
    h.press("x");
    h.answer("yes");
    h.settle().await;
    assert_eq!(fs.read("/dst/a.txt").unwrap(), "A");
}

#[tokio::test]
async fn t9_cross_device_move() {
    let (fs, mut h) = mock();
    fs.write("/src/f", "data");
    fs.mkdir_p("/mnt/usb");
    fs.mount("/mnt/usb", 2);
    h.app.open_dir("/src");
    h.settle().await;
    h.goto("f");
    h.press("R");
    h.answer("/mnt/usb");
    h.settle().await;
    {
        let pv = h.preview();
        assert!(pv.plan.items[0].cross_device);
        assert!(pv.plan.warnings.iter().any(|w| w.contains("non-atomic")));
    }
    fs.clear_ops();
    h.press("x");
    h.settle().await;
    let ops = fs.ops();
    let copy = ops.iter().position(|o| matches!(o, FsOp::Copy { .. })).expect("copy");
    let remove = ops.iter().position(|o| *o == FsOp::Remove(p("/src/f"))).expect("remove");
    assert!(copy < remove, "destination copy must precede source removal: {ops:?}");
    assert_eq!(fs.read("/mnt/usb/f").unwrap(), "data");
    assert!(!fs.exists_now("/src/f"));

    // A failing copy never deletes the source.
    fs.write("/src/g", "g");
    fs.fail_on("/mnt/usb/g", "disk full");
    h.press("q");
    h.press("g");
    h.settle().await;
    h.goto("g");
    h.press("R");
    h.answer("/mnt/usb");
    h.settle().await;
    h.press("x");
    h.settle().await;
    assert!(fs.exists_now("/src/g"));
}

#[tokio::test]
async fn t10_wdired_validation() {
    let (t, mut h) = local();
    let root = t.path().to_path_buf();
    for n in ["a", "b", "c"] {
        write(&root.join(n), n);
    }
    h.app.open_dir(&root);
    h.settle().await;
    h.goto("a");
    h.press("C-x C-q");
    h.press("C-e DEL");
    h.typ("z");
    h.press("C-n C-e DEL");
    h.typ("z");
    assert_eq!(h.names(), vec!["..", "z", "z", "c"]);
    h.press("C-c C-c");
    {
        let pv = h.preview();
        assert!(!pv.plan.errors.is_empty());
        assert!(pv.plan.errors.iter().any(|e| e.contains("both renamed")));
        assert!(matches!(pv.plan.items[0].conflict, Some(Conflict::Collision(_))));
        let (lines, _) = pv.lines();
        assert!(lines.iter().any(|l| l.text.contains("collision")));
    }
    h.press("x");
    h.settle().await;
    for n in ["a", "b", "c"] {
        assert_eq!(std::fs::read_to_string(root.join(n)).unwrap(), n);
    }
    assert!(!root.join("z").exists());
    // Back in the directory buffer, still editing; C-c C-k restores names.
    h.press("q");
    assert!(h.dir().wdired.is_some());
    h.press("C-c C-k");
    assert_eq!(h.names(), vec!["..", "a", "b", "c"]);
}

#[tokio::test]
async fn t11_wdired_rename_cycle() {
    let (t, mut h) = local();
    let root = t.path().to_path_buf();
    write(&root.join("a"), "contents of a");
    write(&root.join("b"), "contents of b");
    h.app.open_dir(&root);
    h.settle().await;
    h.goto("a");
    h.press("m"); // mark follows the file through the rename
    h.goto("a");
    h.press("C-x C-q");
    h.press("C-e DEL");
    h.typ("b");
    h.press("C-n C-e DEL");
    h.typ("a");
    h.press("C-c C-c");
    assert!(h.preview().plan.errors.is_empty());
    assert_eq!(h.preview().plan.steps.len(), 3, "cycle broken via temp name");
    h.press("x");
    h.settle().await;
    assert_eq!(std::fs::read_to_string(root.join("a")).unwrap(), "contents of b");
    assert_eq!(std::fs::read_to_string(root.join("b")).unwrap(), "contents of a");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2, "no temp files left");
    h.press("q");
    h.settle().await;
    assert!(h.dir().wdired.is_none());
    assert_eq!(h.state("b"), EntryState::Marked);
    assert_eq!(h.state("a"), EntryState::Unmarked);
    // Best-effort undo reverses the swap.
    h.press("C-x u");
    h.settle().await;
    assert_eq!(std::fs::read_to_string(root.join("a")).unwrap(), "contents of a");
}

#[tokio::test]
async fn t12_prompt_cancellation() {
    let (fs, mut h) = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("a.txt");
    h.press("m");
    h.goto("b.txt");
    fs.clear_ops();
    let before_names = h.names();
    let before_point = h.dir().point;
    let prompts = [
        "C-x d", "C-x C-f", "C", "R", "+", "M", "!", "&", "% m", "% d", "% u", "% r", "A", "j", "/", "b",
        "C-x k", "M-x", "C-s", "C-r",
    ];
    for seq in prompts {
        h.press(seq);
        assert!(h.app.prompt().is_some(), "{seq} opens a prompt");
        h.typ("zz");
        h.press("C-g");
        assert!(h.app.prompt().is_none(), "{seq} cancelled");
        h.settle().await;
        assert_eq!(h.app.buffers().len(), 1, "{seq}");
        assert_eq!(h.dir().point, before_point, "{seq}");
        assert_eq!(h.names(), before_names, "{seq}");
        assert_eq!(h.state("a.txt"), EntryState::Marked, "{seq}");
        assert!(h.dir().filter.is_none(), "{seq}");
    }
    // The second stage of `% r`, and isearch that moved point, also cancel cleanly.
    h.press("% r");
    h.answer("a");
    h.typ("q");
    h.press("C-g");
    h.press("C-s");
    h.typ("c.l");
    assert_eq!(h.point_name(), "c.log");
    h.press("C-g");
    assert_eq!(h.dir().point, before_point);
    // Prefix keys cancel too.
    h.press("C-x");
    assert!(h.app.key_hint().is_some());
    h.press("C-g");
    assert!(h.app.key_hint().is_none());
    h.settle().await;
    assert!(fs.ops().is_empty(), "no filesystem mutation: {:?}", fs.ops());
    assert!(h.runner.spawned().is_empty());
}

#[tokio::test]
async fn t13_inline_expansion() {
    let (fs, mut h) = mock();
    fs.write("/t/d1/d2/f", "");
    fs.write("/t/d1/g", "");
    fs.write("/t/z", "");
    h.app.open_dir("/t");
    h.settle().await;
    h.goto("d1");
    h.press("i");
    h.settle().await;
    assert_eq!(
        h.rows_with_depth(),
        vec![("..".into(), 0), ("d1".into(), 0), ("d2".into(), 1), ("g".into(), 1), ("z".into(), 0)]
    );
    h.goto("d2");
    h.press("i");
    h.settle().await;
    assert_eq!(
        h.rows_with_depth(),
        vec![("..".into(), 0), ("d1".into(), 0), ("d2".into(), 1), ("f".into(), 2), ("g".into(), 1), ("z".into(), 0)]
    );
    // Marks in subtrees are addressable operands.
    h.goto("f");
    h.press("m");
    assert_eq!(h.dir().operands()[0].key, p("d1/d2/f"));
    h.goto("f");
    // `$` inside the subtree collapses the containing subdir.
    h.press("$");
    assert_eq!(h.point_name(), "d2");
    h.goto("d1");
    h.press("$");
    assert_eq!(h.names(), vec!["..", "d1", "z"]);
    // Directory symlinks are not expanded by default.
    fs.symlink("/t/loop", "/t");
    h.press("g");
    h.settle().await;
    h.goto("loop");
    h.press("i");
    h.settle().await;
    assert!(h.echo().contains("symlink"));
    assert_eq!(h.names().len(), 4);
}

#[tokio::test]
async fn t14_argument_safe_shell() {
    let (fs, mut h) = mock();
    let names = ["a b.txt", "it's", "q\"uote", "line\nbreak", "$(rm -rf ~)", ";ls|cat>x", "-rf", "`id`"];
    for n in names {
        fs.write(format!("/s/{n}"), "");
    }
    h.app.open_dir("/s");
    h.settle().await;
    h.press("t");
    h.press("!");
    h.answer("echo");
    h.settle().await;
    let argv = h.runner.argv();
    assert_eq!(argv.len(), 1);
    assert_eq!(argv[0][0], OsString::from("echo"));
    let mut got: Vec<String> = argv[0][1..].iter().map(|a| a.to_string_lossy().into_owned()).collect();
    got.sort();
    let mut want: Vec<String> = names
        .iter()
        .map(|n| if n.starts_with('-') { format!("./{n}") } else { n.to_string() })
        .collect();
    want.sort();
    assert_eq!(got, want, "argv values are exact (option-like names get ./)");
    assert_eq!(h.runner.spawned()[0].cwd, p("/s"));
    // `?` runs once per file.
    h.press("!");
    h.answer("cat ?");
    h.settle().await;
    assert_eq!(h.runner.argv().len(), 1 + names.len());
    // Shell mode is opt-in, labeled, confirmed, and still passes files as "$@".
    h.press("M-x");
    h.answer("dired-do-shell-command-in-shell");
    assert!(h.app.prompt().unwrap().label.contains("SHELL"));
    h.answer("wc -l");
    assert!(h.app.prompt().unwrap().label.contains("Type yes"));
    h.answer("yes");
    h.settle().await;
    let last = h.runner.argv().last().unwrap().clone();
    assert_eq!(last[0], OsString::from("/bin/sh"));
    assert_eq!(last[2], OsString::from("wc -l \"$@\""));
    assert!(last[4..].iter().any(|a| a == "$(rm -rf ~)"));
    // `&` streams into a focused process buffer.
    h.runner.set_output(vec!["hello".into()]);
    h.press("b");
    h.answer("s/");
    h.press("&");
    h.answer("echo");
    h.settle().await;
    assert_eq!(h.app.active().name, "*Async Shell Command*");
    match &h.app.active().kind {
        BufferKind::Text(t) => assert!(t.lines.iter().any(|l| l.text == "hello")),
        _ => panic!(),
    }
}

#[tokio::test]
async fn t15_large_directory_responsiveness() {
    let fs = Arc::new(MockFs::new().with_batch(1000));
    fs.add_many("/big", 100_000);
    let mut h = harness(fs.clone(), Arc::new(MockTrash::default()));
    let start = Instant::now();
    h.app.open_dir("/big");
    h.app.set_viewport(50);
    let mut first_viewport = None;
    let mut max_event = Duration::ZERO;
    let mut moved_during_scan = false;
    while h.app.pending_tasks() > 0 {
        let ev = h.rx.recv().await.unwrap();
        let t0 = Instant::now();
        h.app.handle_event(ev);
        h.app.prepare_frame();
        max_event = max_event.max(t0.elapsed());
        let scanning = h.dir().is_scanning();
        if first_viewport.is_none() && h.dir().rows().len() > 50 {
            assert!(scanning, "rows are renderable before the scan completes");
            first_viewport = Some(start.elapsed());
        }
        if scanning && first_viewport.is_some() && !moved_during_scan {
            let before = h.dir().point;
            h.press("C-n");
            h.app.prepare_frame();
            assert_eq!(h.dir().point, before + 1, "input handled during scan");
            moved_during_scan = true;
        }
    }
    let first = first_viewport.expect("viewport");
    assert!(moved_during_scan);
    assert_eq!(h.dir().counts().entries, 100_000);
    // Generous bounds so unoptimized test builds pass; release is far faster.
    let budget = if cfg!(debug_assertions) { 4 } else { 1 };
    assert!(first < Duration::from_millis(250 * budget), "first viewport {first:?}");
    assert!(max_event < Duration::from_millis(16 * budget * 4), "max event+frame {max_event:?}");
    // Memory: ~entries * (Entry + metadata) stays well under 100 MB.
    let per_entry = std::mem::size_of::<dired_domain::Entry>() + std::mem::size_of::<dired_domain::EntryMetadata>() + 64;
    assert!(per_entry * 100_000 < 100 * 1024 * 1024);
    eprintln!("T15: first viewport {first:?}, max event+frame {max_event:?}");
}

#[tokio::test]
async fn t16_keymap_diagnostics() {
    let (config, diags) = Config::parse(
        r#"
version = 1
[[keymap]]
key = "C-c o"
command = "no-such-command"
[[keymap]]
key = "C-c v"
command = "dired-visit"
[[keymap]]
key = "C-c  v"
command = "dired-mark"
[[keymap]]
key = "C-c e"
exec = ["open", "-R", "{file}"]
"#,
        "test.toml",
    );
    let (fs, _) = fixture();
    let mut h = harness_with(fs, Arc::new(MockTrash::default()), config, diags);
    let d: Vec<String> = h.app.diagnostics().iter().map(|d| d.to_string()).collect();
    assert_eq!(d.len(), 2, "{d:?}");
    assert!(d.iter().any(|m| m.contains("unknown command `no-such-command`")));
    assert!(d.iter().any(|m| m.contains("duplicate chord `C-c v`")));
    assert!(h.app.find_buffer("*Diagnostics*").is_some());
    // Still usable: the valid bindings work.
    h.app.open_dir("/w");
    h.settle().await;
    h.goto("a.txt");
    h.press("C-c v"); // last duplicate wins: dired-mark
    assert_eq!(h.state("a.txt"), EntryState::Marked);
    h.goto("b.txt");
    h.press("U");
    h.press("C-c e");
    h.settle().await;
    assert_eq!(h.runner.argv()[0], vec![OsString::from("open"), "-R".into(), "b.txt".into()]);
    // Plugin bindings are accepted but report unavailability.
    let (config, diags) = Config::parse("version = 1\n[[keymap]]\nkey = \"C-c p\"\nplugin = \"future\"\n", "t");
    h.app.apply_config(config, diags, "t");
    assert!(h.app.diagnostics().is_empty());
    h.press("C-c p");
    assert!(h.echo().contains("plugin"));
}

#[tokio::test]
async fn minibuffer_completion_history_and_kill_confirmation() {
    let (_fs, mut h) = fixture();
    h.app.open_dir("/w");
    h.settle().await;
    // Asynchronous path completion.
    h.press("C-x d");
    h.press("C-a C-k");
    h.typ("/w/su");
    h.press("TAB");
    h.settle().await;
    assert_eq!(h.app.prompt().unwrap().input, "/w/sub/");
    h.press("RET");
    h.settle().await;
    assert_eq!(h.dir().root, p("/w/sub"));
    // History recall with M-p.
    h.press("C-x d");
    h.press("C-a C-k M-p");
    assert_eq!(h.app.prompt().unwrap().input, "/w/sub/");
    h.press("C-g");
    // Fuzzy command completion.
    h.press("M-x");
    h.typ("togglhid");
    h.press("RET");
    assert!(!h.dir().show_hidden);
    // Kill confirmation only with uncommitted Wdired edits.
    h.press("C-x C-q");
    h.typ("x");
    assert!(h.echo().contains("Read-only"), "`..` is not editable");
    h.press("C-n");
    h.typ("x");
    h.press("C-x k RET");
    assert!(h.app.prompt().unwrap().label.contains("Wdired"));
    h.press("n");
    assert_eq!(h.app.buffers().len(), 2);
    h.press("C-x k RET");
    h.press("y");
    assert_eq!(h.app.buffers().len(), 1);
    h.press("C-x k RET");
    assert!(h.app.prompt().is_none(), "no confirmation without unfinished work");
    assert!(h.app.should_quit());
}

#[tokio::test]
async fn hidden_toggle_keeps_marks_and_help_lists_bindings() {
    let (fs, mut h) = fixture();
    fs.write("/w/.dot", "");
    h.app.open_dir("/w");
    h.settle().await;
    h.goto(".dot");
    h.press("m");
    h.press("C-x M-o");
    assert!(!h.names().contains(&".dot".to_string()));
    assert_eq!(h.dir().counts().marked, 1);
    h.press("C-x M-o");
    assert_eq!(h.state(".dot"), EntryState::Marked);
    h.press("?");
    match &h.app.active().kind {
        BufferKind::Text(t) => {
            assert!(t.lines.iter().any(|l| l.text.contains("dired-do-flagged-delete")));
            assert!(t.lines.iter().any(|l| l.text.contains("% m")));
        }
        _ => panic!("help buffer"),
    }
}

#[tokio::test]
async fn search_results_visit_location() {
    let (_fs, mut h) = fixture();
    h.runner.set_output(vec![
        r#"{"type":"begin","data":{"path":{"text":"/w/a.txt"}}}"#.into(),
        r#"{"type":"match","data":{"path":{"text":"/w/a.txt"},"lines":{"text":"hello\n"},"line_number":7,"submatches":[]}}"#.into(),
    ]);
    h.app.open_dir("/w");
    h.settle().await;
    h.press("A");
    h.answer("hel+o");
    h.settle().await;
    let argv = h.runner.argv().pop().unwrap();
    assert_eq!(argv, vec![OsString::from("rg"), "--json".into(), "-e".into(), "hel+o".into(), "/w".into()]);
    h.press("M-<");
    h.press("C-s");
    h.typ("a.txt:7");
    h.press("RET");
    h.press("RET");
    let opened = h.opener.opened.lock().unwrap().clone();
    assert_eq!(opened.last().unwrap().path, p("/w/a.txt"));
    assert_eq!(opened.last().unwrap().line, Some(7));
}

#[tokio::test]
async fn review_regressions() {
    // #1: overwriting a destination that contains the source is blocked.
    let (t, mut h) = local();
    let root = t.path().to_path_buf();
    std::fs::create_dir_all(root.join("d/d")).unwrap();
    write(&root.join("d/d/precious"), "keep");
    h.app.open_dir(root.join("d"));
    h.settle().await;
    h.goto("d");
    h.press("R");
    h.answer(&root.display().to_string());
    h.settle().await;
    assert_eq!(h.preview().plan.items[0].conflict, Some(Conflict::DestinationContainsSource));
    h.press("o");
    assert!(h.echo().contains("not valid"));
    h.press("O x");
    h.settle().await;
    assert!(root.join("d/d/precious").exists());
    h.press("q");

    // #2: copying a directory into itself through a symlink is detected.
    std::os::unix::fs::symlink(root.join("d"), root.join("ln")).unwrap();
    h.app.open_dir(&root);
    h.settle().await;
    h.goto("d");
    h.press("C");
    h.answer(&root.join("ln").display().to_string());
    h.settle().await;
    assert_eq!(h.preview().plan.items[0].conflict, Some(Conflict::SelfRecursive));
    h.press("q");

    // #5: wdired commands outside a directory buffer report instead of panicking.
    h.press("?");
    h.press("M-x");
    h.answer("wdired-toggle-edit");
    assert!(h.echo().contains("directory buffer"));
    h.press("q");

    // #6: a preview arriving while a prompt is open doesn't steal the prompt's buffer.
    h.press("C");
    h.answer(&root.join("copy").display().to_string());
    h.press("% m"); // prompt opens before the plan task delivers
    h.settle().await;
    h.typ("pre");
    h.press("RET");
    assert!(h.app.active().dir().is_some());

    // #4: a failed overwrite restores the previous destination.
    let (fs, mut h) = mock();
    fs.write("/s/f", "new");
    fs.write("/d/f", "old");
    fs.fail_on("/s/f", "read error"); // copy fails; stat still works
    h.app.open_dir("/s");
    h.settle().await;
    h.goto("f");
    h.press("C");
    h.answer("/d");
    h.settle().await;
    assert_eq!(h.preview().plan.items[0].conflict, Some(Conflict::DestinationExists));
    h.press("o x");
    h.answer("yes");
    h.settle().await;
    assert!(matches!(&h.preview().phase, PreviewPhase::Done(r) if r.failed() == 1));
    assert_eq!(fs.read("/d/f").unwrap(), "old", "old destination restored");

    // Gap: flags on hidden entries are included in the deletion preview.
    let (fs, mut h) = mock();
    fs.write("/h/.x", "");
    fs.write("/h/y", "");
    h.app.open_dir("/h");
    h.settle().await;
    h.goto(".x");
    h.press("d");
    h.goto("y");
    h.press("d");
    h.press("C-x M-o");
    h.press("x");
    h.settle().await;
    assert_eq!(h.preview().plan.items.len(), 2);
}
