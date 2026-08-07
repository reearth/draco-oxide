//! `draco-oxide-decoder` decodes Draco streams into the [`draco_oxide_core`]
//! geometry model: triangular meshes on bitstream 2.2, and, behind the
//! `point-cloud` feature, kd-tree point clouds on bitstream 2.3.
//!
//! [`Decoder`] is the entry point, and the free functions are one-shot wrappers
//! around it. The generic [`decode`] returns the [`Geometry`] the stream
//! declares; the typed entry points are the convenience for callers that
//! already know which they have.
//!
//! Decoding has two tiers. The `_portable` entry points are always compiled and
//! return integer attributes paired with the [`AttributeTransform`] parameters
//! a consumer (e.g. a GPU shader) needs to reconstruct floats itself. The
//! dequantized entry points, behind the default `dequantize` feature, apply
//! those transforms and return original-format attributes.

use draco_oxide_core::bit_coder::{Reader, ReaderErr};
use draco_oxide_core::mesh::Mesh;
#[cfg(feature = "point-cloud")]
pub use draco_oxide_core::point_cloud::PointCloud;

mod attribute;
/// Connectivity decoding. Exposes decoder internals for advanced consumers.
pub mod connectivity;
/// Entropy decoding. Exposes decoder internals for advanced consumers.
pub mod entropy;
/// Stream header parsing. Exposes decoder internals for advanced consumers.
pub mod header;
mod metadata;
#[cfg(feature = "point-cloud")]
mod point_cloud;
mod reader;

