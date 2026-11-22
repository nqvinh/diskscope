//! Thin Windows API wrappers: drives, cluster size, Recycle Bin, memory, last-access setting,
//! time zone, console attach, compressed file size, app data folder.

use std::path::{Path, PathBuf};
use windows_sys::Win32::Storage::FileSystem::{
    GetCompressedFileSizeW, GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetDriveTypeW, GetLogicalDrives,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `C:\` for any path on drive C.
pub fn drive_root(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    let s = s.strip_prefix("\\\\?\\").unwrap_or(&s);
    let b = s.as_bytes();
    (b.len() >= 2 && b[1] == b':').then(|| format!("{}:\\", s[..1].to_uppercase()))
}

#[derive(Clone, Debug)]
pub struct Drive {
    pub root: String,
    pub total: u64,
    pub free: u64,
}

/// Fixed and removable drives with their capacity.
pub fn drives() -> Vec<Drive> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| {
            let root = format!("{}:\\", (b'A' + i) as char);
            let w = wide(&root);
            let kind = unsafe { GetDriveTypeW(w.as_ptr()) };
            if kind != 2 && kind != 3 {
                return None; // 2 = removable, 3 = fixed
            }
            let (total, free) = disk_space(&root)?;
            Some(Drive { root, total, free })
        })
        .collect()
}

/// (total, free) bytes of the volume holding `root`.
pub fn disk_space(root: &str) -> Option<(u64, u64)> {
    let w = wide(root);
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) };
    (ok != 0).then_some((total, free))
}

pub fn cluster_size(path: &Path) -> Option<u64> {
    let w = wide(&drive_root(path)?);
    let (mut spc, mut bps, mut free, mut total) = (0u32, 0u32, 0u32, 0u32);
    let ok = unsafe { GetDiskFreeSpaceW(w.as_ptr(), &mut spc, &mut bps, &mut free, &mut total) };
    (ok != 0 && spc > 0 && bps > 0).then(|| spc as u64 * bps as u64)
}

/// Real on-disk size of a compressed or sparse file.
pub fn compressed_size(path: &Path) -> Option<u64> {
    let w = wide(&path.to_string_lossy());
    let mut high = 0u32;
    let low = unsafe { GetCompressedFileSizeW(w.as_ptr(), &mut high) };
    (low != u32::MAX).then(|| ((high as u64) << 32) | low as u64)
}

/// (bytes, items) in the Recycle Bin of all drives.
pub fn recycle_bin() -> Option<(u64, u64)> {
    use windows_sys::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, i64Size: 0, i64NumItems: 0 };
    let hr = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) };
    (hr >= 0).then(|| (info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Memory {
    /// This process: private bytes, and the peak since it started.
    pub process: u64,
    pub peak: u64,
    pub system_free: u64,
    pub system_total: u64,
}

pub fn memory() -> Memory {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut m = Memory::default();
    unsafe {
        let mut pmc: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pmc.cb) != 0 {
            m.process = pmc.PrivateUsage as u64;
            m.peak = pmc.PeakPagefileUsage as u64;
        }
        let mut ms: MEMORYSTATUSEX = std::mem::zeroed();
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut ms) != 0 {
            m.system_free = ms.ullAvailPhys;
            m.system_total = ms.ullTotalPhys;
        }
    }
    m
}

/// Whether NTFS updates last-access times (NtfsDisableLastAccessUpdate low bit = 0). None = unknown.
pub fn last_access_enabled() -> Option<bool> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    let key = wide("SYSTEM\\CurrentControlSet\\Control\\FileSystem");
    let val = wide("NtfsDisableLastAccessUpdate");
    let mut data = 0u32;
    let mut len = 4u32;
    let rc = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut data as *mut u32 as *mut _, &mut len)
    };
    (rc == 0).then_some(data & 1 == 0)
}

/// Local time minus UTC, in seconds.
pub fn local_utc_offset_secs() -> i64 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let mut bias = tz.Bias as i64;
    if id == 2 {
        bias += tz.DaylightBias as i64;
    }
    -bias * 60
}

/// Lets a GUI-subsystem exe print to the console it was started from (CLI mode).
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// True when this process runs elevated (needed to read the NTFS MFT directly).
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, (&mut e as *mut TOKEN_ELEVATION).cast(), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len);
        CloseHandle(token);
        ok != 0 && e.TokenIsElevated != 0
    }
}

/// Starts this exe again elevated (Windows shows its UAC prompt). True when it was launched.
pub fn restart_elevated(args: &str) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let Ok(exe) = std::env::current_exe() else { return false };
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (verb, file, params) = (w("runas"), w(&exe.to_string_lossy()), w(args));
    let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), std::ptr::null(), 1) };
    r as isize > 32
}

/// File system of a volume root like `C:\` ("NTFS", "FAT32", ...).
pub fn filesystem_name(root: &str) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
    let w = wide(root);
    let mut fs = [0u16; 32];
    let ok = unsafe {
        GetVolumeInformationW(w.as_ptr(), std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), fs.len() as u32)
    };
    let end = fs.iter().position(|&c| c == 0).unwrap_or(fs.len());
    (ok != 0).then(|| String::from_utf16_lossy(&fs[..end]))
}

/// %LOCALAPPDATA%\diskscope (snapshots, action log).
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("diskscope")
}

#[test]
fn drive_roots() {
    assert_eq!(drive_root(Path::new("c:\\Users\\alice")).as_deref(), Some("C:\\"));
    assert_eq!(drive_root(Path::new("\\\\?\\D:\\x")).as_deref(), Some("D:\\"));
    assert_eq!(drive_root(Path::new("relative")), None);
}
