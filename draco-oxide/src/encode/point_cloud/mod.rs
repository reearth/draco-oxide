//! Point-cloud encoding (bitstream 2.3, kd-tree method).

mod bit_encoder;
mod kd_tree;

use std::collections::HashMap;

use draco_oxide_core::attribute::{Attribute, AttributeType, ComponentDataType};
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::point_cloud::PointCloud;
use draco_oxide_core::types::{ConfigType, NdVector, PointIdx, Vector};
use draco_oxide_core::utils::bit_coder::leb128_write;
use thiserror::Error;

use super::attribute::portabilization::Quantization;

const GEOMETRY_TYPE_POINT_CLOUD: u8 = 0;
const METHOD_KD_TREE: u8 = 1;
const METADATA_FLAG_MASK: u16 = 0x8000;

/// The highest kd-tree compression level the format defines.
pub const MAX_COMPRESSION_LEVEL: u8 = 6;

/// Errors returned while encoding a point cloud.
#[remain::sorted]
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Err {
    /// The entropy coder failed.
    #[error("entropy error: {0}")]
    Entropy(#[from] crate::encode::entropy::rans::Err),
    /// An attribute holds a value that is not finite.
    #[error("attribute {0:?} holds a non-finite value")]
    NonFiniteValue(AttributeType),
    /// The point cloud has no points.
    #[error("the point cloud has no points")]
    NoPoints,
    /// Quantization produced a value outside the range the kd-tree codes.
    #[error("quantized value out of range for attribute {0:?}")]
    QuantizedValueOutOfRange(AttributeType),
    /// The attribute layout cannot be carried by a kd-tree stream.
    #[error("unsupported attribute layout: {0} components of {1:?}")]
    UnsupportedAttributeLayout(usize, ComponentDataType),
    /// A component type the kd-tree method cannot carry.
    #[error("unsupported component type for a point cloud: {0:?}")]
    UnsupportedComponentType(ComponentDataType),
    /// The configured compression level is outside `0..=6`.
    #[error("unsupported kd-tree compression level: {0}")]
    UnsupportedCompressionLevel(u8),
}

/// Point-cloud encoder configuration.
#[derive(Clone, Debug)]
pub struct Config {
    compression_level: u8,
    quantization: Quantization,
    overrides: HashMap<AttributeType, Quantization>,
    metadata: bool,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            compression_level: MAX_COMPRESSION_LEVEL,
            quantization: Quantization::Bits(11),
            overrides: HashMap::new(),
            metadata: false,
        }
    }
}

impl Config {
    /// Sets the kd-tree compression level (`0..=6`). Level 6 also picks the
    /// split axis adaptively.
    pub fn with_compression_level(mut self, level: u8) -> Self {
        self.compression_level = level;
        self
    }

    /// Sets the quantization of every float attribute without an override.
    pub fn with_quantization(mut self, quantization: Quantization) -> Self {
        self.quantization = quantization;
        self
    }

    /// Overrides the quantization of one attribute type.
    pub fn with_attribute_quantization(
        mut self,
        att_type: AttributeType,
        quantization: Quantization,
    ) -> Self {
        self.overrides.insert(att_type, quantization);
        self
    }

    /// Writes the metadata section.
    pub fn with_metadata(mut self, metadata: bool) -> Self {
        self.metadata = metadata;
        self
    }

    /// Rejects configurations that cannot be encoded.
    pub fn validate(&self) -> Result<(), Err> {
        if self.compression_level > MAX_COMPRESSION_LEVEL {
            return Err(Err::UnsupportedCompressionLevel(self.compression_level));
        }
        Ok(())
    }

    fn quantization_for(&self, att_type: AttributeType) -> Quantization {
        self.overrides
            .get(&att_type)
            .copied()
            .unwrap_or(self.quantization)
    }
}

/// What the decoder needs to undo one attribute's portabilization.
enum Portable {
    Quantized { min: Vec<f32>, range: f32, bits: u8 },
    Unsigned,
    Signed { mins: Vec<i32> },
}

/// Encodes a point cloud into the writer.
pub(crate) fn encode_impl<W>(pc: PointCloud, writer: &mut W, cfg: Config) -> Result<(), Err>
where
    W: ByteWriter,
{
    cfg.validate()?;
    let num_points = pc.num_points();
    if num_points == 0 {
        return Err(Err::NoPoints);
    }

    let attributes = pc.into_attributes();
    let dimension: usize = attributes.iter().map(|a| a.get_num_components()).sum();

    for b in b"DRACO" {
        writer.write_u8(*b);
    }
    writer.write_u8(2);
    writer.write_u8(3);
    writer.write_u8(GEOMETRY_TYPE_POINT_CLOUD);
    writer.write_u8(METHOD_KD_TREE);
    let flags = if cfg.metadata { METADATA_FLAG_MASK } else { 0 };
    writer.write_u16(flags);
    if cfg.metadata {
        super::metadata::encode_point_cloud_metadata(&attributes, writer);
    }

    writer.write_u32(num_points as u32);

    writer.write_u8(1);
    leb128_write(attributes.len() as u64, writer);
    for (i, att) in attributes.iter().enumerate() {
        att.get_attribute_type().write_to(writer);
        att.get_component_type().write_to(writer);
        writer.write_u8(att.get_num_components() as u8);
        writer.write_u8(0);
        leb128_write(i as u64, writer);
    }

    let mut points = vec![0u32; num_points * dimension];
    let mut portables = Vec::with_capacity(attributes.len());
    let mut offset = 0usize;
    for att in &attributes {
        let n = att.get_num_components();
        portables.push(portabilize(
            att,
            &cfg,
            num_points,
            &mut points,
            dimension,
            offset,
        )?);
        offset += n;
    }

    writer.write_u8(cfg.compression_level);
    kd_tree::encode_points(&mut points, dimension, cfg.compression_level, writer)?;

    for portable in &portables {
        if let Portable::Quantized { min, range, bits } = portable {
            for m in min {
                writer.write_u32(m.to_bits());
            }
            writer.write_u32(range.to_bits());
            writer.write_u8(*bits);
        }
    }
    for portable in &portables {
        if let Portable::Signed { mins } = portable {
            for &m in mins {
                leb128_write(zigzag(m) as u64, writer);
            }
        }
    }
    Ok(())
}

