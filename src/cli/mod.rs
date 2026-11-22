//! Agent-friendly CLI. It only REPORTS: there is no delete command, and this module has no access
//! to the cleanup code. It does not touch the GUI's "grew" baseline unless --save-snapshot is given.

mod report;

use std::path::PathBuf;

pub const HELP: &str = "diskscope - disk usage report (read-only)

USAGE:
  diskscope scan <path> [options]       scan a drive (C:) or a folder and print a report
  diskscope [path]                       open the window (and scan <path> if given)

OPTIONS (scan):
  --json               machine-readable JSON instead of text
  --top <n>            biggest files and folders to list (default 20)
  --recent <age>       files/folders created or modified within <age> (7d, 12h, 2w)
  --least-used <age>   files not accessed for <age> (falls back to modified time when
                       NTFS last-access updates are disabled)
  --candidates         dev caches and leftovers that could be cleaned (suggestions only)
  --grew               folders that grew since the last snapshot of this path
  --save-snapshot      save this scan as the new baseline for --grew / the GUI
  --depth <n>          list every folder down to <n> levels (size, files, newest dates)
  --mft                require the fast NTFS MFT scan (needs administrator; error otherwise)
  --walker             force the directory walker (default: MFT when elevated, else walker)
  --under <path>       restrict --recent / --least-used / --top / --depth to this sub-folder

The CLI never deletes anything. Cleanup only happens in the window, after review and
explicit confirmation.";

pub struct Options {
    pub path: PathBuf,
    pub json: bool,
    pub top: usize,
    pub recent: Option<u32>,
    pub least_used: Option<u32>,
    pub candidates: bool,
    pub grew: bool,
    pub save_snapshot: bool,
    pub under: Option<PathBuf>,
    pub depth: Option<usize>,
    pub method: crate::scan::Method,
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut it = args.iter();
    let mut o = Options {
        path: PathBuf::new(),
        json: false,
        top: 20,
        recent: None,
        least_used: None,
        candidates: false,
        grew: false,
        save_snapshot: false,
        under: None,
        depth: None,
        method: crate::scan::Method::Auto,
    };
    let age = |v: Option<&String>, flag: &str| {
        v.and_then(|s| crate::fmt::parse_age(s)).ok_or(format!("{flag} needs an age like 7d, 12h or 2w"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => o.json = true,
            "--top" => o.top = it.next().and_then(|s| s.parse().ok()).ok_or("--top needs a number")?,
            "--recent" => o.recent = Some(age(it.next(), "--recent")?),
            "--least-used" => o.least_used = Some(age(it.next(), "--least-used")?),
            "--candidates" => o.candidates = true,
            "--grew" => o.grew = true,
            "--save-snapshot" => o.save_snapshot = true,
            "--mft" => o.method = crate::scan::Method::Mft,
            "--walker" => o.method = crate::scan::Method::Walker,
            "--depth" => o.depth = Some(it.next().and_then(|s| s.parse().ok()).ok_or("--depth needs a number")?),
            "--under" => o.under = Some(PathBuf::from(it.next().ok_or("--under needs a path")?)),
            s if s.starts_with("--") => return Err(format!("unknown option {s} (there is no delete option)")),
            s if o.path.as_os_str().is_empty() => o.path = PathBuf::from(s),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    if o.path.as_os_str().is_empty() {
        return Err("scan needs a path, e.g. diskscope scan C: --top 50".into());
    }
    Ok(o)
}

/// Runs `diskscope <args>` in CLI mode; returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") | Some("help") => {
            println!("{HELP}");
            0
        }
        Some("scan") => match parse(&args[1..]) {
            Ok(o) => report::run(&o),
            Err(e) => {
                eprintln!("error: {e}\n\n{HELP}");
                2
            }
        },
        Some(other) => {
            eprintln!("error: unknown command {other}\n\n{HELP}");
            2
        }
    }
}

#[test]
fn parses_options_and_rejects_unknown_ones() {
    let a = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    let o = parse(&a("C: --json --top 50 --recent 7d --candidates")).unwrap();
    assert!(o.json && o.candidates && o.top == 50 && o.recent == Some(7 * 86_400));
    assert!(parse(&a("C: --delete")).is_err());
    assert!(parse(&a("--json")).is_err());
}
