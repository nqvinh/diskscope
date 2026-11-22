//! Finds the installed developer tools and asks each one, read-only, where its cache is
//! (`pnpm store path`, `npm config get cache`, `docker system df`, ...). Probes run in parallel
//! threads, never show a console window and are stopped after `PROBE_TIMEOUT_SECS`.

use super::tool_actions::{parse_probe, spec, Tool, ToolInfo, ALL, PROBE_TIMEOUT_SECS};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// CREATE_NO_WINDOW: console tools run without flashing a window.
pub const NO_WINDOW: u32 = 0x0800_0000;

/// Full path of `name` on PATH, trying the PATHEXT extensions (`npm` -> `npm.cmd`).
pub fn which(name: &str) -> Option<PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let path = std::env::var_os("PATH")?;
    find_in(std::env::split_paths(&path), name, &exts)
}

/// `which` over explicit folders (testable without touching the real PATH).
pub fn find_in(dirs: impl Iterator<Item = PathBuf>, name: &str, pathext: &str) -> Option<PathBuf> {
    let exts: Vec<String> = pathext.split(';').filter(|e| !e.is_empty()).map(|e| e.to_lowercase()).collect();
    for dir in dirs {
        for ext in &exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Builds the command for `program` (a .cmd/.bat runs through cmd.exe; Rust escapes the args).
pub fn command(program: &Path, args: &[String]) -> Command {
    let mut c = Command::new(program);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(NO_WINDOW);
    c
}

/// Kills a process and everything it started (npm.cmd -> node.exe) with `taskkill /T /F`.
pub fn kill_tree(child: &mut Child) {
    let _ = Command::new("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(NO_WINDOW).status();
    let _ = child.kill();
}

/// Runs a short read-only command; Ok(stdout) on exit code 0, Err(reason) otherwise.
pub fn run_capture(program: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    let mut child = command(program, args).spawn().map_err(|e| format!("could not start: {e}"))?;
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let t_out = std::thread::spawn(move || read_all(out.as_mut()));
    let t_err = std::thread::spawn(move || read_all(err.as_mut()));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() > timeout => {
                kill_tree(&mut child);
                return Err(format!("no answer within {} s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let (stdout, stderr) = (t_out.join().unwrap_or_default(), t_err.join().unwrap_or_default());
    if status.success() {
        Ok(stdout)
    } else {
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        Err(format!("exit code {}{}", status.code().unwrap_or(-1), if last.is_empty() { String::new() } else { format!(": {last}") }))
    }
}

fn read_all<R: Read>(r: Option<&mut R>) -> String {
    let mut buf = Vec::new();
    if let Some(r) = r {
        let _ = r.read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Probes one tool: `None` when it is not installed at all.
pub fn probe(t: Tool) -> Option<ToolInfo> {
    let s = spec(t);
    let timeout = Duration::from_secs(PROBE_TIMEOUT_SECS);
    let mut first_error = None;
    for launcher in s.launchers {
        let Some(program) = which(launcher[0]) else { continue };
        let prefix: Vec<String> = launcher[1..].iter().map(|a| a.to_string()).collect();
        let mut args = prefix.clone();
        args.extend(s.probe.iter().map(|a| a.to_string()));
        let found = if s.probe.is_empty() { parse_probe(t, "") } else { run_capture(&program, &args, timeout).and_then(|out| parse_probe(t, &out)) };
        match found {
            Ok((caches, reclaimable)) => return Some(ToolInfo { tool: t, program, prefix, caches, reclaimable, note: String::new() }),
            Err(e) => first_error = first_error.or(Some((program, prefix, e))),
        }
    }
    let (program, prefix, e) = first_error?;
    let note = if t == Tool::Docker { format!("installed, but not answering ({e}); start Docker Desktop and check again") } else { format!("installed, but the cache query failed: {e}") };
    Some(ToolInfo { tool: t, program, prefix, caches: Vec::new(), reclaimable: None, note })
}

/// Probe results arriving one tool at a time, so a fast tool (npm) is usable before a slow one
/// (Docker) has answered.
#[derive(Default)]
pub struct Probes {
    /// Installed tools found so far.
    pub found: Mutex<Vec<ToolInfo>>,
    /// Tools whose probe has finished (installed or not).
    pub done: Mutex<Vec<Tool>>,
}

impl Probes {
    pub fn finished(&self) -> bool {
        self.done.lock().map(|d| d.len() >= ALL.len()).unwrap_or(true)
    }
}

/// Starts one probe thread per supported tool.
pub fn start_all() -> Arc<Probes> {
    let probes = Arc::new(Probes::default());
    for t in ALL {
        let p = probes.clone();
        std::thread::spawn(move || {
            let info = probe(t);
            if let (Some(i), Ok(mut f)) = (info, p.found.lock()) {
                f.push(i);
            }
            if let Ok(mut d) = p.done.lock() {
                d.push(t);
            }
        });
    }
    probes
}
