extern crate alloc;
use alloc::vec::Vec;
use core::iter::repeat;

#[derive(Debug, Default)]
pub struct Buf {
    buf: Vec<u8>,
    len: usize,
    max_len: usize,
}

impl Buf {
    #[inline]
    pub fn new(cap: usize, max_len: usize) -> Self {
        let buf: Vec<_> = repeat(0).take(cap).collect();
        Self {
            buf,
            len: 0,
            max_len,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[cold]
    fn resize(&mut self) {
        assert!(self.max_len > self.buf.len());
        let new_cap = core::cmp::min(1 + (3 * self.buf.len()) >> 1, self.max_len);
        self.buf.resize(new_cap, 0);
    }

    #[inline]
    pub fn push(&mut self, x: u64, num: usize) {
        let l = self.len();
        if l + 8 > self.buf.len() {
            self.resize();
        }
        self.buf[l..l + 8].copy_from_slice(&x.to_le_bytes());
        self.len += num;
    }

    #[inline]
    pub fn read_u64(&self, i: usize) -> u64 {
        debug_assert!(self.buf[i..].len() >= 8);
        u64::from_le_bytes(self.buf[i..i + 8].try_into().unwrap())
    }

    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.buf.truncate(self.len + 7);
        self.buf.shrink_to_fit();
    }
}
