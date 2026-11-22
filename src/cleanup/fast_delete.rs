//! Fast permanent delete of one folder tree: list it once (links are removed as links, never
//! followed), delete the files in parallel (read-only attributes cleared), then remove the
//! folders deepest-first, one depth level at a time in parallel. Cancel is honoured between
//! files; progress counts files and bytes. Long paths work because std adds `\\?\` as needed.

use rayon::prelude::*;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

const REPARSE: u32 = 0x400;
const DIRECTORY: u32 = 0x10;

#[derive(Default)]
pub struct DeleteProgress {
    pub files_done: AtomicU64,
    pub bytes_done: AtomicU64,
    pub items_done: AtomicU64,
    pub cancel: AtomicBool,
}

/// Removes one file, clearing the read-only attribute if that is what blocks it.
pub fn remove_file_force(p: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(p) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let mut perm = std::fs::symlink_metadata(p)?.permissions();
            if perm.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                std::fs::set_permissions(p, perm)?;
                return std::fs::remove_file(p);
            }
            Err(e)
        }
        r => r,
    }
}

struct Listing {
    files: Vec<(PathBuf, u64)>,
    /// (depth, path); links to folders are listed here too and removed with remove_dir.
    dirs: Vec<(usize, PathBuf)>,
}

/// Lists the tree with one rayon task per folder (listing waits on the disk, so it parallelises).
fn list(root: &Path) -> std::io::Result<Listing> {
    let out = Mutex::new(Listing { files: Vec::new(), dirs: vec![(0, root.to_path_buf())] });
    let error: Mutex<Option<std::io::Error>> = Mutex::new(None);
    fn walk<'s>(s: &rayon::Scope<'s>, dir: PathBuf, depth: usize, out: &'s Mutex<Listing>, error: &'s Mutex<Option<std::io::Error>>) {
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => {
                error.lock().expect("error").get_or_insert(e);
                return;
            }
        };
        let (mut files, mut dirs, mut descend) = (Vec::new(), Vec::new(), Vec::new());
        for ent in read.flatten() {
            let Ok(md) = ent.metadata() else { continue };
            let p = ent.path();
            // Attribute bit, not is_dir(): a directory junction reports as a symlink otherwise.
            if md.file_attributes() & DIRECTORY != 0 {
                dirs.push((depth + 1, p.clone()));
                if md.file_attributes() & REPARSE == 0 {
                    descend.push(p); // a junction/symlink is removed, its target never entered
                }
            } else {
                files.push((p, md.file_size()));
            }
        }
        {
            let mut o = out.lock().expect("listing");
            o.files.extend(files);
            o.dirs.extend(dirs);
        }
        for p in descend {
            s.spawn(move |s| walk(s, p, depth + 1, out, error));
        }
    }
    rayon::scope(|s| walk(s, root.to_path_buf(), 0, &out, &error));
    match error.into_inner().expect("error") {
        Some(e) => Err(e),
        None => Ok(out.into_inner().expect("listing")),
    }
}
/// Deletes `root` and everything under it. Err carries the number of failures and the first one.
pub fn remove_tree(root: &Path, progress: &DeleteProgress) -> Result<(), String> {
    // Deleting waits on the disk like scanning does: more requests in flight keep an SSD busy.
    match rayon::ThreadPoolBuilder::new().num_threads(32).build() {
        Ok(pool) => pool.install(|| remove_tree_inner(root, progress)),
        Err(_) => remove_tree_inner(root, progress),
    }
}

fn remove_tree_inner(root: &Path, progress: &DeleteProgress) -> Result<(), String> {
    let l = list(root).map_err(|e| format!("could not list the folder: {e}"))?;
    let errors = Mutex::new((0usize, String::new()));
    let fail = |p: &Path, e: std::io::Error| {
        let mut g = errors.lock().expect("errors");
        if g.0 == 0 {
            g.1 = format!("{}: {}", p.display(), super::execute::explain(&e));
        }
        g.0 += 1;
    };
    l.files.par_iter().for_each(|(p, size)| {
        if progress.cancel.load(Relaxed) {
            return;
        }
        match remove_file_force(p) {
            Ok(()) => {
                progress.files_done.fetch_add(1, Relaxed);
                progress.bytes_done.fetch_add(*size, Relaxed);
            }
            Err(e) => fail(p, e),
        }
    });
    if progress.cancel.load(Relaxed) {
        return Err("cancelled (part of the folder was already deleted)".into());
    }
    let max_depth = l.dirs.iter().map(|d| d.0).max().unwrap_or(0);
    for depth in (0..=max_depth).rev() {
        l.dirs.par_iter().filter(|d| d.0 == depth).for_each(|(_, p)| {
            if let Err(e) = std::fs::remove_dir(p) {
                fail(p, e);
            }
        });
    }
    let (n, first) = errors.into_inner().expect("errors");
    if n == 0 { Ok(()) } else { Err(format!("{n} items could not be deleted, e.g. {first}")) }
}

#[cfg(test)]
#[path = "fast_delete_tests.rs"]
mod tests;
