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
use draco_oxide_core::bit_coder::{Reader, ReaderErr};
use draco_oxide_core::mesh::Mesh;

mod attribute;
pub mod connectivity;
pub mod entropy;
pub mod header;
mod metadata;
mod reader;
#[cfg(feature = "simd")]
mod simd;

/// Errors produced while decoding a draco stream.
#[remain::sorted]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Err {
    /// An attribute framing field failed to parse.
    #[error("attribute framing error: {0}")]
    Attribute(#[from] draco_oxide_core::attribute::Err),

    /// A shared entropy-coding error (frequency table, symbol method, etc.).
    #[error("entropy error: {0}")]
    Entropy(#[from] draco_oxide_core::codec::entropy::Err),

    /// A symbol bit-length byte outside the valid 1..=18 range.
    #[error("invalid symbol bit length: {0}")]
    InvalidBitLength(u8),

    /// The stream header is malformed (bad magic or field).
    #[error("invalid header: {0}")]
    InvalidHeader(&'static str),

    /// The attribute section is inconsistent (bad descriptor or payload).
    #[error("malformed attribute section: {0}")]
    MalformedAttribute(&'static str),

    /// The connectivity stream is inconsistent (bad symbol/split data).
    #[error("malformed connectivity: {0}")]
    MalformedConnectivity(&'static str),

    /// A byte reader ran out of data or otherwise failed.
    #[error("reader error: {0}")]
    Reader(#[from] ReaderErr),

    /// The requested decode path is not implemented yet.
    #[error("decoder functionality not yet implemented")]
    Unimplemented,

    /// The bitstream version is not supported (only 2.2 for now).
    #[error("unsupported bitstream version: {0}.{1}")]
    UnsupportedVersion(u8, u8),
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
pub fn decode_portable(bytes: &[u8]) -> Result<PortableMesh, Err> {
    let mut reader = Reader::new(bytes);
    let header = header::decode_header(&mut reader)?;
    if header.metadata {
        metadata::decode_metadata(&mut reader)?;
    }
    let conn = connectivity::decode_connectivity(&mut reader, header.encoder_method)?;
    let decoded = attribute::decode_attributes(&mut reader, &conn)?;

    let mut mesh = Mesh::new();
    mesh.faces = decoded.faces;
    mesh.attributes = decoded.attributes;
    Ok(PortableMesh {
        mesh,
        transforms: decoded.transforms,
    })
}

/// Decode a draco stream into a [`Mesh`] with original-format (float) attributes.
///
/// Equivalent to [`decode_portable`] followed by applying each
/// [`AttributeTransform`].
#[cfg(feature = "dequantize")]
pub fn decode(bytes: &[u8]) -> Result<Mesh, Err> {
    let PortableMesh {
        mut mesh,
        transforms,
    } = decode_portable(bytes)?;
    let attributes = std::mem::take(&mut mesh.attributes);
    mesh.attributes = attributes
        .into_iter()
        .zip(&transforms)
        .map(|(att, transform)| attribute::dequantize::dequantize_attribute(att, transform))
        .collect::<Result<_, _>>()?;
    Ok(mesh)
}
