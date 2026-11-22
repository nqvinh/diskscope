//! Parser tests on synthetic FILE records built byte by byte (1 KiB record, 512-byte sectors).

use super::*;

const FT_2026: u64 = 134_000_000_000_000_000; // a FILETIME in 2025-2026

fn pad8(v: &mut Vec<u8>) {
    while v.len() % 8 != 0 {
        v.push(0);
    }
}

fn resident(ty: u32, name: &str, value: &[u8]) -> Vec<u8> {
    let name16: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let mut voff = 24 + name16.len();
    voff = voff.div_ceil(8) * 8;
    let mut a = vec![0u8; voff];
    a[0..4].copy_from_slice(&ty.to_le_bytes());
    a[9] = (name16.len() / 2) as u8;
    a[10..12].copy_from_slice(&24u16.to_le_bytes());
    a[16..20].copy_from_slice(&(value.len() as u32).to_le_bytes());
    a[20..22].copy_from_slice(&(voff as u16).to_le_bytes());
    a[24..24 + name16.len()].copy_from_slice(&name16);
    a.extend_from_slice(value);
    pad8(&mut a);
    let len = a.len() as u32;
    a[4..8].copy_from_slice(&len.to_le_bytes());
    a
}

fn nonresident_data(alloc: u64, real: u64, attr_flags: u16, compressed: Option<u64>) -> Vec<u8> {
    let hdr = if compressed.is_some() { 72 } else { 64 };
    let mut a = vec![0u8; hdr];
    a[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
    a[8] = 1;
    a[12..14].copy_from_slice(&attr_flags.to_le_bytes());
    a[32..34].copy_from_slice(&(hdr as u16).to_le_bytes());
    a[40..48].copy_from_slice(&alloc.to_le_bytes());
    a[48..56].copy_from_slice(&real.to_le_bytes());
    a[56..64].copy_from_slice(&real.to_le_bytes());
    if let Some(c) = compressed {
        a[64..72].copy_from_slice(&c.to_le_bytes());
    }
    a.extend_from_slice(&[0x11, 0x01, 0x10, 0x00]); // one run, then end
    pad8(&mut a);
    let len = a.len() as u32;
    a[4..8].copy_from_slice(&len.to_le_bytes());
    a
}

fn si(attrs: u32) -> Vec<u8> {
    let mut v = vec![0u8; 48];
    for (k, t) in [FT_2026, FT_2026 + 10_000_000, 0, FT_2026 + 20_000_000].iter().enumerate() {
        v[k * 8..k * 8 + 8].copy_from_slice(&t.to_le_bytes());
    }
    v[32..36].copy_from_slice(&attrs.to_le_bytes());
    resident(ATTR_STANDARD_INFORMATION, "", &v)
}

fn file_name(parent: u64, name: &str, namespace: u8) -> Vec<u8> {
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut v = vec![0u8; 66];
    v[0..8].copy_from_slice(&(parent | (3u64 << 48)).to_le_bytes()); // sequence number in the top bits
    v[64] = units.len() as u8;
    v[65] = namespace;
    v.extend(units.iter().flat_map(|c| c.to_le_bytes()));
    resident(ATTR_FILE_NAME, "", &v)
}

/// A complete record with update sequence protection applied (as stored on disk).
fn record(flags: u16, base: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
    let mut r = vec![0u8; 1024];
    r[0..4].copy_from_slice(b"FILE");
    r[4..6].copy_from_slice(&48u16.to_le_bytes());
    r[6..8].copy_from_slice(&3u16.to_le_bytes());
    r[20..22].copy_from_slice(&56u16.to_le_bytes());
    r[22..24].copy_from_slice(&flags.to_le_bytes());
    r[28..32].copy_from_slice(&1024u32.to_le_bytes());
    r[32..40].copy_from_slice(&base.to_le_bytes());
    let mut off = 56;
    for a in attrs {
        r[off..off + a.len()].copy_from_slice(a);
        off += a.len();
    }
    r[off..off + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    r[24..28].copy_from_slice(&((off + 8) as u32).to_le_bytes());
    let usn = 0x0007u16;
    r[48..50].copy_from_slice(&usn.to_le_bytes());
    for i in 1..3 {
        let pos = i * 512 - 2;
        let saved = [r[pos], r[pos + 1]];
        r[48 + 2 * i..50 + 2 * i].copy_from_slice(&saved);
        r[pos..pos + 2].copy_from_slice(&usn.to_le_bytes());
    }
    r
}

fn parse(mut r: Vec<u8>, number: u32) -> (Option<Parsed>, String) {
    let mut names = String::new();
    if !apply_fixup(&mut r, 512) {
        return (None, names);
    }
    (parse_record(&r, number, &mut names), names)
}

#[test]
fn file_with_win32_name_and_resident_data() {
    let (p, names) = parse(record(1, 0, &[si(0x20), file_name(5, "notes.txt", 1), resident(ATTR_DATA, "", &[1u8; 300])]), 40);
    let p = p.expect("in use");
    assert_eq!((p.number, p.base), (40, None));
    let r = p.rec;
    assert_eq!(&names[r.name_off as usize..(r.name_off + r.name_len as u32) as usize], "notes.txt");
    assert_eq!((r.parent, r.size, r.alloc, r.attrs), (5, 300, 0, 0x20));
    assert_eq!(r.flags & (R_IN_USE | R_DIR | R_HAS_SI | R_HAS_DATA), R_IN_USE | R_HAS_SI | R_HAS_DATA);
    assert_eq!(r.modified, crate::scan::entry::filetime_to_unix(FT_2026 + 10_000_000));
    assert!(r.created > 1_700_000_000 && r.accessed > r.created);
}

#[test]
fn directory_flag_and_nonresident_sizes() {
    let (p, _) = parse(record(3, 0, &[si(0x10), file_name(5, "Users", 3)]), 50);
    assert!(p.unwrap().rec.flags & R_DIR != 0);
    let (p, _) = parse(record(1, 0, &[si(0), file_name(50, "big.bin", 1), nonresident_data(8192, 5000, 0, None)]), 51);
    let r = p.unwrap().rec;
    assert_eq!((r.size, r.alloc), (5000, 8192));
    let (p, _) = parse(record(1, 0, &[si(0x800), file_name(50, "c.log", 1), nonresident_data(65536, 60000, 1, Some(16384))]), 52);
    assert_eq!(p.unwrap().rec.alloc, 16384, "compressed files report their compressed size");
}

#[test]
fn name_preference_and_hard_links_count_once() {
    let (p, names) = parse(record(1, 0, &[si(0), file_name(5, "PROGRA~1", 2), file_name(5, "Program Files", 1)]), 60);
    let r = p.unwrap().rec;
    assert_eq!(&names[r.name_off as usize..(r.name_off + r.name_len as u32) as usize], "Program Files");
    let (p, names) = parse(record(1, 0, &[si(0), file_name(9, "posix", 0)]), 61);
    let r = p.unwrap().rec;
    assert_eq!((r.name_rank, &names[r.name_off as usize..(r.name_off + r.name_len as u32) as usize]), (2, "posix"));
    let (p, names) = parse(record(1, 0, &[si(0), file_name(70, "a.dll", 1), file_name(80, "b.dll", 1)]), 62);
    let r = p.unwrap().rec;
    assert_eq!((r.parent, &names[r.name_off as usize..(r.name_off + r.name_len as u32) as usize]), (70, "a.dll"));
    let (p, _) = parse(record(1, 0, &[si(0), file_name(5, "SHORT~1", 2)]), 63);
    assert_eq!(p.unwrap().rec.name_rank, 0, "a DOS-only name is not used");
}

#[test]
fn named_streams_are_ignored() {
    let (p, _) = parse(record(1, 0, &[si(0), file_name(5, "f", 1), resident(ATTR_DATA, "Zone.Identifier", &[0u8; 100]), resident(ATTR_DATA, "", &[0u8; 10])]), 70);
    assert_eq!(p.unwrap().rec.size, 10);
}

#[test]
fn torn_and_unused_records_are_skipped() {
    let mut torn = record(1, 0, &[si(0), file_name(5, "x", 1)]);
    torn[510] ^= 0xFF;
    assert!(parse(torn, 1).0.is_none());
    assert!(parse(record(0, 0, &[si(0), file_name(5, "deleted", 1)]), 2).0.is_none());
    let mut bad = record(1, 0, &[]);
    bad[0] = b'B';
    assert!(parse(bad, 3).0.is_none());
}

#[test]
fn extension_records_merge_in_any_order() {
    let (base, _) = parse(record(1, 0, &[si(0x20)]), 100);
    let (ext, _) = parse(record(1, 100, &[file_name(5, "huge.vhdx", 1), nonresident_data(1 << 30, 1 << 30, 0, None)]), 900);
    let (base, ext) = (base.unwrap(), ext.unwrap());
    assert_eq!(ext.base, Some(100));
    let mut a = Rec::default();
    merge(&mut a, &base.rec, true);
    merge(&mut a, &ext.rec, false);
    let mut b = Rec::default();
    merge(&mut b, &ext.rec, false);
    merge(&mut b, &base.rec, true);
    assert_eq!(a, b);
    assert_eq!((a.size, a.attrs, a.flags & R_HAS_SI != 0, a.name_rank), (1 << 30, 0x20, true, 3));
}

#[test]
fn runlists_decode_with_negative_offsets() {
    let runs = decode_runlist(&[0x21, 0x18, 0x34, 0x56, 0x11, 0x08, 0xF0, 0x00]).unwrap();
    assert_eq!(runs, vec![(0x5634, 0x18), (0x5624, 8)]);
    assert!(decode_runlist(&[0x01, 0x05, 0x00]).is_none(), "sparse runs are refused");
    assert!(decode_runlist(&[0x31, 0x01]).is_none(), "truncated");
    assert_eq!(super::super::volume::stream_to_disk(&runs, 4096 * 0x18 + 100, 4096), Some(0x5624 * 4096 + 100));
}
