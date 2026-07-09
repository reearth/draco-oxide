//! Sequential mesh connectivity decode (Google `MeshSequentialDecoder`).
//!
//! Sequential meshes store the triangle list directly (no edgebreaker /
//! corner table). Two encodings: method 0 = entropy-coded delta of the
//! flattened index list; method 1 = raw fixed-width indices (width chosen by
//! the point count). Attributes are then decoded in plain point order (see
//! `decode::attribute::decode_attributes_sequential`).

use crate::decode::entropy::symbol_coding;
use crate::decode::header::Header;
use crate::prelude::ByteReader;
use draco_oxide_core::utils::bit_coder::leb128_read;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Reader error: {0}")]
    Reader(#[from] draco_oxide_core::bit_coder::ReaderErr),
    #[error("Symbol coding error: {0}")]
    SymbolCoding(#[from] symbol_coding::Err),
    #[error("Invalid sequential connectivity method: {0}")]
    InvalidMethod(u8),
    #[error("Sequential index {idx} out of range (num_points={num_points})")]
    IndexOutOfRange { idx: u32, num_points: usize },
    #[error("Sequential index delta would underflow/overflow")]
    IndexDeltaInvalid,
}

/// Decoded sequential connectivity: the flat triangle list (point indices)
/// and the point count.
pub(crate) struct SequentialConnectivity {
    pub faces: Vec<[u32; 3]>,
    pub num_points: usize,
}

pub(crate) fn decode<R: ByteReader>(
    reader: &mut R,
    header: &Header,
) -> Result<SequentialConnectivity, Err> {
    // v2.2+ writes counts as varints (older used u32; we target 2.2 like the
    // rest of the decoder).
    let version = ((header.version_major as u16) << 8) | header.version_minor as u16;
    let (num_faces, num_points) = if version < 0x0202 {
        (reader.read_u32()? as usize, reader.read_u32()? as usize)
    } else {
        (leb128_read(reader)? as usize, leb128_read(reader)? as usize)
    };

    let method = reader.read_u8()?;
    let mut faces: Vec<[u32; 3]> = Vec::with_capacity(num_faces);

    if method == 0 {
        // Entropy-coded, delta of the flattened index stream.
        let symbols = symbol_coding::decode_symbols(num_faces * 3, 1, reader)?;
        let mut last: i64 = 0;
        let mut k = 0usize;
        for _ in 0..num_faces {
            let mut face = [0u32; 3];
            for f in face.iter_mut() {
                let encoded = symbols[k] as i64;
                k += 1;
                let mut diff = encoded >> 1;
                if encoded & 1 != 0 {
                    if diff > last {
                        return Err(Err::IndexDeltaInvalid);
                    }
                    diff = -diff;
                }
                let value = diff + last;
                if value < 0 || value as usize >= num_points {
                    return Err(Err::IndexOutOfRange {
                        idx: value.max(0) as u32,
                        num_points,
                    });
                }
                *f = value as u32;
                last = value;
            }
            faces.push(face);
        }
    } else if method == 1 {
        // Raw fixed-width indices; width depends on the point count.
        let read_index = |reader: &mut R| -> Result<u32, Err> {
            let v = if num_points < 256 {
                reader.read_u8()? as u32
            } else if num_points < (1 << 16) {
                reader.read_u16()? as u32
            } else if num_points < (1 << 21) && version >= 0x0202 {
                leb128_read(reader)? as u32
            } else {
                reader.read_u32()?
            };
            if v as usize >= num_points {
                return Err(Err::IndexOutOfRange {
                    idx: v,
                    num_points,
                });
            }
            Ok(v)
        };
        for _ in 0..num_faces {
            faces.push([read_index(reader)?, read_index(reader)?, read_index(reader)?]);
        }
    } else {
        return Err(Err::InvalidMethod(method));
    }

    Ok(SequentialConnectivity { faces, num_points })
}
