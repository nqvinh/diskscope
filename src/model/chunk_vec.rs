//! Append-only vector stored in fixed 64K-element chunks. Growing never copies existing elements,
//! so a 2.5M-node scan does not briefly need twice its memory the way `Vec` doubling does.

const SHIFT: usize = 16;
const CHUNK: usize = 1 << SHIFT;
const MASK: usize = CHUNK - 1;

pub struct ChunkVec<T> {
    chunks: Vec<Vec<T>>,
    len: usize,
}

impl<T> Default for ChunkVec<T> {
    fn default() -> Self {
        Self { chunks: Vec::new(), len: 0 }
    }
}

impl<T> ChunkVec<T> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends `value` and returns its index.
    pub fn push(&mut self, value: T) -> usize {
        if self.len & MASK == 0 {
            self.chunks.push(Vec::with_capacity(CHUNK));
        }
        self.chunks.last_mut().expect("chunk").push(value);
        self.len += 1;
        self.len - 1
    }

    /// Releases the unused tail of the last chunk.
    pub fn shrink_last(&mut self) {
        if let Some(last) = self.chunks.last_mut() {
            last.shrink_to_fit();
        }
    }

    /// Bytes held by element storage (capacity, not length).
    pub fn heap_bytes(&self) -> usize {
        self.chunks.iter().map(|c| c.capacity() * std::mem::size_of::<T>()).sum()
    }
}

impl<T> std::ops::Index<usize> for ChunkVec<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        &self.chunks[i >> SHIFT][i & MASK]
    }
}

impl<T> std::ops::IndexMut<usize> for ChunkVec<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        &mut self.chunks[i >> SHIFT][i & MASK]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_index_across_chunks() {
        let mut v = ChunkVec::default();
        for i in 0..(CHUNK * 2 + 10) {
            assert_eq!(v.push(i as u32), i);
        }
        assert_eq!(v.len(), CHUNK * 2 + 10);
        assert_eq!(v[CHUNK - 1], (CHUNK - 1) as u32);
        assert_eq!(v[CHUNK * 2 + 9], (CHUNK * 2 + 9) as u32);
        v[5] = 99;
        assert_eq!(v[5], 99);
    }
}
