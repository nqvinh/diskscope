//! Hard blocklist: paths that can never enter the review list or be deleted, whatever the user
//! ticks. Checked when an item is added AND again right before each delete.

use std::path::Path;

/// Folders directly inside a user profile that stay protected themselves (their contents are not).
const PROFILE_FOLDERS: &[&str] = &[
    "appdata", "desktop", "documents", "downloads", "pictures", "videos", "music", "onedrive",
    "favorites", "contacts", "links", "saved games", "searches", ".ssh",
];
const APPDATA_ROOTS: &[&str] = &["local", "roaming", "locallow"];
const ROOT_SYSTEM_ITEMS: &[&str] = &[
    "pagefile.sys", "hiberfil.sys", "swapfile.sys", "dumpstack.log.tmp", "bootmgr", "system volume information",
    "$recycle.bin", "recovery", "boot", "efi", "config.msi", "$windows.~bt", "$windows.~ws", "$winreagent",
];

/// Lowercase, backslash-separated, without `\\?\` prefix or trailing separators.
pub fn normalize(path: &Path) -> String {
    let s = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let s = s.strip_prefix("\\\\?\\").unwrap_or(&s);
    s.trim_end_matches('\\').to_string()
}

/// Why `path` is protected, or `None` when it may be reviewed for deletion.
pub fn blocked_reason(path: &Path) -> Option<&'static str> {
    let p = normalize(path);
    let parts: Vec<&str> = p.split('\\').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() || p.starts_with("\\\\") {
        return Some("not a local drive path");
    }
    if parts.len() == 1 {
        return Some("drive root");
    }
    if parts.iter().any(|s| *s == "..") {
        return Some("relative path segments");
    }
    let top = parts[1];
    if top == "windows" || system_root_contains(&p) {
        return Some("Windows system folder");
    }
    if top.starts_with("program files") {
        return Some("installed programs (Program Files)");
    }
    if top == "programdata" && (parts.len() == 2 || parts[2] == "microsoft") {
        return Some("ProgramData / ProgramData\\Microsoft");
    }
    if ROOT_SYSTEM_ITEMS.contains(&top) || (parts.len() == 2 && top.starts_with('$')) {
        return Some("system file or folder at the drive root");
    }
    if top == "users" {
        match parts.len() {
            2 => return Some("Users folder"),
            3 => return Some("a user profile root"),
            4 if PROFILE_FOLDERS.contains(&parts[3]) || parts[3].starts_with("ntuser.") => {
                return Some("a main folder of a user profile")
            }
            5 if parts[3] == "appdata" && APPDATA_ROOTS.contains(&parts[4]) => return Some("AppData root"),
            _ => {}
        }
    }
    None
}

fn system_root_contains(p: &str) -> bool {
    let Ok(root) = std::env::var("SystemRoot") else { return false };
    let root = normalize(Path::new(&root));
    !root.is_empty() && (p == root || p.starts_with(&format!("{root}\\")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(p: &str) -> bool {
        blocked_reason(Path::new(p)).is_some()
    }

    #[test]
    fn protects_system_and_profile_roots() {
        for p in [
            "C:\\", "C:", "d:/", "C:\\Windows", "C:\\Windows\\System32\\drivers", "C:\\Program Files",
            "C:\\Program Files (x86)\\Steam", "C:\\ProgramData", "C:\\ProgramData\\Microsoft\\Windows",
            "C:\\pagefile.sys", "D:\\hiberfil.sys", "C:\\swapfile.sys", "C:\\$Recycle.Bin",
            "C:\\System Volume Information", "C:\\Users", "C:\\Users\\alice", "C:\\Users\\alice\\",
            "C:\\Users\\alice\\AppData", "C:\\Users\\alice\\AppData\\Local", "C:\\Users\\alice\\AppData\\Roaming\\",
            "C:\\Users\\alice\\Documents", "C:\\Users\\alice\\NTUSER.DAT", "\\\\?\\C:\\Windows", "\\\\server\\share\\x",
            "C:\\Users\\alice\\..\\..\\Windows", "C:\\$MFT", "D:\\$Extend",
        ] {
            assert!(blocked(p), "{p} should be blocked");
        }
    }

    #[test]
    fn allows_ordinary_folders() {
        for p in [
            "C:\\Users\\alice\\AppData\\Local\\CapCut\\Apps\\5.8.0.2216",
            "C:\\Users\\alice\\AppData\\Local\\Temp\\x",
            "C:\\Users\\alice\\dev\\proj\\node_modules",
            "D:\\tmp\\build",
            "C:\\ProgramData\\SomeVendor\\cache",
            "E:\\games\\MyGame\\Library\\Bee",
        ] {
            assert!(!blocked(p), "{p} should be allowed");
        }
    }
}
