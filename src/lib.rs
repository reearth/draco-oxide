// lib.rs

/// Contains the interface between obj, stl, gltf and 'Mesh' object.
pub mod io;

/// Contains compression techniques used by the encoder and the decoder.
pub mod compression;

/// Defines the encoders.
pub mod encode;

/// Defines the decoders.
pub mod decode;

/// Contains the shared definitions, native objects, and the buffer.
pub mod core;

mod utils;
mod tests;