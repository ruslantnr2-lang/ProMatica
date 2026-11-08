//! `LitematicaBitArray`: fixed-width entries packed LSB-first into longs. Unlike vanilla's
//! 1.16+ chunk storage, an entry may span two longs.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitArray {
    longs: Vec<u64>,
    bits: u32,
    mask: u64,
    len: u64,
}

impl BitArray {
    /// Bits per entry for a palette of `palette_len` entries: `max(2, ceil(log2(n)))`.
    pub fn bits_for(palette_len: usize) -> u32 {
        let n = palette_len.max(1) as u32;
        (u32::BITS - (n - 1).leading_zeros()).max(2)
    }

    pub fn new(bits: u32, len: u64) -> Self {
        let bits = bits.clamp(1, 32);
        let longs = (len * u64::from(bits)).div_ceil(64).max(1) as usize;
        Self { longs: vec![0; longs], bits, mask: (1u64 << bits) - 1, len }
    }

    /// Wraps a file's long array; fails when it is too short for `len` entries.
    pub fn from_longs(longs: &[i64], bits: u32, len: u64) -> Result<Self, String> {
        let bits = bits.clamp(1, 32);
        let need = (len * u64::from(bits)).div_ceil(64) as usize;
        if longs.len() < need {
            return Err(format!("BlockStates holds {} longs, {need} needed", longs.len()));
        }
        Ok(Self {
            longs: longs.iter().map(|&v| v as u64).collect(),
            bits,
            mask: (1u64 << bits) - 1,
            len,
        })
    }

    /// Like `from_longs`, but keeps the file's array (no copy).
    pub fn from_vec(longs: Vec<i64>, bits: u32, len: u64) -> Result<Self, String> {
        let bits = bits.clamp(1, 32);
        let need = (len * u64::from(bits)).div_ceil(64) as usize;
        if longs.len() < need {
            return Err(format!("BlockStates holds {} longs, {need} needed", longs.len()));
        }
        // Same size and alignment: the collect reuses the allocation.
        let longs: Vec<u64> = longs.into_iter().map(|v| v as u64).collect();
        Ok(Self { longs, bits, mask: (1u64 << bits) - 1, len })
    }

    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn bits(&self) -> u32 {
        self.bits
    }
    pub fn longs(&self) -> &[u64] {
        &self.longs
    }

    pub fn get(&self, index: u64) -> u32 {
        debug_assert!(index < self.len);
        let bits = u64::from(self.bits);
        let start = index * bits;
        let start_long = (start >> 6) as usize;
        let end_long = (((index + 1) * bits - 1) >> 6) as usize;
        let offset = (start & 63) as u32;
        if start_long == end_long {
            ((self.longs[start_long] >> offset) & self.mask) as u32
        } else {
            let end_offset = 64 - offset;
            (((self.longs[start_long] >> offset) | (self.longs[end_long] << end_offset)) & self.mask)
                as u32
        }
    }

    /// Repacks every entry into `bits` bits each (more than now; fewer or the same does nothing).
    pub fn widen(&mut self, bits: u32) {
        let bits = bits.clamp(1, 32);
        if bits <= self.bits {
            return;
        }
        let mut wide = Self::new(bits, self.len);
        for i in 0..self.len {
            let v = self.get(i);
            if v != 0 {
                wide.set(i, v);
            }
        }
        *self = wide;
    }

    pub fn set(&mut self, index: u64, value: u32) {
        debug_assert!(index < self.len);
        let value = u64::from(value) & self.mask;
        let bits = u64::from(self.bits);
        let start = index * bits;
        let start_long = (start >> 6) as usize;
        let end_long = (((index + 1) * bits - 1) >> 6) as usize;
        let offset = (start & 63) as u32;
        self.longs[start_long] = (self.longs[start_long] & !(self.mask << offset)) | (value << offset);
        if start_long != end_long {
            let end_offset = 64 - offset;
            let j = self.bits - end_offset;
            self.longs[end_long] = (self.longs[end_long] >> j << j) | (value >> end_offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_per_entry() {
        assert_eq!(BitArray::bits_for(1), 2);
        assert_eq!(BitArray::bits_for(2), 2);
        assert_eq!(BitArray::bits_for(4), 2);
        assert_eq!(BitArray::bits_for(5), 3);
        assert_eq!(BitArray::bits_for(17), 5);
        assert_eq!(BitArray::bits_for(256), 8);
        assert_eq!(BitArray::bits_for(257), 9);
    }

    #[test]
    fn entries_spanning_two_longs_round_trip() {
        // 5 bits: entry 12 covers bits 60..65, across the first two longs.
        let mut a = BitArray::new(5, 100);
        for i in 0..100 {
            a.set(i, (i * 7 % 31) as u32);
        }
        for i in 0..100 {
            assert_eq!(a.get(i), (i * 7 % 31) as u32, "entry {i}");
        }
        let longs: Vec<i64> = a.longs().iter().map(|&v| v as i64).collect();
        let b = BitArray::from_longs(&longs, 5, 100).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn widening_keeps_every_entry() {
        let mut a = BitArray::new(2, 1000);
        for i in 0..1000 {
            a.set(i, (i % 4) as u32);
        }
        a.widen(7);
        assert_eq!(a.bits(), 7);
        for i in 0..1000 {
            assert_eq!(a.get(i), (i % 4) as u32, "entry {i}");
        }
        a.set(999, 100);
        assert_eq!(a.get(999), 100);
        assert_eq!(a.get(998), 2);
        a.widen(3); // narrower: nothing changes
        assert_eq!(a.bits(), 7);
    }

    #[test]
    fn overwriting_keeps_neighbours() {
        let mut a = BitArray::new(3, 50);
        for i in 0..50 {
            a.set(i, 7);
        }
        a.set(21, 0); // bits 63..66, spanning
        assert_eq!(a.get(20), 7);
        assert_eq!(a.get(21), 0);
        assert_eq!(a.get(22), 7);
    }
}
