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

// The decoder crate (`draco-oxide-decoder`) is not published to crates.io yet, so
// `draco-oxide` cannot depend on it while it is published. Once the decoder is
// published, re-add the optional dep + `decoder` feature and restore:
//
//     #[cfg(feature = "decoder")]
//     pub use draco_oxide_decoder::decode;

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
