//! NTFS FILE record parsing from raw bytes (no I/O). One `Rec` per base record: times and
//! attributes from $STANDARD_INFORMATION, one name from $FILE_NAME (Win32 / Win32&DOS preferred,
//! then POSIX; DOS-only short names skipped; extra hard-link names ignored so a file counts once),
//! size from the unnamed $DATA stream (named streams ignored, like the directory walker).
//! Extension records (base reference != 0) are merged into their base record in any order.

pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;

pub const R_IN_USE: u8 = 1;
pub const R_DIR: u8 = 2;
pub const R_HAS_SI: u8 = 4;
pub const R_HAS_DATA: u8 = 8;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Rec {
    /// Parent directory record number (from the chosen $FILE_NAME).
    pub parent: u32,
    pub name_chunk: u32,
    pub name_off: u32,
    pub name_len: u16,
    /// 0 = no name, 2 = POSIX, 3 = Win32 or Win32&DOS.
    pub name_rank: u8,
    pub flags: u8,
    pub size: u64,
    pub alloc: u64,
    pub created: u32,
    pub modified: u32,
    pub accessed: u32,
    pub attrs: u32,
}

#[derive(Debug)]
pub struct Parsed {
    pub number: u32,
    /// Base record of an extension record; `None` for a base record.
    pub base: Option<u32>,
    pub rec: Rec,
}

pub fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
pub fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
pub fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Applies the update sequence array in place; false when the record is torn or not a FILE record.
pub fn apply_fixup(buf: &mut [u8], sector: usize) -> bool {
    if buf.len() < 48 || &buf[0..4] != b"FILE" || sector < 2 {
        return false;
    }
    let (usa_off, usa_cnt) = (u16_at(buf, 4) as usize, u16_at(buf, 6) as usize);
    if usa_cnt == 0 || usa_off + 2 * usa_cnt > buf.len() || (usa_cnt - 1) * sector > buf.len() {
        return false;
    }
    let usn = u16_at(buf, usa_off);
    for i in 1..usa_cnt {
        let pos = i * sector - 2;
        if u16_at(buf, pos) != usn {
            return false;
        }
        let fix = usa_off + 2 * i;
        buf[pos] = buf[fix];
        buf[pos + 1] = buf[fix + 1];
    }
    true
}

/// Iterates attributes of a fixed-up record as (offset, type, length).
pub fn attributes(buf: &[u8]) -> impl Iterator<Item = (usize, u32, usize)> + '_ {
    let end = (u32_at(buf, 24) as usize).min(buf.len());
    let mut off = u16_at(buf, 20) as usize;
    std::iter::from_fn(move || {
        if off + 16 > end {
            return None;
        }
        let ty = u32_at(buf, off);
        let len = u32_at(buf, off + 4) as usize;
        if ty == ATTR_END || len < 16 || off + len > end {
            return None;
        }
        let item = (off, ty, len);
        off += len;
        Some(item)
    })
}

/// Resident value of the attribute at `off`, if resident and in bounds.
pub fn resident_value(buf: &[u8], off: usize, len: usize) -> Option<&[u8]> {
    if buf[off + 8] != 0 || len < 24 {
        return None;
    }
    let (vlen, voff) = (u32_at(buf, off + 16) as usize, u16_at(buf, off + 20) as usize);
    (voff + vlen <= len).then(|| &buf[off + voff..off + voff + vlen])
}

fn ft(v: u64) -> u32 {
    crate::scan::entry::filetime_to_unix(v)
}

