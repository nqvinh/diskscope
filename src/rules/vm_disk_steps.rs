//! "How to reclaim" steps for a VM / container disk, with copyable commands that already contain
//! the disk's path. diskscope never runs them: compacting needs the VM stopped and often admin.

use super::vm_disks::{Owner, VmDisk};

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub text: String,
    /// Command (or diskpart lines) to copy; None = explanation only.
    pub cmd: Option<String>,
}

fn step(text: &str, cmd: Option<String>) -> Step {
    Step { text: text.into(), cmd }
}

/// Compact a .vhdx/.vhd: Optimize-VHD (Hyper-V module) or diskpart, both as administrator.
fn compact_vhd(path: &str) -> Vec<Step> {
    vec![
        step("Compact it: PowerShell as administrator (needs the Hyper-V module, Windows Pro/Enterprise):", Some(format!("Optimize-VHD -Path \"{path}\" -Mode Full"))),
        step("Or without Hyper-V: run 'diskpart' as administrator and type these lines:", Some(format!("select vdisk file=\"{path}\"\nattach vdisk readonly\ncompact vdisk\ndetach vdisk\nexit"))),
    ]
}

pub fn reclaim_steps(d: &VmDisk) -> Vec<Step> {
    let path = d.path.display().to_string();
    let mut v = Vec::new();
    match &d.owner {
        Owner::Wsl(distro) => {
            v.push(step("Free space inside Linux first (delete files there): the disk keeps its size until it is compacted.", None));
            v.push(step("Stop WSL (closes every distro):", Some("wsl --shutdown".into())));
            v.push(step("Once (WSL 2.0 or newer): make the disk sparse so WSL gives freed space back by itself:", Some(format!("wsl --manage \"{distro}\" --set-sparse true"))));
            v.extend(compact_vhd(&path));
        }
        Owner::Docker => {
            v.push(step("See what Docker can free (images, containers, volumes, build cache):", Some("docker system df".into())));
            v.push(step("Remove the build cache and unused images (keeps volumes, which may hold data):", Some("docker builder prune -f\ndocker image prune -a".into())));
            v.push(step("Quit Docker Desktop, then stop WSL:", Some("wsl --shutdown".into())));
            v.extend(compact_vhd(&path));
            v.push(step("Last resort: Docker Desktop > Troubleshoot > Clean / Purge data removes everything Docker stores.", None));
        }
        Owner::HyperV | Owner::Other => {
            v.push(step("Shut the virtual machine down (Hyper-V Manager) and free space inside it first.", None));
            v.extend(compact_vhd(&path));
            v.push(step("Hyper-V Manager can also do it: Edit Disk > Compact.", None));
        }
        Owner::VirtualBox => {
            v.push(step("Shut the VM down. Inside the guest, zero the free space first (Windows: 'sdelete -z C:', Linux: zerofree or fstrim).", None));
            v.push(step("Compact (VBoxManage is in C:\\Program Files\\Oracle\\VirtualBox):", Some(format!("VBoxManage modifymedium disk \"{path}\" --compact"))));
        }
        Owner::VmWare => {
            v.push(step("Shut the VM down and remove snapshots you no longer need.", None));
            v.push(step("Shrink (vmware-vdiskmanager is in the VMware Workstation folder), or use VM > Manage > Clean Up Disks:", Some(format!("vmware-vdiskmanager -k \"{path}\""))));
        }
    }
    v
}
