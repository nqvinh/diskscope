//! Console build of the read-only CLI (same commands as `diskscope.exe scan ...`), for scripts and
//! agents that need normal console output and exit codes. `diskscope-cli C: --top 5` = `scan C: --top 5`.

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let explicit = matches!(args.first().map(String::as_str), None | Some("scan" | "-h" | "--help" | "help"));
    if !explicit {
        args.insert(0, "scan".to_string());
    }
    std::process::exit(diskscope::cli::run(&args));
}
