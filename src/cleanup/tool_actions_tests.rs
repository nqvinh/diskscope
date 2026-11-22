//! Tool-aware cleanup: parsing, command building, path matching, PATH lookup and the runner.
//! No real developer tool is started: the runner tests use `cmd /c` as a stand-in.

use super::*;
use crate::cleanup::tool_probe::find_in;
use crate::cleanup::tool_runner::{run, End};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn info(tool: Tool, caches: &[&str]) -> ToolInfo {
    ToolInfo { tool, program: PathBuf::from("x"), prefix: Vec::new(), caches: caches.iter().map(PathBuf::from).collect(), reclaimable: None, note: String::new() }
}

#[test]
fn parses_single_path_probes() {
    let (c, _) = parse_probe(Tool::Pnpm, "C:\\Users\\a\\AppData\\Local\\pnpm\\store\\v10\r\n").unwrap();
    assert_eq!(c, [PathBuf::from("C:\\Users\\a\\AppData\\Local\\pnpm\\store\\v10")]);
    let (c, _) = parse_probe(Tool::Npm, "npm warn something\nD:\\npm-cache\n").unwrap();
    assert_eq!(c, [PathBuf::from("D:\\npm-cache")], "skips non-path lines");
    let (c, _) = parse_probe(Tool::Pip, "c:\\users\\a\\appdata\\local\\pip\\cache\n").unwrap();
    assert_eq!(c, [PathBuf::from("c:\\users\\a\\appdata\\local\\pip\\cache")]);
    assert!(parse_probe(Tool::Yarn, "error: not found\n").is_err());
}

#[test]
fn nuget_lists_global_packages_first() {
    let out = "http-cache: C:\\Users\\a\\AppData\\Local\\NuGet\\v3-cache\nglobal-packages: C:\\Users\\a\\.nuget\\packages\\\ntemp: C:\\Users\\a\\AppData\\Local\\Temp\\NuGetScratch\nplugins-cache: C:\\Users\\a\\AppData\\Local\\NuGet\\plugins-cache\n";
    let (c, _) = parse_probe(Tool::Nuget, out).unwrap();
    assert_eq!(c.len(), 4);
    assert_eq!(c[0], PathBuf::from("C:\\Users\\a\\.nuget\\packages"), "primary first, trailing \\ dropped");
}

#[test]
fn docker_build_cache_and_sizes() {
    let out = "Images\t1.2GB\t800MB (66%)\nContainers\t10MB\t10MB (100%)\nLocal Volumes\t500MB\t0B (0%)\nBuild Cache\t2.3GB\t2.1GB\n";
    let (c, r) = parse_probe(Tool::Docker, out).unwrap();
    assert!(c.is_empty());
    assert_eq!(r, Some(2_100_000_000));
    assert!(parse_probe(Tool::Docker, "").is_err(), "daemon not running -> no table");
    assert_eq!(parse_docker_size("0B"), Some(0));
    assert_eq!(parse_docker_size("1.5kB"), Some(1500));
    assert_eq!(parse_docker_size("800MB (66%)"), Some(800_000_000));
    assert_eq!(parse_docker_size("lots"), None);
}

#[test]
fn builds_exact_commands() {
    let mut i = info(Tool::Pnpm, &["C:\\s"]);
    assert_eq!(i.command_line(), "pnpm store prune");
    assert_eq!(i.clean_args(), ["store", "prune"]);
    assert_eq!(command_line(Tool::Npm, &[]), "npm cache clean --force");
    assert_eq!(command_line(Tool::Nuget, &[]), "dotnet nuget locals all --clear");
    assert_eq!(command_line(Tool::Docker, &[]), "docker builder prune -f");
    i.tool = Tool::Pip;
    i.prefix = vec!["-m".into(), "pip".into()];
    assert_eq!(i.clean_args(), ["-m", "pip", "cache", "purge"], "py launcher keeps its leading args");
    assert!(!info(Tool::Cargo, &[]).can_clean(), "cargo only gets a hint");
    let mut broken = info(Tool::Docker, &[]);
    broken.note = "not running".into();
    assert!(!broken.can_clean());
}

