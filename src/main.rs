//! diskscope.exe: the window by default; `diskscope scan ...` prints a read-only report to the
//! console it was started from (see also diskscope-cli.exe for plain console use).
#![windows_subsystem = "windows"]

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("scan" | "-h" | "--help" | "help")) {
        diskscope::sys::attach_parent_console();
        std::process::exit(diskscope::cli::run(&args));
    }
    // `diskscope.exe <path> [--select <item>]`: scan <path>, then select <item> in the tree (nothing is added or deleted).
    let select = args.iter().position(|a| a == "--select").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let tab = args.iter().position(|a| a == "--tab").and_then(|i| args.get(i + 1)).cloned();
    let initial = args.iter().find(|a| !a.starts_with("--") && Some(PathBuf::from(a.as_str())) != select && Some(*a) != tab.as_ref()).map(PathBuf::from);
    if let Err(e) = diskscope::gui::run(initial, select, tab) {
        diskscope::sys::attach_parent_console();
        eprintln!("diskscope: could not open the window: {e}");
        std::process::exit(1);
    }
}
