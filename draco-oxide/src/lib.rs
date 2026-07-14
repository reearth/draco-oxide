// lib.rs

/// Re-export of the shared core crate (`draco-oxide-core`): the geometry/attribute
/// data model, numeric primitives (`core::types`), and the codec algorithms shared
/// between encoder and decoder (`core::codec`). Reachable as `draco_oxide::core`.
pub use draco_oxide_core as core;

// Re-export the core data-model types a caller needs to drive the encoder, so
// depending on `draco-oxide` alone is enough — no separate `draco-oxide-core`
// import just to name a `Mesh`, build one, or reach `Config::default()`. The full
// surface remains available under `draco_oxide::core`.

/// The geometry container consumed by [`encode`](encode::encode), and the builder
/// used to assemble one.
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

/// Cross-crate round-trip / integration tests relocated from `draco-oxide-core`
/// and `draco-oxide-decoder` (they need the encoder + io + decoder together).
#[cfg(test)]
mod roundtrip_tests;

/// Evaluation module contains the evaluation functions for the encoder and the decoder.
/// When enabled, draco-oxide encoder will spit out the evaluation data mixed with encoded data,
/// and then the `EvalWriter` is used to filter out the evaluation data. This functionality is
/// most often used in the development and testing phase.
#[cfg(feature = "evaluation")]
pub mod eval;
