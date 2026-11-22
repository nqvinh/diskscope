//! Where VM / container disks usually live on this PC: registered WSL distros (HKCU Lxss), Docker
//! Desktop's data disks, "VirtualBox VMs", VMware's "Virtual Machines" and the Hyper-V default
//! folder. Read-only: registry reads and directory listings, two levels deep at most.

use super::vm_disks::is_vm_disk;
use std::path::{Path, PathBuf};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads a REG_SZ value under `key` (a subkey path of `root`).
fn reg_string(root: windows_sys::Win32::System::Registry::HKEY, key: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
    let (k, n) = (wide(key), wide(name));
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let rc = unsafe { RegGetValueW(root, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut len) };
    if rc != 0 {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
}

/// Subkey names of `HKCU\<key>`.
fn subkeys(key: &str) -> Vec<String> {
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ};
    let k = wide(key);
    let mut h: HKEY = std::ptr::null_mut();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_READ, &mut h) } != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0.. {
        let mut name = [0u16; 256];
        let mut len = name.len() as u32;
        let rc = unsafe { RegEnumKeyExW(h, i, name.as_mut_ptr(), &mut len, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        if rc != 0 {
            break;
        }
        out.push(String::from_utf16_lossy(&name[..len as usize]));
    }
    unsafe { RegCloseKey(h) };
    out
}

/// Strips the `\\?\` prefix WSL stores in BasePath.
pub fn clean_base(s: &str) -> PathBuf {
    PathBuf::from(s.strip_prefix("\\\\?\\").unwrap_or(s))
}

/// (distro name, BasePath) of every registered WSL distro.
pub fn wsl_distros() -> Vec<(String, PathBuf)> {
    use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
    const LXSS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Lxss";
    subkeys(LXSS)
        .into_iter()
        .filter_map(|id| {
            let key = format!("{LXSS}\\{id}");
            Some((reg_string(HKEY_CURRENT_USER, &key, "DistributionName")?, clean_base(&reg_string(HKEY_CURRENT_USER, &key, "BasePath")?)))
        })
        .collect()
}

/// VM disk files in `dir` and one level of sub-folders below it.
fn disks_in(dir: &Path, depth: u32, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() && !ft.is_symlink() && depth > 0 {
            disks_in(&p, depth - 1, out);
        } else if ft.is_file() && is_vm_disk(&p) {
            out.push((p, e.metadata().map(|m| m.len()).unwrap_or(0)));
        }
    }
}

/// Folders to look in for this user (existing or not).
pub fn known_folders(wsl: &[(String, PathBuf)]) -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = wsl.iter().map(|(_, b)| b.clone()).collect();
    if let Some(local) = env("LOCALAPPDATA") {
        for sub in ["Docker\\wsl\\disk", "Docker\\wsl\\data", "Docker\\wsl\\main", "Docker\\wsl\\distro"] {
            dirs.push(local.join(sub));
        }
    }
    if let Some(home) = env("USERPROFILE") {
        dirs.push(home.join("VirtualBox VMs"));
        dirs.push(home.join("Documents\\Virtual Machines"));
    }
    if let Some(pd) = env("ProgramData") {
        dirs.push(pd.join("Microsoft\\Windows\\Virtual Hard Disks"));
    }
    if let Some(public) = env("PUBLIC") {
        dirs.push(public.join("Documents\\Hyper-V\\Virtual Hard Disks"));
    }
    dirs
}

pub fn known_disks(wsl: &[(String, PathBuf)]) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    for d in known_folders(wsl) {
        disks_in(&d, 1, &mut out);
    }
    out
}
