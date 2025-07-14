/// Texture I/O functionality for reading and writing texture data.
/// Based on the Draco C++ implementation in draco/io/texture_io.h

use crate::core::texture::{Texture, ImageFormat};
use std::fs;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("IO Error: {0}")]
    IoError(String),
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    #[error("Decode Error: {0}")]
    DecodeError(String),
}




/// Writes a texture into a buffer.
pub(crate) fn write_texture_to_buffer(texture: &Texture, buffer: &mut Vec<u8>) -> Result<(), Err> {
    let source_image = texture.get_source_image();
    
    // If we have encoded data, use it directly
    if !source_image.get_encoded_data().is_empty() {
        buffer.extend_from_slice(source_image.get_encoded_data());
        return Ok(());
    }
    
    // If we have a filename but no encoded data, try to read from file
    if !source_image.get_filename().is_empty() {
        let file_data = fs::read(source_image.get_filename()).map_err(|e| {
            Err::IoError(format!("Failed to read texture file '{}': {}", source_image.get_filename(), e))
        })?;
        buffer.extend_from_slice(&file_data);
        return Ok(());
    }
    
    Err(Err::InvalidFormat("Texture has no encoded data or filename".to_string()))
}

/// Returns the image format of an encoded texture stored in buffer.
/// ImageFormat::None is returned for unknown image formats.
pub fn image_format_from_buffer(buffer: &[u8]) -> ImageFormat {
    if buffer.len() > 4 {
        // JPEG markers
        let jpeg_soi_marker = [0xFF, 0xD8];
        let jpeg_eoi_marker = [0xFF, 0xD9];
        
        if buffer.starts_with(&jpeg_soi_marker) {
            // Look for the end marker (allow trailing bytes)
            if buffer.windows(2).any(|window| window == jpeg_eoi_marker) {
                return ImageFormat::Jpeg;
            }
        }
    }

    if buffer.len() > 2 {
        // Basis format signature 'B' * 256 + 's', or 0x4273
        let basis_signature = [0x42, 0x73];
        if buffer.starts_with(&basis_signature) {
            return ImageFormat::Basis;
        }
    }

    if buffer.len() > 4 {
        // KTX2 format signature 0xab 0x4b 0x54 0x58
        let ktx2_signature = [0xab, 0x4b, 0x54, 0x58];
        if buffer.starts_with(&ktx2_signature) {
            return ImageFormat::Basis;
        }
    }

    if buffer.len() > 8 {
        // PNG signature
        let png_signature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        if buffer.starts_with(&png_signature) {
            return ImageFormat::Png;
        }
    }

    if buffer.len() > 12 {
        // WebP signature: RIFF followed by size (4 bytes) then WEBP
        let riff = [0x52, 0x49, 0x46, 0x46];
        let webp = [0x57, 0x45, 0x42, 0x50];
        
        if buffer.starts_with(&riff) && buffer.len() > 8 && &buffer[8..12] == webp {
            return ImageFormat::Webp;
        }
    }

    ImageFormat::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_format_detection() {
        // Test PNG
        let png_data = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00];
        assert_eq!(image_format_from_buffer(&png_data), ImageFormat::Png);
        
        // Test JPEG
        let jpeg_data = [0xFF, 0xD8, 0x00, 0x00, 0xFF, 0xD9];
        assert_eq!(image_format_from_buffer(&jpeg_data), ImageFormat::Jpeg);
        
        // Test unknown format
        let unknown_data = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(image_format_from_buffer(&unknown_data), ImageFormat::None);
    }
}