// dired-7qs
//! Local filesystem adapter.

use std::path::{Path, PathBuf};

use dired_domain::{EntryMetadata, FileKind};

use crate::{
    CopyOptions, DirectoryStream, FileSystem, FsError, FsErrorKind, RawDirEntry, RemoveOptions,
    RenameOptions,
};

const BATCH: usize = 1024;

#[derive(Debug, Default, Clone)]
pub struct LocalFs;

fn kind_of(ft: &std::fs::FileType) -> FileKind {
    #[cfg(unix)]
    use std::os::unix::fs::FileTypeExt;
    if ft.is_symlink() {
        return FileKind::Symlink;
    }
    if ft.is_dir() {
        return FileKind::Dir;
    }
    if ft.is_file() {
        return FileKind::File;
    }
    #[cfg(unix)]
    {
        if ft.is_fifo() {
            return FileKind::Fifo;
        }
        if ft.is_socket() {
            return FileKind::Socket;
        }
        if ft.is_block_device() {
            return FileKind::BlockDevice;
        }
        if ft.is_char_device() {
            return FileKind::CharDevice;
        }
    }
    FileKind::Unknown
}

pub(crate) fn stat(path: &Path, follow: bool) -> Result<EntryMetadata, FsError> {
    let md = if follow {
        std::fs::metadata(path)
    } else {
        std::fs::symlink_metadata(path)
    }
    .map_err(|e| FsError::from_io("stat", path, &e))?;
    let kind = kind_of(&md.file_type());
    #[cfg(unix)]
    let (mode, dev, ino) = {
        use std::os::unix::fs::MetadataExt;
        (Some(md.mode()), Some(md.dev()), Some(md.ino()))
    };
    #[cfg(not(unix))]
    let (mode, dev, ino) = (None, None, None);
    let (symlink_target, target_is_dir) = if kind == FileKind::Symlink {
        (
            std::fs::read_link(path).ok(),
            std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false),
        )
    } else {
        (None, false)
    };
    Ok(EntryMetadata {
        kind,
        size: md.len(),
        modified: md.modified().ok(),
        mode,
        readonly: md.permissions().readonly(),
        dev,
        ino,
        symlink_target,
        target_is_dir,
    })
}

fn copy_rec(from: &Path, to: &Path, opts: CopyOptions) -> Result<(), FsError> {
    copy_inner(from, to, opts, None, 0)
}

const MAX_COPY_DEPTH: usize = 512;