/// Errors produced while decoding a draco stream.
#[remain::sorted]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Err {
    /// The stream declares more data than could be allocated.
    #[error("stream declares more data than can be allocated: {0}")]
    AllocationTooLarge(&'static str),

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

    /// The metadata section is inconsistent.
    #[error("malformed metadata: {0}")]
    MalformedMetadata(&'static str),

    /// A byte reader ran out of data or otherwise failed.
    #[error("reader error: {0}")]
    Reader(#[from] ReaderErr),

    /// The requested decode path is not implemented yet.
    #[error("decoder functionality not yet implemented")]
    Unimplemented,

    /// A point-cloud stream reached a mesh entry point, or the `point-cloud`
    /// feature is disabled.
    #[error("point cloud streams are not supported here")]
    UnsupportedPointCloud,

    /// The bitstream version is not supported.
    #[error("unsupported bitstream version: {0}.{1}")]
    UnsupportedVersion(u8, u8),
}

/// Rejects component types this build cannot decode: 64-bit types always (no
/// compressed representation is implemented), and the rarely used i8/i16/i32
/// unless the `rare-component-types` feature is on.
pub(crate) fn check_component_type(
    ty: draco_oxide_core::attribute::ComponentDataType,
) -> Result<(), Err> {
    use draco_oxide_core::attribute::ComponentDataType as C;
    match ty {
        C::I64 | C::U64 | C::F64 => Err(Err::Unimplemented),
        #[cfg(not(feature = "rare-component-types"))]
        C::I8 | C::I16 | C::I32 => Err(Err::Unimplemented),
        _ => Ok(()),
    }
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

/// A decoded point cloud whose attributes are still in their portable,
/// integer form, together with the transform parameters needed to reconstruct
/// the original values.
#[cfg(feature = "point-cloud")]
pub struct PortablePointCloud {
    /// The point cloud with integer-typed attributes.
    pub point_cloud: PointCloud,
    /// The reconstruction parameters for each attribute, in attribute order.
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
    /// Values stored verbatim by the generic encoder, widened into i32.
    /// `component_type` is what they are read back as.
    Raw {
        /// The attribute's declared component type.
        component_type: draco_oxide_core::attribute::ComponentDataType,
    },
    /// Values carried by the integer codec, widened into the portable i32;
    /// narrowing them to the declared component type restores the original
    /// format.
    Integer {
        /// The attribute's declared component type.
        component_type: draco_oxide_core::attribute::ComponentDataType,
    },
}

/// A decoded geometry, dispatched on the geometry type the stream declares in
/// its header.
#[non_exhaustive]
pub enum Geometry {
    /// A triangular mesh.
    Mesh(Mesh),
    /// A point cloud, produced by kd-tree encoded point-cloud streams.
    #[cfg(feature = "point-cloud")]
    PointCloud(PointCloud),
}

/// The decoder. One instance is meant to be reused across decodes so that
/// resources can be shared between runs.
#[derive(Default)]
pub struct Decoder {}

impl Decoder {
    /// Creates a new decoder.
    pub fn new() -> Self {
        Self {}
    }

    /// Decodes a draco stream into whatever [`Geometry`] it declares, with
    /// original-format (float) attributes.
    #[cfg(feature = "dequantize")]
    pub fn decode(&mut self, bytes: &[u8]) -> Result<Geometry, Err> {
        if is_point_cloud_stream(bytes) {
            #[cfg(feature = "point-cloud")]
            return Ok(Geometry::PointCloud(self.decode_point_cloud(bytes)?));
            #[cfg(not(feature = "point-cloud"))]
            return Err(Err::UnsupportedPointCloud);
        }
        Ok(Geometry::Mesh(self.decode_mesh(bytes)?))
    }

    /// Decodes a draco point-cloud stream into a [`PointCloud`] with
    /// original-format attributes. Only the kd-tree method is supported;
    /// sequential streams fail with [`Err::Unimplemented`].
    ///
    /// Equivalent to [`Self::decode_point_cloud_portable`] followed by applying
    /// each [`AttributeTransform`].
    #[cfg(all(feature = "point-cloud", feature = "dequantize"))]
    pub fn decode_point_cloud(&mut self, bytes: &[u8]) -> Result<PointCloud, Err> {
        point_cloud::decode(bytes)
    }

    /// Decodes a draco point-cloud stream into a [`PortablePointCloud`] with
    /// integer attributes. Always available when the `point-cloud` feature is.
    #[cfg(feature = "point-cloud")]
    pub fn decode_point_cloud_portable(&mut self, bytes: &[u8]) -> Result<PortablePointCloud, Err> {
        let (point_cloud, transforms) = point_cloud::decode_portable(bytes)?;
        Ok(PortablePointCloud {
            point_cloud,
            transforms,
        })
    }

    /// Decodes a draco triangular-mesh stream into a [`Mesh`] with
    /// original-format (float) attributes.
    ///
    /// Equivalent to [`Self::decode_mesh_portable`] followed by applying each
    /// [`AttributeTransform`].
    #[cfg(feature = "dequantize")]
    pub fn decode_mesh(&mut self, bytes: &[u8]) -> Result<Mesh, Err> {
        let PortableMesh {
            mut mesh,
            transforms,
        } = self.decode_mesh_portable(bytes)?;
        let attributes = std::mem::take(&mut mesh.attributes);
        mesh.attributes = attributes
            .into_iter()
            .zip(&transforms)
            .map(|(att, transform)| attribute::dequantize::dequantize_attribute(att, transform))
            .collect::<Result<_, _>>()?;
        Ok(mesh)
    }

    /// Decodes a draco triangular-mesh stream into a [`PortableMesh`] with
    /// quantized-integer attributes. Always available, on every target.
    pub fn decode_mesh_portable(&mut self, bytes: &[u8]) -> Result<PortableMesh, Err> {
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
}

/// Decodes a draco stream into whatever [`Geometry`] it declares, with a
/// freshly constructed [`Decoder`].
#[cfg(feature = "dequantize")]
pub fn decode(bytes: &[u8]) -> Result<Geometry, Err> {
    Decoder::new().decode(bytes)
}

/// Decodes a draco triangular-mesh stream into a [`Mesh`] with original-format
/// (float) attributes, with a freshly constructed [`Decoder`].
#[cfg(feature = "dequantize")]
pub fn decode_mesh(bytes: &[u8]) -> Result<Mesh, Err> {
    Decoder::new().decode_mesh(bytes)
}

/// Decodes a draco triangular-mesh stream into a [`PortableMesh`] with
/// quantized-integer attributes, with a freshly constructed [`Decoder`].
pub fn decode_mesh_portable(bytes: &[u8]) -> Result<PortableMesh, Err> {
    Decoder::new().decode_mesh_portable(bytes)
}

/// Decodes a draco point-cloud stream into a [`PointCloud`], with a freshly
/// constructed [`Decoder`].
#[cfg(all(feature = "point-cloud", feature = "dequantize"))]
pub fn decode_point_cloud(bytes: &[u8]) -> Result<PointCloud, Err> {
    Decoder::new().decode_point_cloud(bytes)
}

/// Decodes a draco point-cloud stream into a [`PortablePointCloud`], with a
/// freshly constructed [`Decoder`].
#[cfg(feature = "point-cloud")]
pub fn decode_point_cloud_portable(bytes: &[u8]) -> Result<PortablePointCloud, Err> {
    Decoder::new().decode_point_cloud_portable(bytes)
}

/// Whether the bytes carry a point-cloud stream (header geometry type 0).
#[cfg(feature = "dequantize")]
fn is_point_cloud_stream(bytes: &[u8]) -> bool {
    bytes.len() >= 11 && &bytes[..5] == b"DRACO" && bytes[7] == 0
}
