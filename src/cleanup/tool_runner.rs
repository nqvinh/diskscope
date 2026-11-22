//! Runs one confirmed tool cleanup command in the background: output lines are captured for the
//! window, Cancel or the timeout stops the whole process tree, free space of the cache's drive is
//! measured before and after, and the run is written to History (mode "tool") and the action log.
//! Only the confirm dialog creates a `ToolJob`; nothing here starts on its own.

use super::action_log::ActionLog;
use super::history::{History, HistoryEntry};
use super::tool_actions::{ToolInfo, CLEAN_TIMEOUT_SECS};
use super::tool_probe::{command, kill_tree};
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum End {
    Ok,
    Failed(String),
    TimedOut,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct RunResult {
    pub end: End,
    /// Drive measured (`C:\`) and its free bytes before / after.
    pub drive: String,
    pub free_before: Option<u64>,
    pub free_after: Option<u64>,
    pub elapsed: Duration,
}

impl RunResult {
    /// Free-space change on the drive (other programs writing at the same time also count).
    pub fn freed(&self) -> Option<i64> {
        Some(self.free_after? as i64 - self.free_before? as i64)
    }
}

pub struct ToolJob {
    pub command_line: String,
    pub lines: Arc<Mutex<Vec<String>>>,
    pub started: Instant,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<RunResult>>,
}

/// History / action-log result word for an ending.
pub fn result_word(end: &End) -> (&'static str, String) {
    match end {
        End::Ok => ("ok", String::new()),
        End::Failed(e) => ("error", e.clone()),
        End::TimedOut => ("error", format!("stopped after the {} min time limit", CLEAN_TIMEOUT_SECS / 60)),
        End::Cancelled => ("skipped", "cancelled by the user".into()),
    }
}

/// The drive whose free space shows the effect: the cache's drive, else the user profile's.
pub fn drive_of(info: &ToolInfo) -> String {
    let profile = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\".into());
    info.caches.first().and_then(|c| crate::sys::drive_root(c)).or_else(|| crate::sys::drive_root(std::path::Path::new(&profile))).unwrap_or_else(|| "C:\\".into())
}

impl ToolJob {
    pub fn start(info: ToolInfo) -> Self {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        let command_line = info.command_line();
        let (l, c, cmd) = (lines.clone(), cancel.clone(), command_line.clone());
        let handle = std::thread::spawn(move || {
            let r = run(&info, &cmd, &l, &c, Duration::from_secs(CLEAN_TIMEOUT_SECS));
            record(&info, &cmd, &r);
            r
        });
        Self { command_line, lines, started: Instant::now(), cancel, handle: Some(handle) }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Relaxed);
    }

    /// The result once the command has ended (call every frame).
    pub fn poll(&mut self) -> Option<RunResult> {
        if self.handle.as_ref().is_some_and(|h| h.is_finished()) {
            return self.handle.take().and_then(|h| h.join().ok());
        }
        None
    }
}

fn push_lines<R: Read + Send + 'static>(r: Option<R>, lines: Arc<Mutex<Vec<String>>>) -> Option<JoinHandle<()>> {
    let r = r?;
    Some(std::thread::spawn(move || {
        for line in BufReader::new(r).split(b'\n').map_while(Result::ok) {
            let text = String::from_utf8_lossy(&line).trim_end_matches('\r').to_string();
            if let Ok(mut v) = lines.lock() {
                v.push(text);
            }
        }
    }))
}

/// Runs the command and waits for it (no History write: `start` records the result).
pub fn run(info: &ToolInfo, cmd: &str, lines: &Arc<Mutex<Vec<String>>>, cancel: &AtomicBool, timeout: Duration) -> RunResult {
    let drive = drive_of(info);
    let free = || crate::sys::disk_space(&drive).map(|(_, f)| f);
    let free_before = free();
    let started = Instant::now();
    let say = |s: String| { if let Ok(mut v) = lines.lock() { v.push(s) } };
    say(format!("> {cmd}"));
    let end = match command(&info.program, &info.clean_args()).spawn() {
        Err(e) => End::Failed(format!("could not start {}: {e}", info.program.display())),
        Ok(mut child) => {
            let readers = [push_lines(child.stdout.take(), lines.clone()), push_lines(child.stderr.take(), lines.clone())];
            let end = loop {
                match child.try_wait() {
                    Ok(Some(s)) if s.success() => break End::Ok,
                    Ok(Some(s)) => break End::Failed(format!("exit code {}", s.code().unwrap_or(-1))),
                    Ok(None) if cancel.load(Relaxed) => { kill_tree(&mut child); break End::Cancelled }
                    Ok(None) if started.elapsed() > timeout => { kill_tree(&mut child); break End::TimedOut }
                    Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                    Err(e) => break End::Failed(e.to_string()),
                }
            };
            let _ = child.wait();
            readers.into_iter().flatten().for_each(|h| { let _ = h.join(); });
            end
        }
    };
    let free_after = free();
    RunResult { end, drive, free_before, free_after, elapsed: started.elapsed() }
}

fn record(info: &ToolInfo, cmd: &str, r: &RunResult) {
    let (word, detail) = result_word(&r.end);
    let freed = r.freed().unwrap_or(0).max(0) as u64;
    let target = info.caches.first().map(|c| c.display().to_string()).unwrap_or_else(|| format!("{} cache", super::tool_actions::spec(info.tool).name));
    let now = crate::fmt::now();
    let entry = HistoryEntry { at_unix: now, at: crate::fmt::date(now), path: format!("{target} · {cmd}"), size: freed, items: 0, mode: "tool".into(), result: word.into(), error: detail.clone() };
    let _ = History::default_location().append(&[entry], now);
    ActionLog::default_location().record("tool-clean", std::path::Path::new(&target), freed, word, &format!("{cmd}; {detail}"));
}
