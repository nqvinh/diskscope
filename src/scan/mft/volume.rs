//! Read-only access to an NTFS volume. The handle is opened with GENERIC_READ only (sharing
//! read/write so nothing is locked); the only control code used is FSCTL_GET_NTFS_VOLUME_DATA,
//! a query. Nothing here can write to the volume.

use super::record::{apply_fixup, attributes, decode_runlist, resident_value, u16_at, u32_at, u64_at, ATTR_ATTRIBUTE_LIST, ATTR_DATA};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile, SetFilePointerEx, FILE_BEGIN, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::IO::DeviceIoControl;

const FSCTL_GET_NTFS_VOLUME_DATA: u32 = 0x0009_0064;

pub struct Volume {
    handle: HANDLE,
    letter: char,
    pub sector: usize,
    pub cluster: u64,
    pub record: usize,
    pub mft_valid: u64,
    mft_lcn: u64,
    pub total_clusters: u64,
    pub free_clusters: u64,
}

// A handle is only used by one thread at a time (each reader thread owns its own Volume).
unsafe impl Send for Volume {}

impl Drop for Volume {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

fn err(what: &str) -> String {
    format!("{what} (Windows error {})", unsafe { GetLastError() })
}

impl Volume {
    pub fn open(letter: char) -> Result<Self, String> {
        let path: Vec<u16> = format!("\\\\.\\{letter}:").encode_utf16().chain(Some(0)).collect();
        let h = unsafe { CreateFileW(path.as_ptr(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut()) };
        if h == INVALID_HANDLE_VALUE {
            return Err(err(&format!("cannot open volume {letter}: for reading")));
        }
        let mut v = Volume { handle: h, letter, sector: 0, cluster: 0, record: 0, mft_valid: 0, mft_lcn: 0, total_clusters: 0, free_clusters: 0 };
        let mut out = [0u8; 128];
        let mut got = 0u32;
        let ok = unsafe { DeviceIoControl(h, FSCTL_GET_NTFS_VOLUME_DATA, null(), 0, out.as_mut_ptr().cast(), out.len() as u32, &mut got, null_mut()) };
        if ok == 0 || got < 96 {
            return Err(err("not an NTFS volume or no volume data"));
        }
        v.total_clusters = u64_at(&out, 16);
        v.free_clusters = u64_at(&out, 24);
        v.sector = u32_at(&out, 40) as usize;
        v.cluster = u32_at(&out, 44) as u64;
        v.record = u32_at(&out, 48) as usize;
        v.mft_valid = u64_at(&out, 56);
        v.mft_lcn = u64_at(&out, 64);
        if v.sector < 512 || v.record < 1024 || v.cluster == 0 || v.cluster % v.record as u64 != 0 && v.record as u64 % v.cluster != 0 {
            return Err("unusual NTFS geometry; using the directory walker".into());
        }
        Ok(v)
    }

    /// A second independent read-only handle (its own file position) for a parallel reader.
    pub fn reopen(&self) -> Result<Self, String> {
        Self::open(self.letter)
    }

    /// Reads exactly `buf.len()` bytes at a byte offset (sector-aligned offsets and lengths).
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        if unsafe { SetFilePointerEx(self.handle, offset as i64, null_mut(), FILE_BEGIN) } == 0 {
            return Err(err("seek failed"));
        }
        let mut done = 0usize;
        while done < buf.len() {
            let want = (buf.len() - done).min(64 << 20) as u32;
            let mut got = 0u32;
            let ok = unsafe { ReadFile(self.handle, buf[done..].as_mut_ptr(), want, &mut got, null_mut()) };
            if ok == 0 || got == 0 {
                return Err(err("volume read failed"));
            }
            done += got as usize;
        }
        Ok(())
    }

    fn read_record_at(&self, disk_offset: u64) -> Result<Vec<u8>, String> {
        let mut r = vec![0u8; self.record];
        self.read_at(disk_offset, &mut r)?;
        if !apply_fixup(&mut r, self.sector) {
            return Err("an $MFT record failed its consistency check".into());
        }
        Ok(r)
    }

