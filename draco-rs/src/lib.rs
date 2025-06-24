// lib.rs

/// Contains the interface between obj, stl, gltf and 'Mesh' object.
pub mod io;

/// Contains compression techniques used by the encoder and the decoder.
pub mod shared;

/// Defines the encoders.
pub mod encode;

/// Defines the decoders.
pub mod decode;

/// Contains the shared definitions, native objects, and the buffer.
pub mod core;

pub mod utils;
mod tests;

pub use crate::core::buffer::Buffer;
pub use crate::core::mesh::{
    Mesh,
    builder::MeshBuilder,
};

pub mod prelude {
    pub use crate::core::attribute::{Attribute, AttributeType};
    pub use crate::core::buffer::{self, Buffer};
    pub use crate::core::mesh::{Mesh, builder::MeshBuilder};
    pub use crate::core::shared::{NdVector, Vector};
    pub use crate::core::shared::ConfigType;
    pub use crate::core::bit_coder::{
        BitReader, 
        BitWriter, 
        ByteReader, 
        ByteWriter, 
        FunctionalByteReader, 
        FunctionalByteWriter
    };
    pub use crate::encode::{self, encode};
    // pub use crate::decode::{self, decode};
}

#[cfg(any(test, feature = "evaluation"))]
pub mod eval;