/// Parses one fixed-up record. `names` receives the chosen name (UTF-8); returns None when the
/// record is unused or malformed.
pub fn parse_record(buf: &[u8], number: u32, names: &mut String) -> Option<Parsed> {
    let flags = u16_at(buf, 22);
    if flags & 1 == 0 {
        return None;
    }
    let base_ref = u64_at(buf, 32) & 0xFFFF_FFFF_FFFF;
    let mut rec = Rec { flags: R_IN_USE | if flags & 2 != 0 { R_DIR } else { 0 }, ..Rec::default() };
    for (off, ty, len) in attributes(buf) {
        let name_len = buf[off + 9];
        match ty {
            ATTR_STANDARD_INFORMATION => {
                if let Some(v) = resident_value(buf, off, len).filter(|v| v.len() >= 36) {
                    rec.created = ft(u64_at(v, 0));
                    rec.modified = ft(u64_at(v, 8));
                    rec.accessed = ft(u64_at(v, 24));
                    rec.attrs = u32_at(v, 32);
                    rec.flags |= R_HAS_SI;
                }
            }
            ATTR_FILE_NAME => {
                let Some(v) = resident_value(buf, off, len).filter(|v| v.len() >= 66) else { continue };
                let (chars, ns) = (v[64] as usize, v[65]);
                let rank = match ns { 1 | 3 => 3, 0 => 2, _ => 0 };
                if rank <= rec.name_rank || 66 + chars * 2 > v.len() {
                    continue;
                }
                let start = names.len();
                let units = (0..chars).map(|k| u16_at(v, 66 + 2 * k));
                names.extend(char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')));
                rec.name_off = start as u32;
                rec.name_len = (names.len() - start).min(u16::MAX as usize) as u16;
                rec.name_rank = rank;
                rec.parent = (u64_at(v, 0) & 0xFFFF_FFFF_FFFF) as u32;
            }
            ATTR_DATA if name_len == 0 && rec.flags & R_HAS_DATA == 0 => {
                if buf[off + 8] == 0 {
                    rec.size = u32_at(buf, off + 16) as u64;
                    rec.alloc = 0; // resident data lives inside the MFT record itself
                    rec.flags |= R_HAS_DATA;
                } else if len >= 64 && u64_at(buf, off + 16) == 0 {
                    let attr_flags = u16_at(buf, off + 12);
                    let compressed_or_sparse = attr_flags & 0x0001 != 0 || attr_flags & 0x8000 != 0;
                    rec.alloc = if compressed_or_sparse && len >= 72 { u64_at(buf, off + 64) } else { u64_at(buf, off + 40) };
                    rec.size = u64_at(buf, off + 48);
                    rec.flags |= R_HAS_DATA;
                }
            }
            _ => {}
        }
    }
    let base = (base_ref != 0).then_some(base_ref as u32);
    Some(Parsed { number, base, rec })
}

/// Folds a parsed record (base or extension) into its slot; order independent.
pub fn merge(dst: &mut Rec, src: &Rec, from_base: bool) {
    if from_base {
        dst.flags |= src.flags & (R_IN_USE | R_DIR);
    }
    if src.flags & R_HAS_SI != 0 {
        dst.created = src.created;
        dst.modified = src.modified;
        dst.accessed = src.accessed;
        dst.attrs = src.attrs;
        dst.flags |= R_HAS_SI;
    }
    if src.name_rank > dst.name_rank {
        dst.parent = src.parent;
        dst.name_chunk = src.name_chunk;
        dst.name_off = src.name_off;
        dst.name_len = src.name_len;
        dst.name_rank = src.name_rank;
    }
    if src.flags & R_HAS_DATA != 0 && dst.flags & R_HAS_DATA == 0 {
        dst.size = src.size;
        dst.alloc = src.alloc;
        dst.flags |= R_HAS_DATA;
    }
}

/// Decodes an NTFS mapping-pairs array into (lcn, clusters) runs. None for sparse runs or garbage.
pub fn decode_runlist(b: &[u8]) -> Option<Vec<(u64, u64)>> {
    let (mut runs, mut i, mut lcn) = (Vec::new(), 0usize, 0i64);
    while i < b.len() && b[i] != 0 {
        let (ls, os) = ((b[i] & 0x0F) as usize, (b[i] >> 4) as usize);
        i += 1;
        if ls == 0 || ls > 8 || os == 0 || os > 8 || i + ls + os > b.len() {
            return None;
        }
        let len = (0..ls).fold(0u64, |a, k| a | (b[i + k] as u64) << (8 * k));
        i += ls;
        let mut off = (0..os).fold(0i64, |a, k| a | (b[i + k] as i64) << (8 * k));
        if b[i + os - 1] & 0x80 != 0 && os < 8 {
            off -= 1i64 << (8 * os); // sign-extend
        }
        i += os;
        lcn += off;
        if lcn < 0 {
            return None;
        }
        runs.push((lcn as u64, len));
    }
    Some(runs)
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
