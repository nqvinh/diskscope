//! Append-only action log: every delete attempt and every export, as JSON lines and CSV, in
//! %LOCALAPPDATA%\diskscope (actions.jsonl + actions.csv).

use crate::fmt::csv_field;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
pub struct Action<'a> {
    pub at: String,
    pub action: &'a str,
    pub path: String,
    pub size: u64,
    pub result: &'a str,
    pub detail: String,
}

pub struct ActionLog {
    dir: PathBuf,
}

impl ActionLog {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn default_location() -> Self {
        Self::new(crate::sys::data_dir())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn record(&self, action: &str, path: &Path, size: u64, result: &str, detail: &str) {
        let a = Action {
            at: crate::fmt::iso_now(),
            action,
            path: path.display().to_string(),
            size,
            result,
            detail: detail.to_string(),
        };
        if let Err(e) = self.write(&a) {
            eprintln!("diskscope: could not write the action log: {e}");
        }
    }

    fn write(&self, a: &Action) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let open = |name: &str| std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(name));
        let mut j = open("actions.jsonl")?;
        writeln!(j, "{}", serde_json::to_string(a).unwrap_or_default())?;
        let csv_path = self.dir.join("actions.csv");
        let new = !csv_path.exists();
        let mut c = open("actions.csv")?;
        if new {
            writeln!(c, "at,action,path,size,result,detail")?;
        }
        writeln!(
            c,
            "{},{},{},{},{},{}",
            a.at,
            a.action,
            csv_field(&a.path),
            a.size,
            a.result,
            csv_field(&a.detail)
        )
    }
}