#[test]
fn matches_only_the_primary_cache_or_one_version_level() {
    let infos = vec![
        info(Tool::Pnpm, &["C:\\Users\\a\\AppData\\Local\\pnpm\\store\\v10"]),
        info(Tool::Nuget, &["C:\\Users\\a\\.nuget\\packages", "C:\\Users\\a\\AppData\\Local\\Temp\\NuGetScratch"]),
        info(Tool::Npm, &["D:\\npm-cache"]),
    ];
    let hit = |p: &str| tool_for_path(&infos, Path::new(p)).map(|i| i.tool);
    assert_eq!(hit("C:\\Users\\a\\AppData\\Local\\pnpm\\store"), Some(Tool::Pnpm), "store holds store\\v10");
    assert_eq!(hit("c:\\users\\a\\appdata\\local\\pnpm\\store\\v10\\"), Some(Tool::Pnpm), "case and trailing slash");
    assert_eq!(hit("C:\\Users\\a\\.nuget\\packages"), Some(Tool::Nuget));
    assert_eq!(hit("D:\\npm-cache"), Some(Tool::Npm));
    assert_eq!(hit("C:\\Users\\a\\AppData\\Local\\Temp"), None, "Temp holds NuGetScratch, but not as the primary cache");
    assert_eq!(hit("C:\\Users\\a\\AppData\\Local\\pnpm"), None, "two levels up is too far");
    assert_eq!(hit("D:\\npm-cache\\_cacache"), None, "a sub-folder of the cache is not the cache");
    assert_eq!(hit("D:\\other\\npm-cache"), None);
}

#[test]
fn which_tries_pathext_in_order() {
    let dir = std::env::temp_dir().join(format!("diskscope-which-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("faketool.cmd"), "@echo off").unwrap();
    let found = find_in([dir.clone()].into_iter(), "faketool", ".COM;.EXE;.BAT;.CMD");
    assert_eq!(found, Some(dir.join("faketool.cmd")));
    assert_eq!(find_in([dir.clone()].into_iter(), "missing", ".EXE;.CMD"), None);
    std::fs::remove_dir_all(&dir).ok();
}

fn cmd_info(args: &[&str]) -> ToolInfo {
    let program = PathBuf::from(std::env::var("ComSpec").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".into()));
    let mut i = info(Tool::Docker, &[]); // Docker = no cache folder: the drive falls back to the profile's
    i.program = program;
    i.prefix = args.iter().map(|s| s.to_string()).collect();
    i
}

/// `clean_args` appends Docker's clean args; `rem` turns them into a comment for cmd.
fn runs(args: &[&str], timeout: u64, cancel: bool) -> (End, Vec<String>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let flag = AtomicBool::new(cancel);
    let r = run(&cmd_info(args), "test", &lines, &flag, Duration::from_secs(timeout));
    assert!(r.free_before.is_some() && r.free_after.is_some(), "free space measured on {}", r.drive);
    let v = lines.lock().unwrap().clone();
    (r.end, v)
}

#[test]
fn runner_captures_output_and_exit_codes() {
    let (end, lines) = runs(&["/c", "echo hello-from-runner & rem"], 30, false);
    assert_eq!(end, End::Ok);
    assert!(lines.iter().any(|l| l.contains("hello-from-runner")), "{lines:?}");
    let (end, _) = runs(&["/c", "exit 3 & rem"], 30, false);
    assert_eq!(end, End::Failed("exit code 3".into()));
}

#[test]
fn runner_stops_on_cancel_and_timeout() {
    let (end, _) = runs(&["/c", "ping -n 30 127.0.0.1 >nul & rem"], 60, true);
    assert_eq!(end, End::Cancelled);
    let started = std::time::Instant::now();
    let (end, _) = runs(&["/c", "ping -n 30 127.0.0.1 >nul & rem"], 1, false);
    assert_eq!(end, End::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(15), "the process tree was killed");
}
