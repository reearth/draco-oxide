//! Sequential connectivity decode: face indices stored directly, or as
//! entropy-coded deltas of the flattened index sequence.

use super::SequentialConnectivity;
use crate::entropy::{start_symbol_decoder, AnySymbolDecoder};
use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::types::PointIdx;
use draco_oxide_core::utils::bit_coder::leb128_read;

/// The index-storage method ids on the wire.
const METHOD_COMPRESSED: u8 = 0;
const METHOD_DIRECT: u8 = 1;

/// The point count above which indices are stored as varints rather than fixed
/// 8- or 16-bit words.
const VARINT_THRESHOLD: usize = 1 << 21;

/// Decodes sequential connectivity from `reader`, positioned just after the
/// header (and metadata). Leaves the reader at the start of the attribute
/// section.
pub fn decode(reader: &mut Reader<'_>) -> Result<SequentialConnectivity, Err> {
    let num_faces = leb128_read(reader)? as usize;
    let num_points = leb128_read(reader)? as usize;

    // Three indices per face, so a face count past a third of what is left
    // cannot be honoured however small the index encoding is.
    if num_faces > reader.remaining() / 3 {
        return Err(Err::MalformedConnectivity("face count exceeds the stream"));
    }

    let faces = match reader.read_u8()? {
        METHOD_DIRECT => read_direct_indices(reader, num_faces, num_points)?,
        METHOD_COMPRESSED => read_compressed_indices(reader, num_faces)?,
        _ => {
            return Err(Err::MalformedConnectivity(
                "unknown sequential index storage method",
            ))
        }
    };

    Ok(SequentialConnectivity { faces, num_points })
}

/// Reads face indices stored verbatim, in the smallest width that holds the
/// point space.
fn read_direct_indices(
    reader: &mut Reader<'_>,
    num_faces: usize,
    num_points: usize,
) -> Result<Vec<[PointIdx; 3]>, Err> {
    let mut faces = Vec::with_capacity(num_faces);
    for _ in 0..num_faces {
        let mut face = [PointIdx::from(0); 3];
        for entry in &mut face {
            let idx = if num_points < 256 {
                reader.read_u8()? as usize
            } else if num_points < 1 << 16 {
                reader.read_u16()? as usize
            } else if num_points < VARINT_THRESHOLD {
                leb128_read(reader)? as usize
            } else {
                reader.read_u32()? as usize
            };
            *entry = PointIdx::from(idx);
        }
        faces.push(face);
    }
    Ok(faces)
}

/// Reads face indices entropy-coded as deltas of the flattened index sequence,
/// each delta stored as magnitude with the sign in the low bit.
fn read_compressed_indices(
    reader: &mut Reader<'_>,
    num_faces: usize,
) -> Result<Vec<[PointIdx; 3]>, Err> {
    let num_indices = num_faces * 3;
    let mut decoder = start_symbol_decoder(reader, num_indices, 1)?;

    let mut faces = Vec::with_capacity(num_faces);
    let mut last: i64 = 0;
    for _ in 0..num_faces {
        let mut face = [PointIdx::from(0); 3];
        for entry in &mut face {
            let encoded = match &mut decoder {
                AnySymbolDecoder::Direct(d) => d.decode(),
                AnySymbolDecoder::Tagged(d) => d.decode(),
            };
            let magnitude = (encoded >> 1) as i64;
            last += if encoded & 1 != 0 {
                -magnitude
            } else {
                magnitude
            };
            if last < 0 {
                return Err(Err::MalformedConnectivity("negative face index"));
            }
            *entry = PointIdx::from(last as usize);
        }
        faces.push(face);
    }
    Ok(faces)
}
