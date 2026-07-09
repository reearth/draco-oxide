use crate::bit_coder::{ByteReader, ByteWriter, ReaderErr};

#[allow(unused)]
pub fn leb128_read<W>(reader: &mut W) -> Result<u64, ReaderErr>
where
    W: ByteReader,
{
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        let byte = reader.read_u8()?;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    Ok(result)
}

/// Maximum bytes a single declared-length read pre-allocates up front.
/// A larger declared length still succeeds — the buffer grows on
/// demand — but the up-front allocation can't be larger than this,
/// so a hostile bitstream containing a multi-GB leb128 length can't
/// trigger an OOM-abort before any actual bytes are read. If the
/// stream truly does carry more than the cap, the per-byte reads
/// will succeed and the `Vec` will reallocate normally.
const PREALLOC_CAP: usize = 64 * 1024 * 1024;

/// Read `declared_len` bytes from `reader` into a fresh `Vec<u8>`.
/// Use this any time the length comes from the bitstream itself
/// (typically via `leb128_read`) — it caps the initial allocation so
/// that a malformed length field can't OOM-abort the process before
/// the decoder gets a chance to surface a `ReaderErr`.
pub fn read_byte_buffer<R>(reader: &mut R, declared_len: usize) -> Result<Vec<u8>, ReaderErr>
where
    R: ByteReader,
{
    let cap = declared_len.min(PREALLOC_CAP);
    let mut buf = Vec::with_capacity(cap);
    for _ in 0..declared_len {
        buf.push(reader.read_u8()?);
    }
    Ok(buf)
}

pub fn leb128_write<W>(mut value: u64, writer: &mut W)
where
    W: ByteWriter,
{
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            writer.write_u8(byte);
            break;
        } else {
            writer.write_u8(byte | 0x80);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_test_leb128_write_read() {
        let mut buffer = Vec::new();
        leb128_write(300, &mut buffer);
        assert_eq!(buffer, vec![172, 2]);

        let mut reader = buffer.into_iter();
        let value = leb128_read(&mut reader).unwrap();
        assert_eq!(value, 300);
    }

    #[test]
    fn more_tests_leb128() {
        let testdata = vec![0, 1, 127, 128, 255, 256, 1234567890, 0xFFFFFFFFFFFFFFFF];
        let mut buffer = Vec::new();
        for &value in &testdata {
            leb128_write(value, &mut buffer);
        }
        let mut reader = buffer.into_iter();
        for &expected in &testdata {
            let value = leb128_read(&mut reader).unwrap();
            assert_eq!(value, expected);
        }
        assert!(
            reader.next().is_none(),
            "Reader should be empty after reading all values"
        );
    }
}
