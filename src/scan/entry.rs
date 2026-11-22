//! One directory entry as read from the file system, before it joins the tree.

use std::fs::Metadata;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

pub const ATTR_DIRECTORY: u32 = 0x10;
pub const ATTR_REPARSE_POINT: u32 = 0x400;
const ATTR_SPARSE: u32 = 0x200;
const ATTR_COMPRESSED: u32 = 0x800;
const ATTR_OFFLINE: u32 = 0x1000;
const ATTR_RECALL_ON_OPEN: u32 = 0x40000;
const ATTR_RECALL_ON_DATA_ACCESS: u32 = 0x400000;

pub struct RawEntry {
    pub name: String,
    pub attrs: u32,
    pub size: u64,
    pub alloc: u64,
    pub created: u32,
    pub modified: u32,
    pub accessed: u32,
    /// Source id: MFT record number for the NTFS path, 0 for the walker.
    pub id: u64,
}

impl RawEntry {
    pub fn from_metadata(name: String, md: &Metadata, cluster: u64, dir: &Path) -> Self {
        let attrs = md.file_attributes();
        let size = if attrs & ATTR_DIRECTORY != 0 { 0 } else { md.file_size() };
        let alloc = allocated(size, attrs, cluster, || dir.join(&name));
        Self {
            attrs,
            size,
            alloc,
            created: filetime_to_unix(md.creation_time()),
            modified: filetime_to_unix(md.last_write_time()),
            accessed: filetime_to_unix(md.last_access_time()),
            id: 0,
            name,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.attrs & ATTR_DIRECTORY != 0
    }

    pub fn is_reparse(&self) -> bool {
        self.attrs & ATTR_REPARSE_POINT != 0
    }
}

/// Bytes on disk: cluster-rounded size; the real compressed size for compressed or sparse files;
/// 0 for cloud placeholders whose data is not on this disk.
pub fn allocated(size: u64, attrs: u32, cluster: u64, path: impl FnOnce() -> std::path::PathBuf) -> u64 {
    if size == 0 || attrs & (ATTR_OFFLINE | ATTR_RECALL_ON_OPEN | ATTR_RECALL_ON_DATA_ACCESS) != 0 {
        return 0;
    }
    let raw = if attrs & (ATTR_COMPRESSED | ATTR_SPARSE) != 0 {
        crate::sys::compressed_size(&path()).unwrap_or(size)
    } else {
        size
    };
    raw.div_ceil(cluster) * cluster
}

/// Windows FILETIME (100 ns since 1601) to Unix seconds, clamped to u32.
pub fn filetime_to_unix(ft: u64) -> u32 {
    (ft / 10_000_000).saturating_sub(11_644_473_600).min(u32::MAX as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_rounds_to_clusters_and_skips_placeholders() {
        let p = || std::path::PathBuf::new();
        assert_eq!(allocated(0, 0, 4096, p), 0);
        assert_eq!(allocated(1, 0, 4096, p), 4096);
        assert_eq!(allocated(4097, 0, 4096, p), 8192);
        assert_eq!(allocated(5000, ATTR_RECALL_ON_DATA_ACCESS, 4096, p), 0);
    }

    #[test]
    fn filetime_epoch() {
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0);
        assert_eq!(filetime_to_unix(0), 0);
    }
}
