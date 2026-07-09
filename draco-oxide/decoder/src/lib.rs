//! `draco-oxide-decoder` decompresses draco-encoded geometry into the
//! [`draco_oxide_core`] mesh model.

pub mod decode;

/// glTF / GLB `KHR_draco_mesh_compression` decode helpers. Behind the `gltf`
/// feature so the core decode path (e.g. the wasm build) stays free of the
/// serde_json dependency.
#[cfg(feature = "gltf")]
pub mod io;

/// Convenience re-exports for the decoder. Mirrors the symbols the `decode`
/// tree referenced via `crate::prelude` in the pre-split monolith, remapped
/// onto [`draco_oxide_core`].
pub mod prelude {
    pub use draco_oxide_core::attribute::ComponentDataType;
    pub use draco_oxide_core::attribute::{Attribute, AttributeDomain, AttributeType};
    pub use draco_oxide_core::bit_coder::{
        ByteReader, ByteWriter, FunctionalByteReader, FunctionalByteWriter,
    };
    pub use draco_oxide_core::mesh::{builder::MeshBuilder, Mesh};
    pub use draco_oxide_core::types::ConfigType;
    pub use draco_oxide_core::types::{DataValue, NdVector, PointIdx, Vector};

    pub use crate::decode::{
        self, decode, decode_to_raw, decode_to_raw_with_warnings, decode_with_warnings,
        DecodeWarning, DecodedRaw, RawAttribute,
    };
}
