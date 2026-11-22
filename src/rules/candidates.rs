//! Cleanup SUGGESTIONS (never ticked automatically): the final list over a finished tree, the live
//! list streamed while a scan runs, and their ranking (reclaimable size x confidence). Nested
//! matches are reported once at the top-most folder, except a Unity Library\Bee inside a Library.

use super::ao_sessions::AoSessions;
pub use super::candidate_rules::Confidence;
use super::candidate_rules::{classify, old_versions};
use crate::model::Tree;
use std::path::PathBuf;
use std::sync::Mutex;

const MIN_SIZE: u64 = 1 << 20;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub node: u32,
    pub kind: &'static str,
    pub reason: String,
    pub size: u64,
    /// Newest change of anything inside.
    pub modified: u32,
    pub confidence: Confidence,
}

impl Candidate {
    pub fn score(&self) -> f64 {
        self.size as f64 * self.confidence.weight()
    }
}

/// Best first: reclaimable size weighted by confidence (safe caches outrank risky folders).
pub fn rank<T>(list: &mut [T], score: impl Fn(&T) -> f64) {
    list.sort_by(|a, b| score(b).total_cmp(&score(a)));
}

pub fn detect(tree: &Tree, ao: &AoSessions, now: u32) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut stack = vec![(0u32, false)];
    while let Some((i, inside)) = stack.pop() {
        let mut inside_now = inside;
        if i != 0 {
            if let Some((kind, reason, confidence)) = classify(tree, i, ao, now, inside) {
                push(&mut out, tree, i, kind, reason, confidence);
                inside_now = true;
            }
        }
        let old: Vec<u32> = if inside_now {
            Vec::new()
        } else {
            old_versions(tree, i).into_iter().map(|(c, why)| { push(&mut out, tree, c, "old-app-version", why, Confidence::Likely); c }).collect()
        };
        for c in tree.children(i) {
            let n = tree.node(c);
            if n.is_dir() && !n.is_reparse() {
                stack.push((c, inside_now || old.contains(&c)));
            }
        }
    }
    rank(&mut out, Candidate::score);
    out
}

/// Total bytes of the suggestions without counting nested ones twice (Library\Bee inside Library).
pub fn total_without_overlap(tree: &Tree, list: &[Candidate]) -> u64 {
    list.iter()
        .filter(|c| !list.iter().any(|o| o.node != c.node && tree.is_within(c.node, o.node)))
        .map(|c| c.size)
        .sum()
}

fn push(out: &mut Vec<Candidate>, tree: &Tree, i: u32, kind: &'static str, reason: String, confidence: Confidence) {
    let n = tree.node(i);
    if n.size >= MIN_SIZE {
        out.push(Candidate { node: i, kind, reason, size: n.size, modified: n.modified, confidence });
    }
}

/// A suggestion found while the scan is still running; its size grows as files are added.
#[derive(Clone, Debug)]
pub struct LiveSuggestion {
    pub path: PathBuf,
    pub kind: &'static str,
    pub reason: String,
    pub confidence: Confidence,
    pub bytes: u64,
    pub modified: u32,
}

impl LiveSuggestion {
    pub fn score(&self) -> f64 {
        self.bytes as f64 * self.confidence.weight()
    }
}

/// Shared between the scanner (writes) and the window (reads a ranked copy).
pub struct LiveSink {
    pub ao: AoSessions,
    pub now: u32,
    pub list: Mutex<Vec<LiveSuggestion>>,
}

impl LiveSink {
    pub fn new(ao: AoSessions) -> Self {
        Self { ao, now: crate::fmt::now(), list: Mutex::new(Vec::new()) }
    }

    /// Ranked copy of suggestions of at least `min_bytes` so far.
    pub fn ranked(&self, min_bytes: u64) -> Vec<LiveSuggestion> {
        let mut v: Vec<LiveSuggestion> = self.list.lock().map(|l| l.iter().filter(|s| s.bytes >= min_bytes).cloned().collect()).unwrap_or_default();
        rank(&mut v, LiveSuggestion::score);
        v
    }
}

#[cfg(test)]
#[path = "candidates_tests.rs"]
mod tests;
