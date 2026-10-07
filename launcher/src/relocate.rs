//! Moving a game's folder: a rename when it stays on the same disk,
//! otherwise a copy (with progress, cancellable) after which the original is
//! removed. The copy lands next to the destination as `<name>.pishop-part` and
//! only takes the final name once complete.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Context, anyhow, bail};

use crate::tr;

#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

/// Bytes under a folder (links not followed).
pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                stack.push(e.path());
            } else if kind.is_file() {
                total += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// The device a path is on (or would be: its closest existing parent's).
fn device(path: &Path) -> Option<u64> {
    path.ancestors().find_map(|p| fs::metadata(p).ok()).map(|m| m.dev())
}

pub fn same_disk(a: &Path, b: &Path) -> bool {
    matches!((device(a), device(b)), (Some(x), Some(y)) if x == y)
}

/// Free bytes where `path` is (or would be).
pub fn free_space(path: &Path) -> Option<u64> {
    path.ancestors().find(|p| p.exists()).and_then(crate::localfs::disk_space).map(|s| s.0)
}

fn canceled() -> anyhow::Error {
    anyhow!(tr!("canceled", "cancelado"))
}

/// Moves the folder `from` to `to`, which must not exist yet (or be empty).
pub fn move_dir(from: &Path, to: &Path, p: &Progress) -> anyhow::Result<()> {
    if to.exists() {
        if fs::read_dir(to).map(|mut d| d.next().is_some()).unwrap_or(true) {
            bail!(tr!("{} already exists", "{} já existe", to.display()));
        }
        fs::remove_dir(to)?;
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).with_context(|| tr!("couldn't create {}", "não foi possível criar {}", parent.display()))?;
    }
    match fs::rename(from, to) {
        Ok(()) => {
            p.done.store(p.total.load(Ordering::Relaxed), Ordering::Relaxed);
            return Ok(());
        }
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {}
        Err(e) => return Err(e).with_context(|| tr!("couldn't move {}", "não foi possível mover {}", from.display())),
    }
    // Another disk: copy, then swap the copy in and drop the original.
    let part = to.with_file_name(format!("{}.pishop-part", to.file_name().unwrap_or_default().to_string_lossy()));
    let _ = fs::remove_dir_all(&part);
    if let Err(e) = copy_tree(from, &part, p) {
        let _ = fs::remove_dir_all(&part);
        return Err(e);
    }
    fs::rename(&part, to)?;
    fs::remove_dir_all(from).with_context(|| {
        tr!(
            "the copy is complete, but the original folder couldn't be removed",
            "a cópia terminou, mas a pasta original não pôde ser removida"
        )
    })?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, p: &Progress) -> anyhow::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        if p.cancel.load(Ordering::Relaxed) {
            return Err(canceled());
        }
        let e = e?;
        let kind = e.file_type()?;
        let dest = to.join(e.file_name());
        if kind.is_dir() {
            copy_tree(&e.path(), &dest, p)?;
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(e.path())?, &dest)?;
        } else if kind.is_file() {
            copy_file(&e.path(), &dest, p).with_context(|| e.path().display().to_string())?;
        }
    }
    if let Ok(m) = fs::metadata(from) {
        let _ = fs::set_permissions(to, m.permissions());
    }
    Ok(())
}

fn copy_file(from: &Path, to: &Path, p: &Progress) -> anyhow::Result<()> {
    let mut src = File::open(from)?;
    let mut dst = File::create(to)?;
    let mut buf = vec![0u8; 4 << 20];
    loop {
        if p.cancel.load(Ordering::Relaxed) {
            return Err(canceled());
        }
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])?;
        p.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    let m = src.metadata()?;
    let _ = dst.set_permissions(m.permissions());
    if let Ok(t) = m.modified() {
        let _ = dst.set_modified(t);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_a_tree_and_counts_it() {
        let base = std::env::temp_dir().join(format!("pishop-move-{}", std::process::id()));
        let from = base.join("Games/My Game");
        fs::create_dir_all(from.join("Bin/Win64")).unwrap();
        fs::write(from.join("game.exe"), vec![1u8; 1000]).unwrap();
        fs::write(from.join("Bin/Win64/x.dll"), vec![2u8; 24]).unwrap();
        assert_eq!(dir_size(&from), 1024);
        let to = base.join("pfx/drive_c/Program Files/My Game");
        let p = Progress::default();
        p.total.store(1024, Ordering::Relaxed);
        move_dir(&from, &to, &p).unwrap();
        assert!(!from.exists());
        assert_eq!(fs::read(to.join("Bin/Win64/x.dll")).unwrap(), vec![2u8; 24]);
        assert_eq!(p.done.load(Ordering::Relaxed), 1024);
        // The destination is taken now.
        fs::create_dir_all(&from).unwrap();
        assert!(move_dir(&from, &to, &p).is_err());
        fs::remove_dir_all(&base).unwrap();
    }
}
