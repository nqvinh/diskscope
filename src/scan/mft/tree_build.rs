//! Turns the parsed MFT table (one `Rec` per record number) into the same arena tree the walker
//! builds, breadth-first from the volume root (record 5) or from a sub-folder of it, through the
//! shared `Builder` (same small-file grouping, reparse points never descended). Folders are
//! prepared in parallel slices and committed in BFS order.

use super::record::{Rec, R_DIR, R_HAS_SI, R_IN_USE};
use crate::model::Tree;
use crate::rules::labels::Label;
use crate::scan::prepare::{prepare, Ctx};
use crate::scan::{builder::Builder, entry::RawEntry, profile, Progress};
use rayon::prelude::*;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

pub const ROOT_RECORD: u32 = 5;
/// Folders prepared in parallel per step (bounds the memory of prepared batches).
const SLICE: usize = 4096;

pub struct Table {
    pub recs: Vec<Rec>,
    pub names: Vec<String>,
}

impl Table {
    fn name(&self, r: &Rec) -> &str {
        let s = &self.names[r.name_chunk as usize];
        s.get(r.name_off as usize..r.name_off as usize + r.name_len as usize).unwrap_or("")
    }

    /// A live base record with a usable name.
    fn live(&self, i: usize) -> bool {
        let r = &self.recs[i];
        r.flags & (R_IN_USE | R_HAS_SI) == (R_IN_USE | R_HAS_SI) && r.name_rank > 0
    }

    fn entry(&self, i: u32) -> RawEntry {
        let r = &self.recs[i as usize];
        let dir = r.flags & R_DIR != 0;
        RawEntry {
            name: self.name(r).to_string(),
            attrs: if dir { r.attrs | 0x10 } else { r.attrs & !0x10 },
            size: if dir { 0 } else { r.size },
            alloc: if dir { 0 } else { r.alloc },
            created: r.created,
            modified: r.modified,
            accessed: r.accessed,
            id: i as u64,
        }
    }
}

/// Children lists by parent record number (counting sort; two flat arrays).
fn children(t: &Table) -> (Vec<u32>, Vec<u32>) {
    let n = t.recs.len();
    let mut offs = vec![0u32; n + 1];
    let ok = |i: usize| i != ROOT_RECORD as usize && t.live(i) && (t.recs[i].parent as usize) < n && t.recs[i].parent as usize != i;
    for i in 0..n {
        if ok(i) {
            offs[t.recs[i].parent as usize + 1] += 1;
        }
    }
    for i in 0..n {
        offs[i + 1] += offs[i];
    }
    let mut kids = vec![0u32; offs[n] as usize];
    let mut cur = offs.clone();
    for i in 0..n {
        if ok(i) {
            let p = t.recs[i].parent as usize;
            kids[cur[p] as usize] = i as u32;
            cur[p] += 1;
        }
    }
    (offs, kids)
}

/// Builds the tree for `root` = drive root joined with `rel` (empty for the whole volume).
pub fn build(t: &Table, rel: &Path, root: PathBuf, cluster: u64, progress: &Progress, start: Instant) -> Result<Tree, String> {
    build_with(t, rel, root, cluster, progress, start, profile::knob("DISKSCOPE_PAR_BUILD", 1) != 0)
}

/// `build` with the folder preparation in parallel (`par`) or serial; both give the same tree.
fn build_with(t: &Table, rel: &Path, root: PathBuf, cluster: u64, progress: &Progress, start: Instant, par: bool) -> Result<Tree, String> {
    if t.recs.len() <= ROOT_RECORD as usize {
        return Err("MFT too small".into());
    }
    let (offs, kids) = profile::time("mft children index", || children(t));
    let t_build = Instant::now();
    let list = |r: u32| &kids[offs[r as usize] as usize..offs[r as usize + 1] as usize];
    let mut top = ROOT_RECORD;
    for comp in rel.components() {
        let Component::Normal(name) = comp else { continue };
        let want = name.to_string_lossy().to_lowercase();
        top = *list(top)
            .iter()
            .find(|&&k| t.recs[k as usize].flags & R_DIR != 0 && t.name(&t.recs[k as usize]).to_lowercase() == want)
            .ok_or_else(|| format!("{} not found in the MFT", rel.display()))?;
    }
    let root_entry = t.entry(top);
    let mut b = Builder::new(root, cluster, Some(&root_entry), progress.live.get().cloned());
    let root_ctx = b.ctx_of(0);
    // Breadth-first, one level at a time: (record, node, label) of the level's folders. Each slice
    // is prepared in parallel and committed in order, so the tree matches a serial BFS exactly.
    let (mut level, mut depth) = (vec![(top, 0u32, root_ctx.label)], root_ctx.depth);
    while !level.is_empty() {
        let mut next = Vec::new();
        for slice in level.chunks(SLICE) {
            let prep = |&(rec, node, label): &(u32, u32, Label)| {
                let name = if node == 0 { root_ctx.name.clone() } else { t.name(&t.recs[rec as usize]).to_string() };
                let batch: Vec<RawEntry> = list(rec).iter().map(|&k| t.entry(k)).collect();
                prepare(&Ctx { label, name, depth }, batch)
            };
            let prepared: Vec<_> = if par { slice.par_iter().map(prep).collect() } else { slice.iter().map(prep).collect() };
            for (&(_, node, _), p) in slice.iter().zip(prepared) {
                next.extend(b.commit(node, depth, p).into_iter().map(|c| (c.id as u32, c.node, c.ctx.label)));
            }
        }
        (level, depth) = (next, depth + 1);
    }
    profile::report("mft tree build (prepare + commit)", t_build.elapsed());
    Ok(b.finish(progress, start))
}

#[cfg(test)]
#[path = "tree_build_tests.rs"]
mod tests;
