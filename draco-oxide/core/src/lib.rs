//! `draco-oxide-core` holds the parts shared by both the draco-oxide encoder and
//! decoder: the geometry/attribute data model, the numeric primitives
//! (`NdVector`, index types, numeric traits), and the compression algorithms
//! shared between encode and decode (`codec`).

pub mod attribute;
pub mod bit_coder;
pub mod buffer;
pub mod corner_table;
pub mod mesh;
pub mod point_cloud;
pub mod point_cloud_builder;
pub mod types;

/// Compression algorithms shared between the encoder and decoder
/// (entropy coding, prediction schemes, connectivity, headers).
pub mod codec;

pub mod utils;
