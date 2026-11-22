//! One automatic label per item, assigned while the tree is built (shared by both scan paths).
//! Strong folder labels (Cache, Dev dependencies, Build output, Temp, Logs, Games, VMs, System)
//! are inherited by everything inside; elsewhere files are labelled by extension. Labels only
//! describe: they never select anything, and System items are never suggested for cleanup.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(u8)]
pub enum Label {
    #[default]
    Other = 0,
    Cache,
    DevDeps,
    Build,
    Temp,
    Logs,
    Installers,
    Archives,
    Video,
    Images,
    Audio,
    Documents,
    Games,
    Vms,
    AppData,
    System,
}

pub const ALL: [Label; 16] = [
    Label::Other, Label::Cache, Label::DevDeps, Label::Build, Label::Temp, Label::Logs, Label::Installers, Label::Archives,
    Label::Video, Label::Images, Label::Audio, Label::Documents, Label::Games, Label::Vms, Label::AppData, Label::System,
];

impl Label {
    pub fn from_u8(v: u8) -> Label {
        ALL.get(v as usize).copied().unwrap_or(Label::Other)
    }
    pub fn name(self) -> &'static str {
        match self {
            Label::Other => "Other",
            Label::Cache => "Cache",
            Label::DevDeps => "Dev dependencies",
            Label::Build => "Build output",
            Label::Temp => "Temp",
            Label::Logs => "Logs",
            Label::Installers => "Installers",
            Label::Archives => "Archives",
            Label::Video => "Video",
            Label::Images => "Images",
            Label::Audio => "Audio",
            Label::Documents => "Documents",
            Label::Games => "Games",
            Label::Vms => "VMs / disk images",
            Label::AppData => "App data",
            Label::System => "System",
        }
    }
    pub fn color(self) -> [u8; 3] {
        match self {
            Label::Other => [110, 110, 120],
            Label::Cache => [70, 160, 220],
            Label::DevDeps => [150, 100, 220],
            Label::Build => [220, 140, 50],
            Label::Temp => [90, 190, 170],
            Label::Logs => [160, 160, 90],
            Label::Installers => [200, 90, 160],
            Label::Archives => [190, 120, 90],
            Label::Video => [220, 70, 70],
            Label::Images => [80, 180, 90],
            Label::Audio => [230, 190, 60],
            Label::Documents => [120, 150, 200],
            Label::Games => [240, 110, 40],
            Label::Vms => [130, 90, 200],
            Label::AppData => [100, 130, 150],
            Label::System => [170, 60, 60],
        }
    }
    /// Labels inherited by everything inside a folder that carries them.
    pub fn is_strong(self) -> bool {
        matches!(self, Label::Cache | Label::DevDeps | Label::Build | Label::Temp | Label::Logs | Label::Games | Label::Vms | Label::System)
    }
    pub fn parse(s: &str) -> Option<Label> {
        ALL.iter().copied().find(|l| l.name().eq_ignore_ascii_case(s) || format!("{l:?}").eq_ignore_ascii_case(s))
    }
}

/// Label of a folder named `name` (any case) inside the folder `parent_name` labelled `parent`, with
/// its siblings' names (lowercase) for context such as `Cargo.toml` next to `target`.
/// `depth` 1 = directly at a drive root.
pub fn folder(name: &str, parent_name: &str, parent: Label, siblings: &[String], depth: usize) -> Label {
    if parent.is_strong() {
        return parent;
    }
    let n = name.to_lowercase();
    let has = |s: &str| siblings.iter().any(|x| x == s);
    let has_ext = |e: &str| siblings.iter().any(|x| x.ends_with(e));
    if depth == 1 && (n == "windows" || n.starts_with("program files") || n.starts_with('$') || n == "system volume information" || n == "recovery") {
        return Label::System;
    }
    let pn = parent_name.to_lowercase();
    if depth == 2 && pn == "programdata" && n == "microsoft" {
        return Label::System;
    }
    match n.as_str() {
        "node_modules" | ".pnpm-store" | ".gradle" | ".m2" | ".venv" | "venv" | "site-packages" => Label::DevDeps,
        "registry" | "git" if pn == ".cargo" => Label::DevDeps,
        "pkgs" if has("envs") || has("conda-meta") => Label::DevDeps,
        "store" if pn == "pnpm" => Label::DevDeps,
        "cache" | "caches" | ".cache" | "npm-cache" | "_cacache" | "__pycache__" | "gpucache" | "code cache" | "shadercache" | "inetcache" => Label::Cache,
        "target" if has("cargo.toml") => Label::Build,
        "library" | "builds" if has("assets") && has("projectsettings") => Label::Build,
        "bin" | "obj" if has_ext(".csproj") || has_ext(".sln") || has_ext(".fsproj") => Label::Build,
        "dist" | "build" | ".next" | "out" if has("package.json") => Label::Build,
        "temp" | "tmp" => Label::Temp,
        "logs" | "log" => Label::Logs,
        "steamapps" | "steamlibrary" | "riot games" | "epic games" | "gog galaxy" => Label::Games,
        "appdata" => Label::AppData,
        _ => parent,
    }
}

/// Label of a file inside a folder labelled `parent`.
pub fn file(name: &str, parent: Label) -> Label {
    if parent.is_strong() {
        return parent;
    }
    let n = name.to_lowercase();
    let ext = n.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "msi" | "msix" | "msixbundle" | "appx" => Label::Installers,
        "exe" if n.contains("setup") || n.contains("install") => Label::Installers,
        "iso" => Label::Installers,
        "zip" | "7z" | "rar" | "gz" | "tgz" | "tar" | "xz" | "bz2" | "zst" => Label::Archives,
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "wmv" | "flv" | "m4v" => Label::Video,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "psd" | "heic" | "svg" | "raw" | "cr2" | "nef" => Label::Images,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "wma" => Label::Audio,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "csv" | "odt" | "rtf" | "epub" => Label::Documents,
        "vhdx" | "vhd" | "vmdk" | "vdi" | "qcow2" | "avhdx" => Label::Vms,
        "log" | "etl" => Label::Logs,
        "tmp" | "temp" => Label::Temp,
        _ if (n == "pagefile.sys" || n == "hiberfil.sys" || n == "swapfile.sys") => Label::System,
        _ => if parent == Label::AppData { Label::AppData } else { Label::Other },
    }
}

/// Label of a scan root from its full path (so scanning `C:\Windows\Temp` still says System).
pub fn root(path: &std::path::Path) -> Label {
    let (mut label, mut depth, mut parent) = (Label::Other, 0, String::new());
    for c in path.components() {
        if let std::path::Component::Normal(n) = c {
            depth += 1;
            let name = n.to_string_lossy().to_string();
            label = folder(&name, &parent, label, &[], depth);
            parent = name;
        }
    }
    label
}

#[cfg(test)]
#[path = "labels_tests.rs"]
mod tests;
