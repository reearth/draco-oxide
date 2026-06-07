// lib.rs

/// Re-export of the shared core crate (`draco-oxide-core`): the geometry/attribute
/// data model, numeric primitives (`core::types`), and the codec algorithms shared
/// between encoder and decoder (`core::codec`). Reachable as `draco_oxide::core`.
pub use draco_oxide_core as core;

/// Contains the interface between `Mesh` object and 3D geometry files
/// such as obj and gltf.
pub mod io;

/// Defines the mesh encoder.
pub mod encode;

// /// Defines the decoders.
// pub mod decode;

/// Evaluation module contains the evaluation functions for the encoder and the decoder.
/// When enabled, draco-oxide encoder will spit out the evaluation data mixed with encoded data,
/// and then the `EvalWriter` is used to filter out the evaluation data. This functionality is
/// most often used in the development and testing phase.
#[cfg(feature = "evaluation")]
pub mod eval;
