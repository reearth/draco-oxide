//! Decode-side prediction: per-scheme metadata (normal flips, texcoord
//! orientations) and the predictor that drives core's prediction schemes over
//! partially decoded data.

use crate::entropy::rans::RabsDecoder;
use crate::reader::RevReader;
use crate::Err;
use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::attribute::prediction_scheme::{
    delta_prediction::DeltaPrediction, mesh_normal_prediction::MeshNormalPrediction,
    mesh_parallelogram_prediction::MeshParallelogramPrediction,
    mesh_prediction_for_texture_coordinates::MeshPredictionForTextureCoordinates,
    PredictionSchemeImpl, PredictionSchemeType,
};
use draco_oxide_core::mesh::ds::GenericAttributeDs;
use draco_oxide_core::types::{CornerIdx, NdVector, Vector, VertexIdx};
use draco_oxide_core::utils::bit_coder::leb128_read;

/// Parses the prediction scheme id byte (the ids of
/// `PredictionSchemeType::get_id`).
pub(crate) fn read_scheme_id(reader: &mut Reader<'_>) -> Result<PredictionSchemeType, Err> {
    let id = reader.read_u8()?;
    match id {
        0 => Ok(PredictionSchemeType::DeltaPrediction),
        1 => Ok(PredictionSchemeType::MeshParallelogramPrediction),
        2 => Ok(PredictionSchemeType::MeshMultiParallelogramPrediction),
        5 => Ok(PredictionSchemeType::MeshPredictionForTextureCoordinates),
        6 => Ok(PredictionSchemeType::MeshNormalPrediction),
        7 => Ok(PredictionSchemeType::DerivativePrediction),
        0xFE => Ok(PredictionSchemeType::NoPrediction),
        _ => Err(Err::MalformedAttribute("unknown prediction scheme id")),
    }
}

/// Builds a rabs decoder over a self-contained `[leb128 len | bytes]` sub-stream,
/// with the probability byte already read by the caller.
fn rabs_over_substream<'a>(reader: &mut Reader<'a>, prob_zero: u8) -> Result<RabsDecoder<'a>, Err> {
    let len = leb128_read(reader)? as usize;
    let rev = RevReader::new(reader.read_bytes(len)?);
    RabsDecoder::new(rev, prob_zero)
}

/// Decodes the normal-prediction flip metadata: one bit per value, in traversal
/// order.
pub(crate) fn decode_flip_metadata(
    reader: &mut Reader<'_>,
    num_values: usize,
) -> Result<Vec<bool>, Err> {
    let zero_prob = reader.read_u8()?;
    let mut rabs = rabs_over_substream(reader, zero_prob)?;
    Ok((0..num_values).map(|_| rabs.decode_bit()).collect())
}

/// Decodes the texture-coordinate orientation metadata: a u32 count, then one
/// delta-coded bit per recorded orientation (a zero bit toggles the running
/// value, anchored at `true`). The decoded vector is in reverse traversal order;
/// consume it by popping from the back.
pub(crate) fn decode_orientation_metadata(reader: &mut Reader<'_>) -> Result<Vec<bool>, Err> {
    let count = reader.read_u32()? as usize;
    let zero_prob = reader.read_u8()?;
    let mut rabs = rabs_over_substream(reader, zero_prob)?;
    let mut last = true;
    let mut orientations = Vec::with_capacity(count);
    for _ in 0..count {
        if !rabs.decode_bit() {
            last = !last;
        }
        orientations.push(last);
    }
    Ok(orientations)
}

/// The decode-side predictor: wraps the core prediction schemes, feeding the
/// decoded flip/orientation metadata where the encoder consulted the actual
/// values.
pub(crate) enum Predictor<'p, const N: usize, D: GenericAttributeDs>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    NoPrediction,
    Delta(DeltaPrediction<'p, N, D>),
    Parallelogram(MeshParallelogramPrediction<'p, N, D>),
    TexCoords {
        scheme: MeshPredictionForTextureCoordinates<'p, N, D>,
        orientations: Vec<bool>,
    },
    Normal {
        scheme: MeshNormalPrediction<'p, N, D>,
        flips: Vec<bool>,
        next: usize,
    },
}

impl<'p, const N: usize, D: GenericAttributeDs> Predictor<'p, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// Builds the predictor for `scheme_ty`. `parents` carries the decoded
    /// portable position attribute for the geometric schemes.
    /// `oct_center` is consulted only by the normal scheme.
    pub(crate) fn new(
        scheme_ty: &PredictionSchemeType,
        parents: &[&'p Attribute],
        ads: &'p D,
        flips: Vec<bool>,
        orientations: Vec<bool>,
        oct_center: i32,
    ) -> Result<Self, Err> {
        Ok(match scheme_ty {
            PredictionSchemeType::NoPrediction => Predictor::NoPrediction,
            PredictionSchemeType::DeltaPrediction => {
                Predictor::Delta(DeltaPrediction::new(parents, ads))
            }
            PredictionSchemeType::MeshParallelogramPrediction => {
                Predictor::Parallelogram(MeshParallelogramPrediction::new(parents, ads))
            }
            PredictionSchemeType::MeshPredictionForTextureCoordinates => Predictor::TexCoords {
                scheme: MeshPredictionForTextureCoordinates::new(parents, ads),
                orientations,
            },
            PredictionSchemeType::MeshNormalPrediction => {
                if oct_center <= 0 {
                    return Err(Err::MalformedAttribute(
                        "normal prediction needs an octahedral prediction transform",
                    ));
                }
                let mut scheme = MeshNormalPrediction::new(parents, ads);
                scheme.set_octahedral_center(oct_center);
                Predictor::Normal {
                    scheme,
                    flips,
                    next: 0,
                }
            }
            // No encoder-side implementation emits these yet.
            PredictionSchemeType::MeshMultiParallelogramPrediction
            | PredictionSchemeType::DerivativePrediction => return Err(Err::Unimplemented),
            PredictionSchemeType::Invalid => {
                return Err(Err::MalformedAttribute("invalid prediction scheme"))
            }
        })
    }

    /// Predicts the value at corner `c` from the already decoded data.
    #[inline]
    pub(crate) fn predict(
        &mut self,
        c: CornerIdx,
        vertices_up_till_now: &[VertexIdx],
        attribute: &Attribute,
    ) -> NdVector<N, i32> {
        match self {
            Predictor::NoPrediction => NdVector::zero(),
            Predictor::Delta(scheme) => scheme.predict(c, vertices_up_till_now, attribute),
            Predictor::Parallelogram(scheme) => scheme.predict(c, vertices_up_till_now, attribute),
            Predictor::TexCoords {
                scheme,
                orientations,
            } => scheme.predict_given_orientation(c, vertices_up_till_now, attribute, orientations),
            Predictor::Normal {
                scheme,
                flips,
                next,
            } => {
                let mut pred = scheme.predicted_value(c);
                let flip = flips.get(*next).copied().unwrap_or(false);
                *next += 1;
                if flip {
                    pred *= -1;
                }
                scheme.project(pred)
            }
        }
    }
}
