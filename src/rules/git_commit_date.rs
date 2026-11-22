//! Date of the last commit (or checkout) of a git repository, read from `.git/logs/HEAD` without
//! running git. A `.git` FILE (worktree or submodule: "gitdir: <path>") is followed. Only the
//! last 4 KiB of the log are read.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The repository's git folder: `<repo>\.git`, or where a `.git` file points.
fn git_dir(repo: &Path) -> Option<PathBuf> {
    let dot = repo.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let p = Path::new(target);
    Some(if p.is_absolute() { p.to_path_buf() } else { repo.join(p) })
}

/// Unix time of the newest entry in a reflog line: `<old> <new> Name <mail> <unix> <tz>\t<msg>`.
fn time_of(line: &str) -> Option<u32> {
    let head = line.split('\t').next()?;
    let after_mail = &head[head.rfind('>')? + 1..];
    after_mail.split_whitespace().next()?.parse().ok()
}

pub fn last_commit(repo: &Path) -> Option<u32> {
    let mut f = std::fs::File::open(git_dir(repo)?.join("logs").join("HEAD")).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(4096))).ok()?;
    let mut tail = String::new();
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    tail.push_str(&String::from_utf8_lossy(&bytes));
    tail.lines().rev().find_map(time_of)
}

#[test]
fn reads_the_last_reflog_entry_and_follows_worktree_files() {
    let base = std::env::temp_dir().join(format!("diskscope-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let logs = base.join("repo").join(".git").join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let z = "0".repeat(40);
    std::fs::write(logs.join("HEAD"), format!(
        "{z} {z} Ann <ann@example.com> 1700000000 +0700\tcommit (initial): first\n{z} {z} Ann <ann@example.com> 1700500000 +0700\tcommit: second\n"
    )).unwrap();
    assert_eq!(last_commit(&base.join("repo")), Some(1_700_500_000));
    let wt = base.join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", base.join("repo").join(".git").display())).unwrap();
    assert_eq!(last_commit(&wt), Some(1_700_500_000), "worktree .git file followed");
    assert_eq!(last_commit(&base), None, "not a repository");
    std::fs::remove_dir_all(&base).ok();
}
