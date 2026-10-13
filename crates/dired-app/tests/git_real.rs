// dired-99k
//! `SystemGit` against the real `git` binary, and against a fake one.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use dired_app::{GitProbe, SystemGit};

fn git_available() -> bool {
    Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[tokio::test]
async fn reads_a_real_repository() {
    if !git_available() {
        eprintln!("git not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    git(r, &["init", "-q", "-b", "trunk"]);
    for f in ["a.txt", "sp ace.txt", "d.txt", "sub/x.txt"] {
        write(&r.join(f), "1\n");
    }
    git(r, &["add", "."]);
    git(r, &["commit", "-q", "-m", "init"]);
    write(&r.join("a.txt"), "2\n"); // modified
    write(&r.join("b.txt"), "n\n");
    git(r, &["add", "b.txt"]); // staged add
    git(r, &["mv", "d.txt", "e.txt"]); // staged rename
    write(&r.join("c.txt"), "u\n"); // untracked
    write(&r.join("newdir/z"), "z\n"); // untracked directory
    write(&r.join("sub/x.txt"), "changed\n"); // modified inside a directory

    let g = SystemGit::default().status(r).await.expect("a repository");
    let xy = |p: &str| g.xy_for(Path::new(p)).map(|x| x.text());
    assert_eq!(xy("a.txt").as_deref(), Some(" M"));
    assert_eq!(xy("b.txt").as_deref(), Some("A "));
    assert_eq!(xy("e.txt").as_deref(), Some("R "));
    assert_eq!(xy("d.txt"), None);
    assert_eq!(xy("c.txt").as_deref(), Some("??"));
    assert_eq!(xy("newdir").as_deref(), Some("??"));
    assert_eq!(xy("sub").as_deref(), Some(" M"), "rolled up");
    assert_eq!(xy("sp ace.txt"), None, "clean");
    assert_eq!(g.summary(), "trunk*");

    // From a subdirectory: keys are relative to it, and only it is examined.
    let sub = SystemGit::default().status(&r.join("sub")).await.expect("still a repository");
    assert_eq!(sub.xy_for(Path::new("x.txt")).map(|x| x.text()).as_deref(), Some(" M"));
    assert_eq!(sub.xy_for(Path::new("a.txt")), None, "the pathspec limits git to this directory");
}

#[tokio::test]
async fn a_clean_repository_with_a_commit_has_no_marker_and_no_star() {
    if !git_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    write(&dir.path().join("f"), "x");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    let g = SystemGit::default().status(dir.path()).await.unwrap();
    assert!(!g.dirty());
    assert_eq!(g.summary(), "main");
}

#[tokio::test]
async fn not_a_repository_or_inside_git_dir_or_no_git_at_all_is_none() {
    let plain = tempfile::tempdir().unwrap();
    assert!(SystemGit::default().status(plain.path()).await.is_none() || git_available() && inside_some_repo(plain.path()));
    if git_available() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q"]);
        assert!(SystemGit::default().status(&repo.path().join(".git")).await.is_none(), "the .git directory is not a work tree");
    }
    let missing = SystemGit { program: "definitely-not-a-git-binary".into(), ..SystemGit::default() };
    assert!(missing.status(plain.path()).await.is_none());
    assert!(SystemGit::default().status(Path::new("/definitely/not/a/dir")).await.is_none());
}

/// The temp dir could itself sit inside a repository on an odd setup.
fn inside_some_repo(p: &Path) -> bool {
    Command::new("git").arg("-C").arg(p).args(["rev-parse", "--is-inside-work-tree"]).output().map(|o| o.status.success()).unwrap_or(false)
}

#[cfg(unix)]
mod fake {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, body: &str) -> std::path::PathBuf {
        let p = dir.join("fake-git");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[tokio::test]
    async fn runs_git_without_optional_locks_and_scoped_to_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let prog = script(
            dir.path(),
            &format!(
                r#"echo "locks=$GIT_OPTIONAL_LOCKS args=$*" >> {log}
case "$*" in
  *rev-parse*) printf 'true\nsrc/\n' ;;
  *status*) printf '## main\0 M src/x.rs\0' ;;
esac"#,
                log = log.display()
            ),
        );
        let probe = SystemGit { program: prog.into(), ..SystemGit::default() };
        let g = probe.status(dir.path()).await.expect("fake git answered");
        assert_eq!(g.xy_for(Path::new("x.rs")).map(|x| x.text()).as_deref(), Some(" M"), "prefix src/ applied");
        let log = std::fs::read_to_string(&log).unwrap();
        assert!(log.lines().all(|l| l.starts_with("locks=0 ")), "never takes the index lock:\n{log}");
        assert!(log.contains("rev-parse --is-inside-work-tree --show-prefix"), "{log}");
        assert!(log.contains("status --porcelain=v1 -z --branch --untracked-files=normal -- ."), "{log}");
    }

    #[tokio::test]
    async fn gives_up_on_a_slow_git() {
        let dir = tempfile::tempdir().unwrap();
        let prog = script(dir.path(), "sleep 5");
        let probe = SystemGit { program: prog.into(), timeout: Duration::from_millis(200) };
        let t = Instant::now();
        assert!(probe.status(dir.path()).await.is_none());
        assert!(t.elapsed() < Duration::from_secs(3), "returned after the timeout, not after the sleep");
    }
}
