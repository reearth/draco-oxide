//! GLB (Binary glTF) parsing and writing utilities.
//!
//! GLB file format:
//! - Header (12 bytes): magic (4) + version (4) + length (4)
//! - JSON chunk: length (4) + type "JSON" (4) + data (padded to 4 bytes with spaces)
//! - BIN chunk (optional): length (4) + type "BIN\0" (4) + data (padded to 4 bytes with zeros)

use std::io::Write;

const GLB_MAGIC: &[u8; 4] = b"glTF";
const GLB_VERSION: u32 = 2;
const CHUNK_TYPE_JSON: &[u8; 4] = b"JSON";
const CHUNK_TYPE_BIN: &[u8; 4] = b"BIN\0";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid GLB magic bytes")]
    InvalidMagic,
    #[error("Unsupported GLB version: {0}")]
    UnsupportedVersion(u32),
    #[error("GLB file too short: expected at least {expected} bytes, got {actual}")]
    FileTooShort { expected: usize, actual: usize },
    #[error("Invalid chunk type at offset {offset}")]
    InvalidChunkType { offset: usize },
    #[error("Missing JSON chunk")]
    MissingJsonChunk,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Parsed GLB file contents.
#[derive(Debug)]
pub struct GlbData {
    /// JSON chunk content (UTF-8 encoded glTF JSON).
    pub json: Vec<u8>,
    /// Binary buffer chunk content (may be empty if no BIN chunk).
    pub buffer: Vec<u8>,
}

/// Parse a GLB file from bytes.
pub fn parse_glb(data: &[u8]) -> Result<GlbData, Error> {
    if data.len() < 20 {
        return Err(Error::FileTooShort {
            expected: 20,
            actual: data.len(),
        });
    }

    let magic = &data[0..4];
    if magic != GLB_MAGIC {
        return Err(Error::InvalidMagic);
    }

    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if version != GLB_VERSION {
        return Err(Error::UnsupportedVersion(version));
    }

    let total_length = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    if data.len() < total_length {
        return Err(Error::FileTooShort {
            expected: total_length,
            actual: data.len(),
        });
    }

    let mut offset = 12;
    let mut json_data: Option<Vec<u8>> = None;
    let mut bin_data: Vec<u8> = Vec::new();

    while offset + 8 <= total_length {
        let chunk_length = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let chunk_type = &data[offset + 4..offset + 8];

        let chunk_data_start = offset + 8;
        let chunk_data_end = chunk_data_start + chunk_length;

        if chunk_data_end > total_length {
            break;
        }

        if chunk_type == CHUNK_TYPE_JSON {
            let mut json = data[chunk_data_start..chunk_data_end].to_vec();
            while json.last() == Some(&b' ') {
                json.pop();
            }
            json_data = Some(json);
        } else if chunk_type == CHUNK_TYPE_BIN {
            bin_data = data[chunk_data_start..chunk_data_end].to_vec();
        }

        offset = chunk_data_end;
    }

    let json = json_data.ok_or(Error::MissingJsonChunk)?;
    Ok(GlbData {
        json,
        buffer: bin_data,
    })
}

/// Write GLB format to a writer.
pub fn write_glb<W: Write>(writer: &mut W, json: &[u8], buffer: &[u8]) -> Result<(), Error> {
    let json_padded_len = (json.len() + 3) & !3;
    let buffer_padded_len = if buffer.is_empty() {
        0
    } else {
        (buffer.len() + 3) & !3
    };

    let total_length = 12
        + 8
        + json_padded_len
        + if buffer_padded_len > 0 {
            8 + buffer_padded_len
        } else {
            0
        };

    writer.write_all(GLB_MAGIC)?;
    writer.write_all(&GLB_VERSION.to_le_bytes())?;
    writer.write_all(&(total_length as u32).to_le_bytes())?;

    writer.write_all(&(json_padded_len as u32).to_le_bytes())?;
    writer.write_all(CHUNK_TYPE_JSON)?;
    writer.write_all(json)?;
    for _ in json.len()..json_padded_len {
        writer.write_all(b" ")?;
    }

    if buffer_padded_len > 0 {
        writer.write_all(&(buffer_padded_len as u32).to_le_bytes())?;
        writer.write_all(CHUNK_TYPE_BIN)?;
        writer.write_all(buffer)?;
        for _ in buffer.len()..buffer_padded_len {
            writer.write_all(&[0u8])?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let json = br#"{"asset":{"version":"2.0"}}"#;
        let buffer = b"hello world binary data";

        let mut glb = Vec::new();
        write_glb(&mut glb, json, buffer).unwrap();

        let parsed = parse_glb(&glb).unwrap();
        assert_eq!(parsed.json, json);
        assert!(parsed.buffer.starts_with(buffer));
    }

    #[test]
    fn test_empty_buffer() {
        let json = br#"{"asset":{"version":"2.0"}}"#;

        let mut glb = Vec::new();
        write_glb(&mut glb, json, &[]).unwrap();

        let parsed = parse_glb(&glb).unwrap();
        assert_eq!(parsed.json, json);
        assert!(parsed.buffer.is_empty());
    }
}
