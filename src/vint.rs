use crate::buf::Buf;

pub struct VarInt;

impl VarInt {
    #[inline]
    pub fn write(buf: &mut Buf, val: u32) {
        let x = val as u64;
        let size = ((70 - (x | 1).leading_zeros()) / 7) as usize;
        let res = (x << size) | (1 << (size - 1));
        buf.push(res, size);
    }

    #[inline]
    pub fn read(buf: &Buf, index: usize) -> (u32, usize) {
        let x = buf.read_u64(index);
        let size = x.trailing_zeros() as usize + 1;
        let mask = u64::MAX >> ((8 - size) << 3);
        let res = (x & mask) >> size;
        (res as u32, index + size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vint_simple() {
        let mut buf = Buf::new(100, 100);
        VarInt::write(&mut buf, 43);
        let (res, _) = VarInt::read(&buf, 0);
        assert_eq!(res, 43);
    }

    #[test]
    fn vint_codec() {
        let mut buf = Buf::new(100, 100);
        VarInt::write(&mut buf, 42);
        VarInt::write(&mut buf, 256);
        let (decoded_first, i) = VarInt::read(&buf, 0);
        let (decoded_second, _) = VarInt::read(&buf, i);
        assert_eq!(decoded_first, 42);
        assert_eq!(decoded_second, 256);
    }

    #[test]
    fn codec_perm() {
        let mut buf = Buf::new(100, 100);
        let vals = [0, 1, 2, 42, 256, 5515, 99049043, u32::MAX];
        for first in vals.iter() {
            for second in vals.iter() {
                VarInt::write(&mut buf, *first);
                VarInt::write(&mut buf, *second);
                let (decoded_first, i) = VarInt::read(&buf, 0);
                let (decoded_second, _) = VarInt::read(&buf, i);
                assert_eq!(decoded_first, *first);
                assert_eq!(decoded_second, *second);
                buf = Buf::new(100, 100);
            }
        }
    }
}
