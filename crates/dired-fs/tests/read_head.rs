// dired-6xv
//! `FileSystem::read_head`: only the start of a file is ever read.

use dired_fs::{FileSystem, FsErrorKind, LocalFs};

#[tokio::test]
async fn reads_at_most_the_requested_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("big");
    let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&p, &data).unwrap();
    let head = LocalFs.read_head(&p, 100).await.unwrap();
    assert_eq!(head, data[..100]);
}

#[tokio::test]
async fn short_and_empty_files_come_back_whole() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::write(&a, b"hello").unwrap();
    std::fs::write(&b, b"").unwrap();
    assert_eq!(LocalFs.read_head(&a, 100).await.unwrap(), b"hello");
    assert!(LocalFs.read_head(&b, 100).await.unwrap().is_empty());
    assert!(LocalFs.read_head(&a, 0).await.unwrap().is_empty());
}

#[tokio::test]
async fn directories_and_missing_files_are_errors_naming_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope");
    let e = LocalFs.read_head(&missing, 10).await.unwrap_err();
    assert_eq!(e.kind, FsErrorKind::NotFound);
    assert_eq!(e.path, missing);
    assert!(LocalFs.read_head(dir.path(), 10).await.is_err());
}
