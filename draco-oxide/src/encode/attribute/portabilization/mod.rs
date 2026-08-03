pub mod octahedral_quantization;
pub mod quantization_coordinate_wise;
pub mod to_bits;

use draco_oxide_core::attribute::{Attribute, AttributeType, ComponentDataType};
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::debug_write;
use draco_oxide_core::types::NdVector;
use draco_oxide_core::types::{ConfigType, Vector};

pub enum Portabilization<Data, const N: usize>
where
    Data: Vector<N> + Portable,
{
    QuantizationCoordinateWise(quantization_coordinate_wise::QuantizationCoordinateWise<Data, N>),
    OctahedralQuantization(octahedral_quantization::OctahedralQuantization<Data, N>),
    ToBits(to_bits::ToBits<Data, N>),
}

impl<Data, const N: usize> Portabilization<Data, N>
where
    Data: Vector<N> + Portable,
    NdVector<N, i32>: Vector<N, Component = i32>,
    NdVector<N, f32>: Vector<N, Component = f32> + Portable,
{
    /// creates a new instance of the portabilization, computes the metadata, and
    /// writes the metadata to the stream.
    // enum_dispatch does not support associated functions, we explicitly write the
    // constructor.
    pub fn new<W>(att: Attribute, cfg: Config, writer: &mut W) -> Self
    where
        W: ByteWriter,
    {
        debug_write!("Start of Portabilization Metadata", writer);
        // cfg.type_.write_to(writer);
        let out = match cfg.type_ {
            PortabilizationType::QuantizationCoordinateWise => {
                Portabilization::QuantizationCoordinateWise(
                    quantization_coordinate_wise::QuantizationCoordinateWise::<_, N>::new(
                        att, cfg, writer,
                    ),
                )
            }
            PortabilizationType::OctahedralQuantization => Portabilization::OctahedralQuantization(
                octahedral_quantization::OctahedralQuantization::new(att, cfg, writer),
            ),
            PortabilizationType::ToBits => {
                Portabilization::ToBits(to_bits::ToBits::new(att, cfg, writer))
            }
        };
        debug_write!("End of Portabilization Metadata", writer);
        out
    }

    pub fn portabilize(self) -> Attribute {
        match self {
            Portabilization::QuantizationCoordinateWise(qcw) => qcw.portabilize(),
            Portabilization::OctahedralQuantization(oct) => oct.portabilize(),
            Portabilization::ToBits(tb) => tb.portabilize(),
        }
    }
}

pub trait PortabilizationImpl<const N: usize>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// portabilizes the whole data.
    fn portabilize(self) -> Attribute;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortabilizationType {
    QuantizationCoordinateWise,
    OctahedralQuantization,
    ToBits,
}

impl PortabilizationType {
    pub(crate) fn get_id(&self) -> u8 {
        match self {
            PortabilizationType::ToBits => 1,
            PortabilizationType::QuantizationCoordinateWise => 2,
            PortabilizationType::OctahedralQuantization => 3,
        }
    }

    pub(crate) fn write_to<W>(&self, writer: &mut W)
    where
        W: ByteWriter,
    {
        let id = self.get_id();
        writer.write_u8(id);
    }

    /// The default portabilization for an attribute of type `ty` with
    /// components of `component_ty`. Integer values ride the integer codec
    /// whatever the attribute type; float quantization is only valid for
    /// float input (the reference decoder rejects a quantization block on a
    /// non-float declared type).
    pub(crate) fn default_for(ty: AttributeType, component_ty: ComponentDataType) -> Self {
        if component_ty.is_integer() {
            return PortabilizationType::ToBits;
        }
        match ty {
            AttributeType::Normal => PortabilizationType::OctahedralQuantization,
            // Float values of every other type, generics included, are
            // quantized: `ToBits` truncates floats numerically, and the
            // reference's lossless float form (the raw type 0 codec) is not
            // implemented.
            _ => PortabilizationType::QuantizationCoordinateWise,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub type_: PortabilizationType,
    pub quantization: Quantization,
}

impl ConfigType for Config {
    fn default() -> Self {
        Config {
            type_: PortabilizationType::QuantizationCoordinateWise,
            quantization: Quantization::Bits(11),
        }
    }
}

impl Config {
    /// The octahedral lattice this config quantizes onto, or `0` if it
    /// portabilizes some other way.
    pub fn oct_center(&self) -> i32 {
        match self.type_ {
            PortabilizationType::OctahedralQuantization => {
                draco_oxide_core::codec::attribute::geom::oct_center(self.quantization.resolve(0.0))
            }
            _ => 0,
        }
    }

    pub fn default_for(ty: AttributeType, component_ty: ComponentDataType) -> Self {
        if component_ty.is_integer() {
            return Config {
                type_: PortabilizationType::ToBits,
                quantization: Quantization::Bits(11), // not used for ToBits
            };
        }
        match ty {
            AttributeType::Normal => Config {
                type_: PortabilizationType::OctahedralQuantization,
                quantization: Quantization::Bits(8),
            },
            AttributeType::TextureCoordinate => Config {
                type_: PortabilizationType::QuantizationCoordinateWise,
                quantization: Quantization::Bits(10),
            },
            _ => Self::default(),
        }
    }
}

/// How the quantization resolution (number of bits) for an attribute is
/// determined. All variants ultimately resolve to a bit count in `1..=30`
/// (Draco's cap) via [`Quantization::resolve`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quantization {
    /// Explicit number of quantization bits.
    Bits(u8),
    /// Derive the bit count from a maximum tolerated quantization error,
    /// measured against the attribute's *observed* value range (the largest
    /// per-axis extent scanned from the data).
    MaxError(f32),
    /// Derive the bit count from a maximum tolerated error against a
    /// *caller-supplied* domain, making the resolution independent of any single
    /// mesh's extent. `range` is the largest per-axis span of the bounding box
    /// (see [`Quantization::from_bounding_box`]).
    Bounded { range: f32, max_error: f32 },
}

impl Default for Quantization {
    fn default() -> Self {
        Quantization::Bits(11)
    }
}

impl Quantization {
    /// Builds a [`Quantization::Bounded`] from an explicit axis-aligned bounding
    /// box; the largest per-axis span sets the resolution.
    pub fn from_bounding_box(min: &[f32], max: &[f32], max_error: f32) -> Self {
        let range = min
            .iter()
            .zip(max.iter())
            .map(|(lo, hi)| hi - lo)
            .fold(0.0_f32, f32::max);
        Quantization::Bounded { range, max_error }
    }

    /// Resolves this spec to a concrete number of quantization bits, clamped to
    /// `1..=30`. `observed_range` is the largest per-axis extent of the data,
    /// used only by [`Quantization::MaxError`]; other variants ignore it.
    pub fn resolve(self, observed_range: f32) -> u8 {
        let bits = match self {
            Quantization::Bits(n) => n,
            Quantization::MaxError(max_error) => bits_for_error(observed_range, max_error),
            Quantization::Bounded { range, max_error } => bits_for_error(range, max_error),
        };
        bits.clamp(1, 30)
    }
}

/// Smallest bit count whose quantization step over `range` does not exceed
/// `max_error`. The decoder dequantizes with step `range / (2^bits - 1)`, so we
/// need `2^bits >= range / max_error + 1`.
fn bits_for_error(range: f32, max_error: f32) -> u8 {
    if range <= 0.0 || max_error <= 0.0 {
        return 1;
    }
    let bits = (range / max_error + 1.0).log2().ceil();
    bits.clamp(1.0, 30.0) as u8
}
