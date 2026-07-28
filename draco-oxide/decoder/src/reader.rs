//! Reverse byte cursor for the rANS/rabs decoders, which consume renormalized
//! bytes back-to-front from the tail of a sub-stream. The sub-stream is a
//! borrowed slice carved out of the forward [`draco_oxide_core::bit_coder::Reader`],
//! so no bytes are copied while reading.

use draco_oxide_core::bit_coder::ReaderErr;

/// Reads a borrowed byte slice back-to-front. Multi-byte reads reproduce the
/// little-endian value as if the bytes had been read forward.
pub struct RevReader<'a> {
    data: &'a [u8],
}

impl<'a> RevReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn read_u8_back(&mut self) -> Result<u8, ReaderErr> {
        let (&last, rest) = self.data.split_last().ok_or(ReaderErr::NotEnoughData)?;
        self.data = rest;
        Ok(last)
    }

    pub fn read_u16_back(&mut self) -> Result<u16, ReaderErr> {
        let mut out = [self.read_u8_back()?, self.read_u8_back()?];
        out.reverse();
        Ok(u16::from_le_bytes(out))
    }

    pub fn read_u24_back(&mut self) -> Result<u32, ReaderErr> {
        let mut out = [
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
        ];
        out.reverse();
        Ok(u32::from_le_bytes([out[0], out[1], out[2], 0]))
    }

    pub fn read_u32_back(&mut self) -> Result<u32, ReaderErr> {
        let mut out = [
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
        ];
        out.reverse();
        Ok(u32::from_le_bytes(out))
    }

    /// Renormalizes a rANS state: folds in tail bytes until `state >= l_base`
    /// or the buffer runs dry, exactly as repeated [`Self::read_u8_back`] calls
    /// would. `l_base` must be a power of two. The common case (at least 4
    /// bytes left) computes the byte count from the state's bit length and
    /// folds with a single 4-byte load instead of a data-dependent byte loop.
    #[inline]
    pub fn rans_refill(&mut self, state: usize, l_base: usize) -> usize {
        let n = self.data.len();
        if n >= 4 {
            let l_bits = l_base.trailing_zeros() as usize;
            // `state | 1` guards the (malformed-stream-only) state == 0 case;
            // it never changes the bit length of a live state, which is >= 4
            // whenever bytes remain.
            let msb = 63 - (state | 1).leading_zeros() as usize;
            // Smallest k with the refilled bit length reaching l_bits; k <= 3
            // because states stay below l_base << 8 = 2^(l_bits + 8).
            let k = (l_bits.saturating_sub(msb) + 7) >> 3;
            let w = u32::from_le_bytes(self.data[n - 4..n].try_into().unwrap()) as u64;
            let pulled = (w >> (32 - 8 * k)) as usize;
            self.data = &self.data[..n - k];
            (state << (8 * k)) | pulled
        } else {
            let mut state = state;
            while state < l_base {
                match self.read_u8_back() {
                    Ok(byte) => state = (state << 8) | byte as usize,
                    Err(_) => break,
                }
            }
            state
        }
    }

    pub fn read_u64_back(&mut self) -> Result<u64, ReaderErr> {
        let mut out = [
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
            self.read_u8_back()?,
        ];
        out.reverse();
        Ok(u64::from_le_bytes(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use draco_oxide_core::bit_coder::{ByteWriter, Reader};

    #[test]
    fn reverse_reader_reads_sub_slice_backwards() {
        let buffer = vec![1_u8, 2, 3, 4, 5];
        let mut reader = Reader::new(&buffer);
        let head = reader.read_bytes(2).unwrap();
        let mut rev = RevReader::new(head);
        assert_eq!(rev.read_u8_back().unwrap(), 2);
        assert_eq!(rev.read_u8_back().unwrap(), 1);
        assert_eq!(rev.read_u8_back(), Err(ReaderErr::NotEnoughData));
        // The forward cursor continues past the reversed head.
        assert_eq!(reader.read_u8().unwrap(), 3);
        assert_eq!(reader.read_u8().unwrap(), 4);
        assert_eq!(reader.read_u8().unwrap(), 5);
        assert!(reader.is_empty());
    }

    #[test]
    fn reverse_reader_reads_little_endian_widths() {
        let mut buffer = Vec::new();
        buffer.write_u8(200);
        buffer.write_u16(201);
        buffer.write_u24(202);
        buffer.write_u32(203);
        assert_eq!(buffer.len(), 10);
        let mut rev = RevReader::new(&buffer);
        assert_eq!(rev.read_u32_back().unwrap(), 203);
        assert_eq!(rev.read_u24_back().unwrap(), 202);
        assert_eq!(rev.read_u16_back().unwrap(), 201);
        assert_eq!(rev.read_u8_back().unwrap(), 200);
        assert_eq!(rev.read_u8_back(), Err(ReaderErr::NotEnoughData));
    }
}