/// Writes one attribute's columns into the point array as unsigned integers.
fn portabilize(
    att: &Attribute,
    cfg: &Config,
    num_points: usize,
    points: &mut [u32],
    dimension: usize,
    offset: usize,
) -> Result<Portable, Err> {
    let num_components = att.get_num_components();
    if !(1..=4).contains(&num_components) {
        return Err(Err::UnsupportedAttributeLayout(
            num_components,
            att.get_component_type(),
        ));
    }
    match num_components {
        1 => portabilize_typed::<1>(att, cfg, num_points, points, dimension, offset),
        2 => portabilize_typed::<2>(att, cfg, num_points, points, dimension, offset),
        3 => portabilize_typed::<3>(att, cfg, num_points, points, dimension, offset),
        _ => portabilize_typed::<4>(att, cfg, num_points, points, dimension, offset),
    }
}

fn portabilize_typed<const N: usize>(
    att: &Attribute,
    cfg: &Config,
    num_points: usize,
    points: &mut [u32],
    dimension: usize,
    offset: usize,
) -> Result<Portable, Err>
where
    NdVector<N, f32>: Vector<N, Component = f32>,
    NdVector<N, u8>: Vector<N, Component = u8>,
    NdVector<N, u16>: Vector<N, Component = u16>,
    NdVector<N, u32>: Vector<N, Component = u32>,
    NdVector<N, i8>: Vector<N, Component = i8>,
    NdVector<N, i16>: Vector<N, Component = i16>,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    let att_type = att.get_attribute_type();

    macro_rules! write_signed {
        ($ty:ty) => {{
            let values: Vec<NdVector<N, $ty>> = (0..num_points)
                .map(|p| att.get(PointIdx::from(p)))
                .collect();
            let mut mins = vec![i32::MAX; N];
            for v in &values {
                for c in 0..N {
                    mins[c] = mins[c].min(*v.get(c) as i32);
                }
            }
            for (p, v) in values.iter().enumerate() {
                for c in 0..N {
                    // Widest span is i32::MAX - i32::MIN: it overflows an i32
                    // subtraction but still fits the u32 the kd-tree codes.
                    points[p * dimension + offset + c] = (*v.get(c) as i64 - mins[c] as i64) as u32;
                }
            }
            Ok(Portable::Signed { mins })
        }};
    }

    macro_rules! write_unsigned {
        ($ty:ty) => {{
            for p in 0..num_points {
                let v: NdVector<N, $ty> = att.get(PointIdx::from(p));
                for c in 0..N {
                    points[p * dimension + offset + c] = *v.get(c) as u32;
                }
            }
            Ok(Portable::Unsigned)
        }};
    }

    match att.get_component_type() {
        ComponentDataType::F32 => {
            let values: Vec<NdVector<N, f32>> = (0..num_points)
                .map(|p| att.get(PointIdx::from(p)))
                .collect();
            let mut min = [f32::INFINITY; N];
            let mut max = [f32::NEG_INFINITY; N];
            for v in &values {
                for c in 0..N {
                    let x = *v.get(c);
                    if !x.is_finite() {
                        return Err(Err::NonFiniteValue(att_type));
                    }
                    min[c] = min[c].min(x);
                    max[c] = max[c].max(x);
                }
            }
            // One step is shared by every component, so the largest extent
            // sets the resolution.
            let mut range = 0.0f32;
            for c in 0..N {
                range = range.max(max[c] - min[c]);
            }
            if range == 0.0 {
                range = 1.0;
            }
            let bits = cfg.quantization_for(att_type).resolve(range);
            let max_quantized = (1u32 << bits) - 1;
            let inverse_delta = max_quantized as f32 / range;
            for (p, v) in values.iter().enumerate() {
                for c in 0..N {
                    let q = ((*v.get(c) - min[c]) * inverse_delta + 0.5).floor();
                    if !(0.0..=max_quantized as f32).contains(&q) {
                        return Err(Err::QuantizedValueOutOfRange(att_type));
                    }
                    points[p * dimension + offset + c] = q as u32;
                }
            }
            Ok(Portable::Quantized {
                min: min.to_vec(),
                range,
                bits,
            })
        }
        ComponentDataType::U8 => write_unsigned!(u8),
        ComponentDataType::U16 => write_unsigned!(u16),
        ComponentDataType::U32 => write_unsigned!(u32),
        ComponentDataType::I8 => write_signed!(i8),
        ComponentDataType::I16 => write_signed!(i16),
        ComponentDataType::I32 => write_signed!(i32),
        other => Err(Err::UnsupportedComponentType(other)),
    }
}

/// Maps a signed integer onto the unsigned code the varint carries.
fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}
