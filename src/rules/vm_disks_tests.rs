//! VM / container disks: extension check, owner, scan detection, merge, reclaim steps, blocklist
//! and the new dev-cache suggestion rules (pip cache, NuGet packages).

use super::*;
use crate::model::fixture::build;
use crate::rules::vm_disk_places::clean_base;
use crate::rules::vm_disk_steps::reclaim_steps;

fn wsl() -> Vec<(String, PathBuf)> {
    vec![
        ("Ubuntu".into(), PathBuf::from("C:\\Users\\a\\AppData\\Local\\Packages\\CanonicalGroupLimited.Ubuntu_79rhkp1fndgsc\\LocalState")),
        ("docker-desktop".into(), PathBuf::from("C:\\Users\\a\\AppData\\Local\\Docker\\wsl\\main")),
    ]
}

#[test]
fn recognises_vm_disk_extensions() {
    for p in ["D:\\VMs\\dev-box.vhdx", "x\\old.VHD", "x\\snap.avhdx", "x\\win.vdi", "x\\disk-s001.vmdk"] {
        assert!(is_vm_disk(Path::new(p)), "{p}");
    }
    for p in ["x\\notes.txt", "x\\vhdx", "x\\backup.vhdx.zip", "x\\disk.img"] {
        assert!(!is_vm_disk(Path::new(p)), "{p}");
    }
}

#[test]
fn finds_the_owner() {
    let w = wsl();
    let c = |p: &str| classify(Path::new(p), &w);
    assert_eq!(c("C:\\Users\\a\\AppData\\Local\\Packages\\CanonicalGroupLimited.Ubuntu_79rhkp1fndgsc\\LocalState\\ext4.vhdx"), Owner::Wsl("Ubuntu".into()));
    assert_eq!(c("C:\\Users\\a\\AppData\\Local\\Docker\\wsl\\main\\ext4.vhdx"), Owner::Docker, "Docker's own distro");
    assert_eq!(c("C:\\Users\\a\\AppData\\Local\\Docker\\wsl\\disk\\docker_data.vhdx"), Owner::Docker);
    assert_eq!(c("D:\\VirtualBox VMs\\win\\win.vmdk"), Owner::VirtualBox, "vmdk inside VirtualBox VMs");
    assert_eq!(c("D:\\vm\\win.vdi"), Owner::VirtualBox);
    assert_eq!(c("D:\\vm\\Windows 11\\Windows 11.vmdk"), Owner::VmWare);
    assert_eq!(c("D:\\VMs\\dev-box.vhdx"), Owner::HyperV);
    assert_eq!(clean_base("\\\\?\\C:\\wsl\\Ubuntu"), PathBuf::from("C:\\wsl\\Ubuntu"));
}

#[test]
fn detects_disks_in_a_scan_and_merges_known_ones() {
    let t = build("D:\\", &[("VMs\\dev-box.vhdx", 30 << 20, 1), ("VMs\\notes.txt", 10, 1), ("vb\\win.VDI", 5 << 20, 1)]);
    let idx = crate::views::ext_files::build_index(&t);
    let mut hits = from_tree(&t, &idx.by_ext);
    hits.sort();
    assert_eq!(hits, [(PathBuf::from("D:\\VMs\\dev-box.vhdx"), 30 << 20), (PathBuf::from("D:\\vb\\win.VDI"), 5 << 20)]);
    let known = vec![(PathBuf::from("d:\\vms\\DEV-BOX.vhdx"), 1), (PathBuf::from("C:\\wsl\\ext4.vhdx"), 90 << 20)];
    let all = merge(hits, known, &[]);
    assert_eq!(all.len(), 3, "the scan hit and the known path are the same file");
    assert_eq!(all[0].path, PathBuf::from("C:\\wsl\\ext4.vhdx"), "biggest first");
    assert!(!all[0].in_scan && all[1].in_scan);
}

#[test]
fn steps_quote_the_path_and_never_delete() {
    let d = |owner: Owner| VmDisk { path: PathBuf::from("C:\\Users\\a b\\ext4.vhdx"), size: 1, owner, in_scan: true };
    let wsl = reclaim_steps(&d(Owner::Wsl("Ubuntu-22.04".into())));
    let cmds: Vec<String> = wsl.iter().filter_map(|s| s.cmd.clone()).collect();
    assert_eq!(cmds[0], "wsl --shutdown");
    assert_eq!(cmds[1], "wsl --manage \"Ubuntu-22.04\" --set-sparse true");
    assert!(cmds[2].contains("Optimize-VHD -Path \"C:\\Users\\a b\\ext4.vhdx\""));
    assert!(cmds[3].starts_with("select vdisk file=\"C:\\Users\\a b\\ext4.vhdx\"") && cmds[3].contains("compact vdisk"));
    let docker = reclaim_steps(&d(Owner::Docker));
    assert!(docker[0].cmd.as_deref() == Some("docker system df"), "inspect before pruning");
    for owner in [Owner::Wsl("x".into()), Owner::Docker, Owner::HyperV, Owner::VirtualBox, Owner::VmWare, Owner::Other] {
        for s in reclaim_steps(&d(owner)) {
            let c = s.cmd.unwrap_or_default().to_lowercase();
            assert!(!c.contains("del ") && !c.contains("remove-item") && !c.contains("--volumes"), "no destructive command: {c}");
        }
    }
}

#[test]
fn vm_disks_are_blocked_from_deletion() {
    let why = crate::rules::blocklist::blocked_reason(Path::new("D:\\VMs\\dev-box.vhdx"));
    assert!(why.is_some_and(|w| w.contains("virtual")), "{why:?}");
    assert!(crate::rules::blocklist::blocked_reason(Path::new("D:\\VMs\\notes.txt")).is_none());
    assert!(crate::rules::blocklist::blocked_reason(Path::new("D:\\VMs")).is_none(), "the folder itself stays the user's choice");
}

#[test]
fn suggests_pip_cache_and_nuget_packages() {
    let t = build("C:\\Users\\a", &[("AppData\\Local\\pip\\cache\\http\\x.bin", 5 << 20, 1), (".nuget\\packages\\newtonsoft.json\\13.0.1\\n.nupkg", 8 << 20, 1)]);
    let ao = crate::rules::ao_sessions::AoSessions::default();
    let kinds: Vec<&str> = crate::rules::candidates::detect(&t, &ao, 100).iter().map(|c| c.kind).collect();
    assert!(kinds.contains(&"pip-cache") && kinds.contains(&"nuget-packages"), "{kinds:?}");
}
