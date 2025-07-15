// lib.rs

/// Contains the interface between `Mesh` object and 3D geometry files
/// such as obj and gltf.
pub mod io;

/// Contains compression techniques used by the encoder and the decoder.
pub(crate) mod shared;

/// Defines the mesh encoder.
pub mod encode;

// /// Defines the decoders.
// pub mod decode;

/// Contains the shared definitions, native objects, and the buffer.
pub mod core;

/// Contains the macros used by the encoder and the decoder.
pub(crate) mod utils;


/// Contains the most commonly used traits, types, and objects.
pub mod prelude {
    pub use crate::core::attribute::{Attribute, AttributeType};
    pub use crate::core::mesh::{Mesh, builder::MeshBuilder};
    pub use crate::core::shared::{NdVector, Vector, DataValue};
    pub use crate::core::shared::ConfigType;
    pub use crate::core::bit_coder::{
        ByteReader, 
        ByteWriter, 
        FunctionalByteReader, 
        FunctionalByteWriter
    };
    pub use crate::encode::{self, encode};
    // pub use crate::decode::{self, decode};
}


/// Evaluation module contains the evaluation functions for the encoder and the decoder.
/// When enabled, draco-oxide encoder will spit out the evaluation data mixed with encoded data,
/// and then the `EvalWriter` is used to filter out the evaluation data. This functionality is
/// most often used in the development and testing phase.
#[cfg(any(test, feature = "evaluation"))]
pub mod eval;