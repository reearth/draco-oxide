//! `draco-oxide` is a high-performance Draco codec written in pure Rust,
//! targeting the current Draco bitstreams: 2.2 for triangular meshes and 2.3
//! for point clouds.
//!
//! [`encode::Encoder`] compresses a [`Mesh`] or a [`PointCloud`] into a Draco
//! stream under an [`encode::Config`] or [`encode::PointCloudConfig`]; the free
//! [`encode::encode_mesh`] and [`encode::encode_point_cloud`] functions are
//! one-shot wrappers. Behind the default `decoder` feature, the decoder crate
//! is re-exported as [`decode`]. The [`io`] module loads and writes OBJ files
//! and transcodes glTF/GLB assets with Draco-compressed mesh primitives.
//!
//! Streams are interoperable with the reference C++ implementation in both
//! directions.
//!
//! Decode-only consumers (e.g. WASM viewers) should depend on the
//! `draco-oxide-decoder` crate directly, which never links the encoder.

/// Re-export of the shared core crate (`draco-oxide-core`): the geometry/attribute
/// data model, numeric primitives (`core::types`), and the codec algorithms shared
/// between encoder and decoder (`core::codec`). Reachable as `draco_oxide::core`.
pub use draco_oxide_core as core;

// Re-export the core data-model types a caller needs to drive the encoder, so
// depending on `draco-oxide` alone is enough, no separate `draco-oxide-core`
// import just to name a `Mesh`, build one, or reach `Config::default()`. The full
// surface remains available under `draco_oxide::core`.

/// The geometry container the encoder consumes, and the builder used to
/// assemble one.
///
/// Everything needed to drive the encoder is reachable from `draco_oxide` alone:
///
/// ```
/// use draco_oxide::{
///     Attribute, AttributeDomain, AttributeType, ComponentDataType, ConfigType, Mesh,
///     MeshBuilder, NdVector,
/// };
/// use draco_oxide::encode::Config;
///
/// let _builder = MeshBuilder::new();
/// let _cfg = <Config as ConfigType>::default();
/// let _ty = AttributeType::Position;
/// let _dom = AttributeDomain::Position;
/// let _ct = ComponentDataType::F32;
/// fn _drives(_m: Mesh, _a: Attribute, _v: NdVector<3, f32>) {}
/// ```
pub use draco_oxide_core::mesh::{builder::MeshBuilder, Mesh};

/// The point cloud container the encoder consumes.
pub use draco_oxide_core::point_cloud::PointCloud;

/// The attribute data model: a vertex attribute and the enums describing it.
pub use draco_oxide_core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType,
};

/// Numeric primitive for attribute values, and the trait exposing `default()` on
/// the encoder configs.
pub use draco_oxide_core::types::{ConfigType, NdVector};

/// Contains the interface between `Mesh` object and 3D geometry files
/// such as obj and gltf.
pub mod io;

/// Defines the mesh encoder.
pub mod encode;

/// The decoder crate (`draco-oxide-decoder`), re-exported behind the default
/// `decoder` feature. Reachable as `draco_oxide::decode`.
#[cfg(feature = "decoder")]
pub use draco_oxide_decoder as decode;

// White-box tests over crate-private encoder internals (data structures,
// traversal, entropy, connectivity); black-box integration tests live in the
// `tests` crate.
#[cfg(test)]
mod white_box_tests;