/// Recursive copy. Directory listings are snapshotted before the target is
/// created, and the target directory itself is never descended into, so a
/// destination that aliases the source cannot recurse forever.
fn copy_inner(from: &Path, to: &Path, opts: CopyOptions, root_target: Option<(u64, u64)>, depth: usize) -> Result<(), FsError> {
    if depth > MAX_COPY_DEPTH {
        return Err(FsError::new("copy", from, FsErrorKind::Other, "directory nesting too deep (possible cycle)"));
    }
    let md = if opts.follow_symlinks {
        std::fs::metadata(from)
    } else {
        std::fs::symlink_metadata(from)
    }
    .map_err(|e| FsError::from_io("copy", from, &e))?;
    if !opts.overwrite && std::fs::symlink_metadata(to).is_ok() {
        return Err(FsError::new("copy", to, FsErrorKind::AlreadyExists, "destination exists"));
    }
    let ft = md.file_type();
    if ft.is_symlink() {
        let target = std::fs::read_link(from).map_err(|e| FsError::from_io("readlink", from, &e))?;
        if opts.overwrite {
            let _ = std::fs::remove_file(to);
        }
        #[cfg(unix)]
        return std::os::unix::fs::symlink(&target, to).map_err(|e| FsError::from_io("symlink", to, &e));
        #[cfg(not(unix))]
        return Err(FsError::new("symlink", to, FsErrorKind::Unsupported, format!("symlinks unsupported ({})", target.display())));
    }
    if ft.is_dir() {
        if !opts.recursive {
            return Err(FsError::new("copy", from, FsErrorKind::Other, "is a directory"));
        }
        let children: Vec<std::fs::DirEntry> = std::fs::read_dir(from)
            .map_err(|e| FsError::from_io("read_dir", from, &e))?
            .collect::<Result<_, _>>()
            .map_err(|e| FsError::from_io("read_dir", from, &e))?;
        match std::fs::create_dir(to) {
            Ok(()) => {}
            Err(e) if opts.overwrite && e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(FsError::from_io("mkdir", to, &e)),
        }
        let root_target = root_target.or_else(|| stat(to, false).ok().and_then(|m| Some((m.dev?, m.ino?))));
        for ent in children {
            let child = ent.path();
            if let (Some(rt), Ok(m)) = (root_target, stat(&child, false)) {
                if (m.dev, m.ino) == (Some(rt.0), Some(rt.1)) {
                    continue; // the copy's own target inside the source
                }
            }
            copy_inner(&child, &to.join(ent.file_name()), opts, root_target, depth + 1)?;
        }
        std::fs::set_permissions(to, md.permissions()).map_err(|e| FsError::from_io("chmod", to, &e))?;
        return Ok(());
    }
    if !ft.is_file() {
        // Opening a FIFO would block; devices and sockets can't be copied as data.
        return Err(FsError::new("copy", from, FsErrorKind::Unsupported, "special file (fifo/socket/device) not copied"));
    }
    std::fs::copy(from, to).map_err(|e| FsError::from_io("copy", from, &e))?;
    Ok(())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, FsError> + Send + 'static,
) -> Result<T, FsError> {
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|e| Err(FsError::new("task", Path::new(""), FsErrorKind::Other, e.to_string())))
}

#[async_trait::async_trait]
impl FileSystem for LocalFs {
    async fn read_dir(&self, path: &Path) -> Result<DirectoryStream, FsError> {
        let path = path.to_path_buf();
        let rd = {
            let p = path.clone();
            blocking(move || std::fs::read_dir(&p).map_err(|e| FsError::from_io("read_dir", &p, &e))).await?
        };
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        tokio::task::spawn_blocking(move || {
            let mut batch = Vec::with_capacity(BATCH);
            for ent in rd {
                match ent {
                    Ok(ent) => {
                        let kind = ent.file_type().map(|ft| kind_of(&ft)).unwrap_or(FileKind::Unknown);
                        batch.push(RawDirEntry {
                            name: ent.file_name(),
                            kind,
                        });
                        if batch.len() >= BATCH
                            && tx.blocking_send(Ok(std::mem::take(&mut batch))).is_err()
                        {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.blocking_send(Err(FsError::from_io("read_dir", &path, &e)));
                    }
                }
            }
            if !batch.is_empty() {
                let _ = tx.blocking_send(Ok(batch));
            }
        });
        Ok(rx)
    }

    async fn metadata(&self, path: &Path, follow_symlinks: bool) -> Result<EntryMetadata, FsError> {
        let p = path.to_path_buf();
        blocking(move || stat(&p, follow_symlinks)).await
    }

    // dired-6xv
    async fn read_head(&self, path: &Path, max: usize) -> Result<Vec<u8>, FsError> {
        use std::io::Read;
        let p = path.to_path_buf();
        blocking(move || {
            let f = std::fs::File::open(&p).map_err(|e| FsError::from_io("read", &p, &e))?;
            let mut buf = Vec::new();
            f.take(max as u64).read_to_end(&mut buf).map_err(|e| FsError::from_io("read", &p, &e))?;
            Ok(buf)
        })
        .await
    }

    async fn copy(&self, from: &Path, to: &Path, options: CopyOptions) -> Result<(), FsError> {
        let (f, t) = (from.to_path_buf(), to.to_path_buf());
        blocking(move || copy_rec(&f, &t, options)).await
    }

    async fn rename(&self, from: &Path, to: &Path, options: RenameOptions) -> Result<(), FsError> {
        let (f, t): (PathBuf, PathBuf) = (from.to_path_buf(), to.to_path_buf());
        blocking(move || {
            if !options.overwrite && std::fs::symlink_metadata(&t).is_ok() {
                // Allow case-only renames where `t` resolves to `f` itself.
                let same = match (stat(&f, false), stat(&t, false)) {
                    (Ok(a), Ok(b)) => a.ino.is_some() && a.ino == b.ino && a.dev == b.dev,
                    _ => false,
                };
                if !same {
                    return Err(FsError::new("rename", &t, FsErrorKind::AlreadyExists, "destination exists"));
                }
            }
            std::fs::rename(&f, &t).map_err(|e| FsError::from_io("rename", &f, &e))
        })
        .await
    }

    async fn remove(&self, path: &Path, options: RemoveOptions) -> Result<(), FsError> {
        let p = path.to_path_buf();
        blocking(move || {
            let md = std::fs::symlink_metadata(&p).map_err(|e| FsError::from_io("remove", &p, &e))?;
            let r = if md.is_dir() {
                if options.recursive {
                    // remove_dir_all does not follow symlinks.
                    std::fs::remove_dir_all(&p)
                } else {
                    std::fs::remove_dir(&p)
                }
            } else {
                std::fs::remove_file(&p)
            };
            r.map_err(|e| FsError::from_io("remove", &p, &e))
        })
        .await
    }

    async fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        let p = path.to_path_buf();
        blocking(move || std::fs::create_dir(&p).map_err(|e| FsError::from_io("mkdir", &p, &e))).await
    }