    /// The $MFT's own data runs (lcn, clusters), in stream order, covering all valid records.
    pub fn mft_runs(&self) -> Result<Vec<(u64, u64)>, String> {
        let rec0 = self.read_record_at(self.mft_lcn * self.cluster)?;
        let mut frags = data_fragments(&rec0)?;
        for r in self.attribute_list_data_records(&rec0)? {
            let concat: Vec<(u64, u64)> = sorted_runs(&frags);
            let pos = r as u64 * self.record as u64;
            // The first $MFT extent starts at MftStartLcn, so low records are found even before any run is known.
            let off = stream_to_disk(&concat, pos, self.cluster)
                .or_else(|| concat.is_empty().then(|| self.mft_lcn * self.cluster + pos))
                .ok_or("$MFT extension record outside the known runs")?;
            frags.extend(data_fragments(&self.read_record_at(off)?)?);
        }
        let runs = sorted_runs(&frags);
        let covered: u64 = runs.iter().map(|r| r.1 * self.cluster).sum();
        if covered < self.mft_valid {
            return Err("could not map the whole $MFT; using the directory walker".into());
        }
        Ok(runs)
    }
}

/// (start_vcn, runs) of every unnamed non-resident $DATA fragment in a record.
fn data_fragments(rec: &[u8]) -> Result<Vec<(u64, Vec<(u64, u64)>)>, String> {
    let mut out = Vec::new();
    for (off, ty, len) in attributes(rec) {
        if ty == ATTR_DATA && rec[off + 9] == 0 && rec[off + 8] != 0 && len >= 64 {
            let ro = u16_at(rec, off + 32) as usize;
            let runs = decode_runlist(&rec[off + ro.min(len)..off + len]).ok_or("bad $MFT runlist")?;
            out.push((u64_at(rec, off + 16), runs));
        }
    }
    Ok(out)
}

impl Volume {
    /// Value of $MFT's attribute list, resident or (for a very fragmented $MFT) read from its clusters.
    fn attribute_list_value(&self, rec: &[u8], off: usize, len: usize) -> Result<Vec<u8>, String> {
        if let Some(v) = resident_value(rec, off, len) {
            return Ok(v.to_vec());
        }
        if len < 64 {
            return Err("bad $MFT attribute list".into());
        }
        let real = u64_at(rec, off + 48) as usize;
        let ro = u16_at(rec, off + 32) as usize;
        let runs = decode_runlist(&rec[off + ro.min(len)..off + len]).ok_or("bad $MFT attribute list runlist")?;
        let mut v = Vec::with_capacity(real);
        for (lcn, clusters) in runs {
            let mut buf = vec![0u8; (clusters * self.cluster) as usize];
            self.read_at(lcn * self.cluster, &mut buf)?;
            v.extend_from_slice(&buf);
            if v.len() >= real {
                break;
            }
        }
        if v.len() < real {
            return Err("$MFT attribute list shorter than its size".into());
        }
        v.truncate(real);
        Ok(v)
    }

    /// Records named by $MFT's attribute list as holding further $DATA fragments.
    fn attribute_list_data_records(&self, rec: &[u8]) -> Result<Vec<u32>, String> {
        let mut out = Vec::new();
        for (off, ty, len) in attributes(rec) {
            if ty == ATTR_ATTRIBUTE_LIST {
                attribute_list_entries(&self.attribute_list_value(rec, off, len)?, &mut out);
            }
        }
        Ok(out)
    }
}

/// $DATA entries of an attribute list value that live in other records.
fn attribute_list_entries(v: &[u8], out: &mut Vec<u32>) {
    {
        let mut p = 0;
        while p + 26 <= v.len() {
            let (ety, elen) = (u32_at(v, p), u16_at(v, p + 4) as usize);
            let r = (u64_at(v, p + 16) & 0xFFFF_FFFF_FFFF) as u32;
            if ety == ATTR_DATA && r != 0 && !out.contains(&r) {
                out.push(r);
            }
            if elen == 0 {
                break;
            }
            p += elen;
        }
    }
}

fn sorted_runs(frags: &[(u64, Vec<(u64, u64)>)]) -> Vec<(u64, u64)> {
    let mut f: Vec<&(u64, Vec<(u64, u64)>)> = frags.iter().collect();
    f.sort_by_key(|x| x.0);
    f.iter().flat_map(|x| x.1.iter().copied()).collect()
}

/// Disk byte offset of byte `pos` of the $MFT stream.
pub fn stream_to_disk(runs: &[(u64, u64)], pos: u64, cluster: u64) -> Option<u64> {
    let mut start = 0u64;
    for &(lcn, len) in runs {
        let bytes = len * cluster;
        if pos < start + bytes {
            return Some(lcn * cluster + (pos - start));
        }
        start += bytes;
    }
    None
}
