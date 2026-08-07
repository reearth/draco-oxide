//! Metadata parsing. The section is fully parsed so the reader stays in sync
//! with the stream; the parsed values are not materialized and are dropped.

use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::utils::bit_coder::leb128_read;

/// The reference decoder's cap on sub-metadata nesting.
const MAX_SUBMETADATA_LEVEL: usize = 1000;

/// Consumes the geometry metadata section: a leb128 count of per-attribute
/// metadata blocks, each a leb128 attribute unique id followed by a metadata
/// block, then the geometry-level metadata block.
pub(crate) fn decode_metadata(reader: &mut Reader<'_>) -> Result<(), Err> {
    let num_att_metadata = leb128_read(reader)?;
    for _ in 0..num_att_metadata {
        let _att_unique_id = leb128_read(reader)?;
        skip_metadata(reader, 0)?;
    }
    skip_metadata(reader, 0)
}

/// Consumes one metadata block: a leb128 count of name/value entries, then a
/// leb128 count of named sub-metadata blocks, depth-first.
fn skip_metadata(reader: &mut Reader<'_>, level: usize) -> Result<(), Err> {
    if level > MAX_SUBMETADATA_LEVEL {
        return Err(Err::MalformedMetadata("sub-metadata nested too deeply"));
    }
    let num_entries = leb128_read(reader)?;
    for _ in 0..num_entries {
        skip_name(reader)?;
        let data_size = leb128_read(reader)? as usize;
        if data_size == 0 {
            return Err(Err::MalformedMetadata("metadata entry with empty value"));
        }
        reader.read_bytes(data_size)?;
    }
    let num_sub_metadata = leb128_read(reader)?;
    for _ in 0..num_sub_metadata {
        skip_name(reader)?;
        skip_metadata(reader, level + 1)?;
    }
    Ok(())
}

/// Consumes a length-prefixed name: a u8 length, then that many bytes.
fn skip_name(reader: &mut Reader<'_>) -> Result<(), Err> {
    let len = reader.read_u8()? as usize;
    reader.read_bytes(len)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One entry: `[name_len][name][leb128 value_len][value]`.
    fn entry(name: &str, value: &[u8]) -> Vec<u8> {
        let mut out = vec![name.len() as u8];
        out.extend(name.as_bytes());
        out.push(value.len() as u8); // values under 128 bytes: leb128 is one byte
        out.extend(value);
        out
    }

    #[test]
    fn consumes_exactly_the_metadata_section() {
        // One attribute block (unique id 7, one entry), then geometry metadata
        // with one entry and one sub-metadata holding another entry.
        let mut bytes = vec![1u8, 7];
        bytes.push(1);
        bytes.extend(entry("name", b"material"));
        bytes.push(0); // attribute block: no sub-metadata
        bytes.push(1);
        bytes.extend(entry("created_by", b"test"));
        bytes.push(1); // one sub-metadata
        bytes.extend([3u8]);
        bytes.extend(b"sub");
        bytes.push(1);
        bytes.extend(entry("k", b"v"));
        bytes.push(0); // sub block: no sub-metadata
        bytes.push(0xAA); // sentinel past the section

        let mut reader = Reader::new(&bytes);
        decode_metadata(&mut reader).expect("well-formed metadata parses");
        assert_eq!(reader.read_u8().unwrap(), 0xAA);
    }

    #[test]
    fn empty_metadata_is_three_zero_counts() {
        let bytes = [0u8, 0, 0, 0xAA];
        let mut reader = Reader::new(&bytes);
        decode_metadata(&mut reader).expect("empty metadata parses");
        assert_eq!(reader.read_u8().unwrap(), 0xAA);
    }

    #[test]
    fn empty_entry_value_is_rejected() {
        // Geometry metadata with one entry whose value length is zero.
        let bytes = [0u8, 1, 1, b'k', 0];
        let mut reader = Reader::new(&bytes);
        assert!(decode_metadata(&mut reader).is_err());
    }

    #[test]
    fn runaway_nesting_is_rejected() {
        // No attribute blocks; each level has no entries and one anonymous
        // sub-metadata, past the reference's depth cap.
        let mut bytes = vec![0u8];
        for _ in 0..(MAX_SUBMETADATA_LEVEL + 2) {
            bytes.extend([0u8, 1, 0]); // no entries, one sub, empty name
        }
        let mut reader = Reader::new(&bytes);
        assert!(decode_metadata(&mut reader).is_err());
    }
}
