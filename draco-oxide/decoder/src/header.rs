//! Mesh stream header parsing: magic string, version, geometry type, encoder
//! method, and flags. Point-cloud headers are parsed by the point-cloud decoder.

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

/// Parses the draco header of a mesh stream. Accepts only bitstream 2.2.
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
    let geometry_type = reader.read_u8()?;

    // Point clouds are bitstream 2.3, so a version-first check would report
    // every point-cloud file as a bad version instead.
    if major == 2 && geometry_type == 0 {
        return Err(Err::UnsupportedPointCloud);
    }
    if (major, minor) != (2, 2) {
        return Err(Err::UnsupportedVersion(major, minor));
    }
    if geometry_type != 1 {
        return Err(Err::InvalidHeader("unknown geometry type"));
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_cloud_stream_is_rejected() {
        // A bitstream-2.2 header declaring geometry type 0 (point cloud).
        let bytes = [b'D', b'R', b'A', b'C', b'O', 2, 2, 0, 0, 0, 0];
        let mut reader = Reader::new(&bytes);
        assert!(matches!(
            decode_header(&mut reader),
            Err(Err::UnsupportedPointCloud)
        ));
    }

    #[test]
    fn point_cloud_bitstream_2_3_is_rejected_as_point_cloud() {
        // Real point-cloud files are bitstream 2.3; the point-cloud diagnosis
        // must win over the version rejection.
        let bytes = [b'D', b'R', b'A', b'C', b'O', 2, 3, 0, 0, 0, 0];
        let mut reader = Reader::new(&bytes);
        assert!(matches!(
            decode_header(&mut reader),
            Err(Err::UnsupportedPointCloud)
        ));
    }
}
