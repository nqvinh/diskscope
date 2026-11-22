//! Compact string storage for file names: all names live in 4 MiB string chunks and a node keeps
//! only a 32-bit reference plus a 16-bit length (no per-name heap allocation).

const CHUNK_BITS: u32 = 22;
const CHUNK: usize = 1 << CHUNK_BITS;

#[derive(Default)]
pub struct NameStore {
    chunks: Vec<String>,
}

impl NameStore {
    /// Stores `s` and returns its reference and length. Strings over 64 KiB are truncated (real file
    /// names are at most 255 UTF-16 units; only the root's display path could be long).
    pub fn push(&mut self, s: &str) -> (u32, u16) {
        let s = &s[..floor_char_boundary(s, u16::MAX as usize)];
        let need_new = match self.chunks.last() {
            None => true,
            Some(last) => last.len() + s.len() > CHUNK,
        };
        if need_new {
            assert!(self.chunks.len() < (1usize << (32 - CHUNK_BITS)), "name store full");
            self.chunks.push(String::with_capacity(CHUNK));
        }
        let idx = self.chunks.len() - 1;
        let last = self.chunks.last_mut().expect("chunk");
        let off = last.len();
        last.push_str(s);
        (((idx as u32) << CHUNK_BITS) | off as u32, s.len() as u16)
    }

    pub fn get(&self, r: u32, len: u16) -> &str {
        let chunk = (r >> CHUNK_BITS) as usize;
        let off = (r & (CHUNK as u32 - 1)) as usize;
        &self.chunks[chunk][off..off + len as usize]
    }

    /// Releases the unused tail of the last chunk.
    pub fn shrink_last(&mut self) {
        if let Some(last) = self.chunks.last_mut() {
            last.shrink_to_fit();
        }
    }

    pub fn heap_bytes(&self) -> usize {
        self.chunks.iter().map(|c| c.capacity()).sum()
    }
}

fn floor_char_boundary(s: &str, max: usize) -> usize {
    if s.len() <= max {
        return s.len();
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[test]
fn round_trip() {
    let mut n = NameStore::default();
    let a = n.push("node_modules");
    let b = n.push("Ảnh chụp.png");
    assert_eq!(n.get(a.0, a.1), "node_modules");
    assert_eq!(n.get(b.0, b.1), "Ảnh chụp.png");
}
