//! Attribute decoding: framing (encoder count, domains, descriptors) and the driver
//! that sequences, reverses prediction, inverts transforms, and (with `dequantize`)
//! reconstructs original-format values.

mod inverse_transform;
mod prediction;
mod sequence;

#[cfg(feature = "dequantize")]
mod dequantize;
