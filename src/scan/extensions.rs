//! Extension statistics (the right-hand table) and the colour of each extension in the treemap.

use super::aggregate::for_each_below;
use crate::model::Tree;

/// Lowercase extension with its dot (`.png`); `None` for no extension or junk like `file.3a8b9c0d1e2f3a4b5`.
pub fn ext_of(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    let ext = &name[dot..];
    if dot == 0 && !name[1..].contains('.') {
        return None; // ".gitignore" style names count as "no extension"
    }
    if ext.len() < 2 || ext.len() > 12 || ext.contains(' ') {
        return None;
    }
    Some(ext.to_lowercase())
}

#[derive(Clone, Debug)]
pub struct ExtStat {
    pub ext: u16,
    pub size: u64,
    pub alloc: u64,
    pub files: u64,
}

/// Per-extension totals under `root`, biggest first.
pub fn ext_stats(tree: &Tree, root: u32) -> Vec<ExtStat> {
    let mut acc: Vec<ExtStat> =
        (0..tree.exts.len()).map(|i| ExtStat { ext: i as u16, size: 0, alloc: 0, files: 0 }).collect();
    for_each_below(tree, root, |i| {
        let n = tree.node(i);
        if !n.is_dir() {
            let s = &mut acc[n.ext as usize];
            s.size += n.size;
            s.alloc += n.alloc;
            s.files += n.files as u64;
        }
    });
    acc.retain(|s| s.files > 0);
    acc.sort_by(|a, b| b.size.cmp(&a.size));
    acc
}

/// Distinct colours for the biggest extensions, muted hashed colours for the rest.
const RANKED: [[u8; 3]; 24] = [
    [214, 118, 32], [194, 194, 24], [52, 168, 72], [140, 76, 214], [200, 60, 140], [44, 140, 230],
    [220, 52, 52], [32, 176, 170], [236, 160, 64], [120, 190, 40], [90, 110, 230], [230, 90, 190],
    [160, 110, 60], [60, 200, 120], [170, 170, 220], [240, 210, 90], [110, 60, 160], [40, 120, 110],
    [200, 140, 140], [120, 160, 200], [180, 90, 40], [100, 140, 60], [150, 40, 90], [70, 90, 140],
];
pub const DIR_COLOR: [u8; 3] = [72, 72, 80];

pub fn palette(tree: &Tree, stats: &[ExtStat]) -> Vec<[u8; 3]> {
    let mut colors: Vec<[u8; 3]> = tree.exts.iter().map(|e| hashed(e)).collect();
    for (rank, s) in stats.iter().take(RANKED.len()).enumerate() {
        colors[s.ext as usize] = RANKED[rank];
    }
    colors
}

fn hashed(ext: &str) -> [u8; 3] {
    let h = ext.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    [90 + (h & 0x5f) as u8, 90 + ((h >> 8) & 0x5f) as u8, 90 + ((h >> 16) & 0x5f) as u8]
}

/// Short human file type for the table.
pub fn type_name(ext: &str) -> String {
    let known = match ext {
        "" => "(No extension)",
        ".dll" => "Application extension",
        ".exe" => "Application",
        ".sys" => "System file",
        ".msi" => "Windows Installer package",
        ".vhdx" | ".vhd" => "Hard disk image",
        ".iso" => "Disc image",
        ".png" => "PNG image",
        ".jpg" | ".jpeg" => "JPEG image",
        ".webp" => "WebP image",
        ".psd" => "Photoshop image",
        ".mp4" | ".mov" | ".mkv" | ".webm" => "Video",
        ".mp3" | ".wav" | ".ogg" | ".m4a" => "Audio",
        ".zip" | ".7z" | ".rar" | ".gz" | ".tar" => "Archive",
        ".db" | ".sqlite" | ".ldb" => "Database file",
        ".log" => "Log file",
        ".json" => "JSON file",
        ".jsonl" => "JSON lines file",
        ".js" | ".mjs" | ".cjs" => "JavaScript",
        ".ts" | ".tsx" => "TypeScript",
        ".rs" => "Rust source",
        ".cs" => "C# source",
        ".lib" | ".a" | ".rlib" => "Object library",
        ".obj" | ".o" => "Object file",
        ".pdb" => "Debug symbols",
        ".bin" => "Binary file",
        ".dat" => "Data file",
        ".safetensors" | ".gguf" | ".onnx" | ".ckpt" => "AI model weights",
        ".glb" | ".gltf" | ".fbx" => "3D model",
        ".apk" | ".aab" => "Android package",
        ".pack" => "Git pack",
        ".asset" | ".prefab" | ".unity" => "Unity asset",
        ".tmp" | ".temp" => "Temporary file",
        ".cab" => "Cabinet archive",
        ".etl" => "Event trace log",
        ".pak" => "Package file",
        _ => "",
    };
    if known.is_empty() {
        format!("{} file", ext.trim_start_matches('.').to_uppercase())
    } else {
        known.to_string()
    }
}

#[test]
fn extension_rules() {
    assert_eq!(ext_of("photo.PNG").as_deref(), Some(".png"));
    assert_eq!(ext_of("archive.tar.gz").as_deref(), Some(".gz"));
    assert_eq!(ext_of(".gitignore"), None);
    assert_eq!(ext_of("README"), None);
    assert_eq!(ext_of("blob.3a8b9c0d1e2f3a4b5c"), None);
    assert_eq!(type_name(".xyz"), "XYZ file");
}
