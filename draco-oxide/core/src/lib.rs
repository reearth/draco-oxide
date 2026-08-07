//! `draco-oxide-core` holds the parts shared by both the draco-oxide encoder and
//! decoder: the geometry/attribute data model, the numeric primitives
//! (`NdVector`, index types, numeric traits), and the compression algorithms
//! shared between encode and decode (`codec`).

/// Mesh attributes: typed per-point or per-corner value arrays and their metadata.
pub mod attribute;
/// Byte- and bit-level readers and writers for the draco stream format.
pub mod bit_coder;
/// Type-erased storage buffers backing attribute data.
pub mod buffer;
/// The mesh data model: faces plus attributes, the mesh builder, and connectivity structures.
pub mod mesh;
/// The point cloud data model: per-point attributes with no connectivity.
pub mod point_cloud;
/// Numeric primitives: `NdVector`, typed index newtypes, and numeric traits.
pub mod types;

/// Compression algorithms shared between the encoder and decoder
/// (entropy coding, prediction schemes, connectivity, headers).
pub mod codec;

/// Shared helpers: geometry math, variable-length integer coding, and debug utilities.
pub mod utils;
