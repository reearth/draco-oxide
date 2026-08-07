//! Encode-side prediction metadata: the side channel a prediction scheme
//! writes so the decoder can replay the choices it made against the true
//! values. Only the schemes that make such choices implement
//! [`PredictionEncoder`]; for the rest the stream carries nothing.

use crate::encode::entropy::rans::{encode_rabs_bit_stream, Err as RansErr, RabsCoder};
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::attribute::prediction_scheme::{
    mesh_constrained_multi_parallelogram_prediction::MeshConstrainedMultiParallelogramPrediction,
    mesh_normal_prediction::MeshNormalPrediction,
    mesh_prediction_for_texture_coordinates::MeshPredictionForTextureCoordinates, PredictionScheme,
};
use draco_oxide_core::mesh::ds::GenericAttributeDs;
use draco_oxide_core::types::{NdVector, Vector};
use draco_oxide_core::utils::bit_coder::leb128_write;

#[derive(thiserror::Error, Clone, Debug)]
pub enum Err {
    #[error("rANS coder error: {0}")]
    RansCoder(#[from] RansErr),
}

/// A prediction scheme that writes metadata for the decoder to replay.
pub trait PredictionEncoder {
    /// Writes this scheme's metadata at the current stream position. The
    /// position relative to the prediction transform is scheme-specific and the
    /// caller's responsibility.
    fn encode_prediction_metadata<W>(&self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter;
}

/// Encodes the per-value sign-flip bits of mesh-normal prediction into `writer`,
/// using the exact rABS layout the decoder expects: a `zero_prob` byte, then the
/// leb128 length of the coded buffer, then the buffer itself.
///
/// Exposed separately from the [`PredictionEncoder`] impl so the zero-CPU
/// "trust prediction" encode path can emit neutral (all-false) flips for `count`
/// values without constructing the predictor at all; an all-false slice is a
/// valid input and reproduces the byte layout of a run in which every predicted
/// normal was kept as-is.
pub fn encode_flip_metadata<W>(flips: &[bool], writer: &mut W) -> Result<(), Err>
where
    W: ByteWriter,
{
    Ok(encode_rabs_bit_stream(flips, writer)?)
}

impl<const N: usize, D: GenericAttributeDs> PredictionEncoder for MeshNormalPrediction<'_, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn encode_prediction_metadata<W>(&self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        encode_flip_metadata(self.flips(), writer)
    }
}

impl<const N: usize, D: GenericAttributeDs> PredictionEncoder
    for MeshPredictionForTextureCoordinates<'_, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn encode_prediction_metadata<W>(&self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        let orientation = self.orientation();
        let freq_count_0 = {
            let mut last = true;
            let mut compare = |o| {
                if o == last {
                    true
                } else {
                    last = o;
                    false
                }
            };
            orientation
                .iter()
                .map(|&o| compare(o))
                .filter(|&o| !o)
                .count()
        };
        let orientation_len_float = orientation.len() as f32 + 0.001;
        let zero_prob = (((freq_count_0 as f32 / orientation_len_float) * 256.0 + 0.5) as u16)
            .clamp(1, 255) as u8;
        let mut rabs_coder: RabsCoder = RabsCoder::new(zero_prob as usize, None);
        writer.write_u32(orientation.len() as u32);
        writer.write_u8(zero_prob);
        let mut last_orientation = true;
        let out = orientation
            .iter()
            .rev()
            .map(|&o| {
                if o == last_orientation {
                    1
                } else {
                    last_orientation = o;
                    0
                }
            })
            .collect::<Vec<_>>();
        for bit in out.into_iter().rev() {
            rabs_coder.write(bit)?;
        }
        let buffer = rabs_coder.flush()?;
        leb128_write(buffer.len() as u64, writer);
        for byte in buffer {
            writer.write_u8(byte);
        }
        Ok(())
    }
}

impl<const N: usize, D: GenericAttributeDs> PredictionEncoder
    for MeshConstrainedMultiParallelogramPrediction<'_, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn encode_prediction_metadata<W>(&self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        for bits in self.creases().bits() {
            leb128_write(bits.len() as u64, writer);
            if !bits.is_empty() {
                encode_rabs_bit_stream(bits, writer)?;
            }
        }
        Ok(())
    }
}

impl<const N: usize, D: GenericAttributeDs> PredictionEncoder for PredictionScheme<'_, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn encode_prediction_metadata<W>(&self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        match self {
            PredictionScheme::MeshConstrainedMultiParallelogramPrediction(p) => {
                p.encode_prediction_metadata(writer)
            }
            PredictionScheme::MeshNormalPrediction(p) => p.encode_prediction_metadata(writer),
            PredictionScheme::MeshPredictionForTextureCoordinates(p) => {
                p.encode_prediction_metadata(writer)
            }
            // The remaining schemes put nothing on the wire.
            _ => Ok(()),
        }
    }
}
