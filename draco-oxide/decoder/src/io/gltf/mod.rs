//! glTF / GLB support for the decoder: parse a GLB container, find
//! `KHR_draco_mesh_compression` primitives, and decode them — or splice the
//! Draco extension out entirely, producing a plain (uncompressed) GLB.
//!
//! `glb` and `draco_extension` are copied from the encoder crate's `io::gltf`.
//! They are self-contained (GLB byte parsing + serde_json extension helpers);
//! a future cleanup can hoist the shared parts into `draco-oxide-core`.

pub mod draco_decoder;
pub mod draco_extension;
pub mod glb;
