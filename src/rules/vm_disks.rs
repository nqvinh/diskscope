//! Virtual machine / container disks (.vhdx .vhd .avhdx .vdi .vmdk): WSL distros, Docker Desktop,
//! Hyper-V, VirtualBox, VMware. They grow but never shrink on their own, and deleting one destroys
//! a whole VM, so diskscope only explains how to compact them (see `vm_disk_steps`) and the
//! blocklist protects them from deletion.

use crate::model::Tree;
use std::path::{Path, PathBuf};

pub const VM_EXTS: [&str; 5] = ["vhdx", "vhd", "avhdx", "vdi", "vmdk"];

/// True for a file whose extension is a VM disk format.
pub fn is_vm_disk(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| VM_EXTS.iter().any(|v| e.eq_ignore_ascii_case(v)))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Owner {
    /// WSL distro (registered name).
    Wsl(String),
    Docker,
    HyperV,
    VirtualBox,
    VmWare,
    Other,
}

impl Owner {
    pub fn name(&self) -> String {
        match self {
            Owner::Wsl(d) => format!("WSL: {d}"),
            Owner::Docker => "Docker Desktop".into(),
            Owner::HyperV => "Hyper-V / VHD".into(),
            Owner::VirtualBox => "VirtualBox".into(),
            Owner::VmWare => "VMware".into(),
            Owner::Other => "unknown".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VmDisk {
    pub path: PathBuf,
    pub size: u64,
    pub owner: Owner,
    /// Found in the current scan (else only through a known location).
    pub in_scan: bool,
}

fn norm(p: &Path) -> String {
    crate::rules::blocklist::normalize(p)
}

/// Who owns a disk: a registered WSL distro first (Docker's own distros count as Docker), then
/// the path and the extension.
pub fn classify(path: &Path, wsl: &[(String, PathBuf)]) -> Owner {
    let p = norm(path);
    if let Some((name, _)) = wsl.iter().find(|(_, base)| p.starts_with(&format!("{}\\", norm(base)))) {
        return if name.to_lowercase().starts_with("docker-desktop") { Owner::Docker } else { Owner::Wsl(name.clone()) };
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if p.contains("\\docker\\wsl\\") || p.ends_with("\\docker_data.vhdx") {
        Owner::Docker
    } else if ext == "vdi" || p.contains("\\virtualbox vms\\") {
        Owner::VirtualBox
    } else if ext == "vmdk" {
        Owner::VmWare
    } else if matches!(ext.as_str(), "vhdx" | "vhd" | "avhdx") {
        Owner::HyperV
    } else {
        Owner::Other
    }
}

/// VM disk files in a scan, via the per-extension index (no walk over the tree).
pub fn from_tree(tree: &Tree, by_ext: &[Vec<u32>]) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    for (e, ext) in tree.exts.iter().enumerate() {
        if VM_EXTS.iter().any(|v| ext.trim_start_matches('.').eq_ignore_ascii_case(v)) {
            for &i in by_ext.get(e).map(Vec::as_slice).unwrap_or(&[]) {
                if !tree.node(i).is_group() {
                    out.push((tree.path(i), tree.node(i).size));
                }
            }
        }
    }
    out
}

/// Merges scan hits with disks found in known locations (one entry per file), biggest first.
pub fn merge(scan: Vec<(PathBuf, u64)>, known: Vec<(PathBuf, u64)>, wsl: &[(String, PathBuf)]) -> Vec<VmDisk> {
    let mut out: Vec<VmDisk> = scan.into_iter().map(|(path, size)| VmDisk { owner: classify(&path, wsl), path, size, in_scan: true }).collect();
    for (path, size) in known {
        if !out.iter().any(|d| norm(&d.path) == norm(&path)) {
            out.push(VmDisk { owner: classify(&path, wsl), path, size, in_scan: false });
        }
    }
    out.sort_by_key(|d| std::cmp::Reverse(d.size));
    out
}

/// Scan hits plus the known locations on this PC (WSL registry, Docker, VirtualBox, VMware, Hyper-V).
pub fn detect(tree: &Tree, by_ext: &[Vec<u32>]) -> Vec<VmDisk> {
    let wsl = super::vm_disk_places::wsl_distros();
    merge(from_tree(tree, by_ext), super::vm_disk_places::known_disks(&wsl), &wsl)
}

#[cfg(test)]
#[path = "vm_disks_tests.rs"]
mod tests;
