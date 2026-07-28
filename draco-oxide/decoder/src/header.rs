//! Stream header parsing: magic string, version (2.2 only for now), geometry type,
//! encoder method, and flags.

use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::header::EncoderMethod;

const METADATA_FLAG_MASK: u16 = 0x8000;

/// The parsed stream header.
pub struct Header {
    /// Geometry type id (0 = point cloud, 1 = triangular mesh).
    pub geometry_type: u8,
    /// Connectivity encoder method.
    pub encoder_method: EncoderMethod,
    /// Whether the metadata flag is set (the encoder then writes a stub section).
    pub metadata: bool,
}

/// Parses the fixed 13-byte draco header. Accepts only bitstream version 2.2.
pub fn decode_header(reader: &mut Reader<'_>) -> Result<Header, Err> {
    let mut magic = [0u8; 5];
    for b in &mut magic {
        *b = reader.read_u8()?;
    }
    if &magic != b"DRACO" {
        return Err(Err::InvalidHeader("missing DRACO magic"));
    }

    let major = reader.read_u8()?;
    let minor = reader.read_u8()?;
    if (major, minor) != (2, 2) {
        return Err(Err::UnsupportedVersion(major, minor));
    }

    let geometry_type = reader.read_u8()?;

    let method_id = reader.read_u8()?;
    let encoder_method = match method_id {
        0 => EncoderMethod::Sequential,
        1 => EncoderMethod::Edgebreaker,
        _ => return Err(Err::InvalidHeader("unknown encoder method")),
    };

    let flags = reader.read_u16()?;
    let metadata = flags & METADATA_FLAG_MASK != 0;

    Ok(Header {
        geometry_type,
        encoder_method,
        metadata,
    })
}