    async fn canonicalize(&self, path: &Path) -> Result<PathBuf, FsError> {
        let p = path.to_path_buf();
        blocking(move || std::fs::canonicalize(&p).map_err(|e| FsError::from_io("canonicalize", &p, &e))).await
    }

    async fn set_permissions(&self, path: &Path, mode: u32) -> Result<(), FsError> {
        let p = path.to_path_buf();
        blocking(move || {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode))
                    .map_err(|e| FsError::from_io("chmod", &p, &e))
            }
            #[cfg(not(unix))]
            {
                let _ = mode;
                Err(FsError::new("chmod", &p, FsErrorKind::Unsupported, "Unix modes unsupported"))
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_dir_and_copy_tree() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        std::fs::create_dir(root.join("d")).unwrap();
        std::fs::write(root.join("d/f"), "x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("f", root.join("d/l")).unwrap();
        let fs = LocalFs;
        let mut rx = fs.read_dir(root).await.unwrap();
        let mut names = vec![];
        while let Some(b) = rx.recv().await {
            names.extend(b.unwrap().into_iter().map(|e| e.name));
        }
        assert_eq!(names, vec!["d"]);
        fs.copy(&root.join("d"), &root.join("e"), CopyOptions { recursive: true, ..Default::default() })
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(root.join("e/f")).unwrap(), "x");
        #[cfg(unix)]
        assert!(std::fs::symlink_metadata(root.join("e/l")).unwrap().file_type().is_symlink());
        let err = fs
            .copy(&root.join("d"), &root.join("e"), CopyOptions { recursive: true, ..Default::default() })
            .await
            .unwrap_err();
        assert_eq!(err.kind, FsErrorKind::AlreadyExists);
        assert!(err.to_string().contains("e"));
        // Copying a directory into itself through a symlink terminates.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("d"), root.join("ln")).unwrap();
            let r = fs
                .copy(&root.join("d"), &root.join("ln/d"), CopyOptions { recursive: true, ..Default::default() })
                .await;
            assert!(r.is_ok(), "{r:?}");
            assert!(!root.join("d/d/d").exists());
        }
    }
}
