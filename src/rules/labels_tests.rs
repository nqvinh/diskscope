use super::*;
use std::path::Path;

fn sib(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_lowercase()).collect()
}

#[test]
fn folder_rules_on_sample_paths() {
    assert_eq!(folder("Windows", "x", Label::Other, &[], 1), Label::System);
    assert_eq!(folder("Program Files (x86)", "x", Label::Other, &[], 1), Label::System);
    assert_eq!(folder("Windows", "x", Label::Other, &[], 3), Label::Other, "only at a drive root");
    assert_eq!(folder("node_modules", "x", Label::Other, &[], 4), Label::DevDeps);
    assert_eq!(folder("target", "x", Label::Other, &sib(&["Cargo.toml", "src"]), 4), Label::Build);
    assert_eq!(folder("target", "x", Label::Other, &sib(&["notes.txt"]), 4), Label::Other, "target without Cargo.toml");
    assert_eq!(folder("Library", "x", Label::Other, &sib(&["Assets", "ProjectSettings"]), 4), Label::Build);
    assert_eq!(folder("dist", "x", Label::Other, &sib(&["package.json"]), 4), Label::Build);
    assert_eq!(folder("bin", "x", Label::Other, &sib(&["App.csproj"]), 4), Label::Build);
    assert_eq!(folder("npm-cache", "x", Label::Other, &[], 2), Label::Cache);
    assert_eq!(folder("Temp", "x", Label::AppData, &[], 4), Label::Temp);
    assert_eq!(folder("logs", "x", Label::Other, &[], 3), Label::Logs);
    assert_eq!(folder("steamapps", "x", Label::Other, &[], 3), Label::Games);
    assert_eq!(folder("pkgs", "x", Label::Other, &sib(&["envs", "conda-meta"]), 3), Label::DevDeps);
    assert_eq!(folder("anything", "x", Label::Cache, &[], 6), Label::Cache, "strong labels are inherited");
    assert_eq!(folder("Microsoft", "ProgramData", Label::Other, &[], 2), Label::System);
    assert_eq!(folder("store", "pnpm", Label::AppData, &[], 5), Label::DevDeps);
    assert_eq!(folder("registry", ".cargo", Label::Other, &[], 4), Label::DevDeps);
    assert_eq!(folder("Local", "x", Label::AppData, &[], 4), Label::AppData, "weak labels too, unless a rule matches");
}

#[test]
fn file_rules_by_extension_and_inheritance() {
    assert_eq!(file("clip.MP4", Label::Other), Label::Video);
    assert_eq!(file("photo.jpeg", Label::Other), Label::Images);
    assert_eq!(file("song.flac", Label::Other), Label::Audio);
    assert_eq!(file("report.pdf", Label::Other), Label::Documents);
    assert_eq!(file("backup.7z", Label::Other), Label::Archives);
    assert_eq!(file("Tool-Setup-2.1.exe", Label::Other), Label::Installers);
    assert_eq!(file("tool.exe", Label::Other), Label::Other);
    assert_eq!(file("ubuntu.iso", Label::Other), Label::Installers);
    assert_eq!(file("disk.vhdx", Label::Other), Label::Vms);
    assert_eq!(file("app.log", Label::Other), Label::Logs);
    assert_eq!(file("icon.png", Label::DevDeps), Label::DevDeps, "a png inside node_modules is a dependency");
    assert_eq!(file("settings.json", Label::AppData), Label::AppData);
    assert_eq!(file("pagefile.sys", Label::Other), Label::System);
}

#[test]
fn root_label_follows_the_path() {
    assert_eq!(root(Path::new("C:\\Windows\\Temp")), Label::System);
    assert_eq!(root(Path::new("C:\\Users\\alice\\AppData\\Local\\Temp")), Label::Temp);
    assert_eq!(root(Path::new("C:\\Users\\alice\\AppData\\Local")), Label::AppData);
    assert_eq!(root(Path::new("D:\\")), Label::Other);
    assert_eq!(Label::from_u8(Label::Video as u8), Label::Video);
    assert_eq!(Label::parse("dev dependencies"), Some(Label::DevDeps));
}
