//! `draco-oxide-decoder` decompresses draco-encoded geometry into the
//! [`draco_oxide_core`] mesh model.
//!
//! The public surface has two tiers. [`decode_portable`] is always compiled and
//! returns a [`PortableMesh`]: a mesh whose attributes are still quantized
//! integers, paired with the per-attribute [`AttributeTransform`] parameters a
//! consumer (e.g. a GPU shader) needs to reconstruct floats itself. [`decode`],
//! gated behind the default `dequantize` feature, additionally applies those
//! transforms and returns a [`Mesh`] with original-format (float) attributes.
//!
//! Both entry points currently return [`Err::Unimplemented`]; the implementation
//! is landing milestone by milestone (see `DECODER_PLAN`).

use draco_oxide_core::bit_coder::{ByteReader, ReaderErr};
use draco_oxide_core::mesh::Mesh;

mod attribute;
mod connectivity;
pub mod entropy;
mod header;
mod metadata;
#[cfg(feature = "simd")]
mod simd;

/// Errors produced while decoding a draco stream.
#[remain::sorted]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Err {
    /// A shared entropy-coding error (frequency table, symbol method, etc.).
    #[error("entropy error: {0}")]
    Entropy(#[from] draco_oxide_core::codec::entropy::Err),

    /// A symbol bit-length byte outside the valid 1..=18 range.
    #[error("invalid symbol bit length: {0}")]
    InvalidBitLength(u8),

    /// A byte reader ran out of data or otherwise failed.
    #[error("reader error: {0}")]
    Reader(#[from] ReaderErr),

    /// The requested decode path is not implemented yet.
    #[error("decoder functionality not yet implemented")]
    Unimplemented,
}

/// A decoded mesh whose attributes are still in their portable, quantized-integer
/// form, together with the transform parameters needed to reconstruct the
/// original float values.
pub struct PortableMesh {
    /// The mesh with integer-typed attributes.
    pub mesh: Mesh,
    /// The dequantization parameters for each attribute, in attribute order.
    pub transforms: Vec<AttributeTransform>,
}

/// Per-attribute parameters for reconstructing original-format values from the
/// portable integer representation.
pub enum AttributeTransform {
    /// Uniform quantization over an axis-aligned box.
    Quantized {
        /// Minimum value per component.
        min: Vec<f32>,
        /// Extent of the largest component range.
        delta_max: f32,
        /// Number of quantization bits.
        bits: u8,
    },
    /// Octahedral encoding of unit vectors (normals).
    Octahedral {
        /// Number of quantization bits.
        bits: u8,
    },
    /// No transform; values are already in their original format.
    None,
}

/// Decode a draco stream into a [`PortableMesh`] with quantized-integer
/// attributes. Always available, on every target.
pub fn decode_portable<R: ByteReader>(_reader: R) -> Result<PortableMesh, Err> {
    Err(Err::Unimplemented)
}

/// Decode a draco stream into a [`Mesh`] with original-format (float) attributes.
///
/// Equivalent to [`decode_portable`] followed by applying each
/// [`AttributeTransform`].
#[cfg(feature = "dequantize")]
pub fn decode<R: ByteReader>(_reader: R) -> Result<Mesh, Err> {
    Err(Err::Unimplemented)
}
