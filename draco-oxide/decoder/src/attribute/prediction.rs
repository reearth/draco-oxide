//! Decode-side prediction: per-scheme metadata (normal flips, texcoord
//! orientations) and the predictor that drives core's prediction schemes over
//! partially decoded data.

use crate::entropy::rans::RabsDecoder;
use crate::reader::RevReader;
use crate::Err;
use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::attribute::prediction_scheme::{
    delta_prediction::DeltaPrediction,
    mesh_constrained_multi_parallelogram_prediction::{
        Creases, MeshConstrainedMultiParallelogramPrediction, MAX_PARALLELOGRAMS,
    },
    mesh_normal_prediction::MeshNormalPrediction,
    mesh_parallelogram_prediction::MeshParallelogramPrediction,
    mesh_prediction_for_texture_coordinates::MeshPredictionForTextureCoordinates,
    NoPrediction, PredictionSchemeImpl, PredictionSchemeType, SchemeDispatch,
};
use draco_oxide_core::mesh::ds::GenericAttributeDs;
use draco_oxide_core::types::{NdVector, Vector};
use draco_oxide_core::utils::bit_coder::leb128_read;

/// Parses the prediction scheme id byte (the ids of
/// `PredictionSchemeType::get_id`).
pub(crate) fn read_scheme_id(reader: &mut Reader<'_>) -> Result<PredictionSchemeType, Err> {
    let id = reader.read_u8()?;
    match id {
        0 => Ok(PredictionSchemeType::DeltaPrediction),
        1 => Ok(PredictionSchemeType::MeshParallelogramPrediction),
        2 => Ok(PredictionSchemeType::MeshMultiParallelogramPrediction),
        4 => Ok(PredictionSchemeType::MeshConstrainedMultiParallelogramPrediction),
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

/// Decodes the constrained multi-parallelogram crease metadata: one rABS bit
/// stream per context, each prefixed by its leb128 bit count. Context `i` holds
/// exactly `i + 1` bits for every value whose vertex had `i + 1` parallelograms
/// available, so `num_values * (i + 1)` bounds a well-formed stream.
pub(crate) fn decode_crease_metadata(
    reader: &mut Reader<'_>,
    num_values: usize,
) -> Result<[Vec<bool>; MAX_PARALLELOGRAMS], Err> {
    let mut out: [Vec<bool>; MAX_PARALLELOGRAMS] = Default::default();
    for (i, bits) in out.iter_mut().enumerate() {
        let count = leb128_read(reader)? as usize;
        if count > num_values * (i + 1) {
            return Err(Err::MalformedAttribute(
                "crease bit count exceeds the attribute's value count",
            ));
        }
        if count > 0 {
            let zero_prob = reader.read_u8()?;
            let mut rabs = rabs_over_substream(reader, zero_prob)?;
            *bits = (0..count).map(|_| rabs.decode_bit()).collect();
        }
    }
    Ok(out)
}

/// A prediction scheme that replays metadata the encoder recorded.
///
/// The wire read itself stays a free function above: payload parsing runs
/// before any attribute data structure is chosen, so the `D`-generic scheme
/// type does not exist yet at the point the bytes must be consumed. This trait
/// covers the second half, handing the decoded metadata to the scheme so
/// `predict::<false>` can consume it.
pub(crate) trait PredictionDecoder {
    /// The metadata this scheme reads before predicting.
    type Metadata;

    /// Installs decoded metadata into the scheme.
    fn install_prediction_metadata(&mut self, metadata: Self::Metadata);
}

impl<const N: usize, D: GenericAttributeDs> PredictionDecoder for MeshNormalPrediction<'_, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    type Metadata = Vec<bool>;

    fn install_prediction_metadata(&mut self, flips: Vec<bool>) {
        self.set_flips(flips);
    }
}

impl<const N: usize, D: GenericAttributeDs> PredictionDecoder
    for MeshPredictionForTextureCoordinates<'_, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    type Metadata = Vec<bool>;

    fn install_prediction_metadata(&mut self, orientations: Vec<bool>) {
        self.set_orientation(orientations);
    }
}

impl<const N: usize, D: GenericAttributeDs> PredictionDecoder
    for MeshConstrainedMultiParallelogramPrediction<'_, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    type Metadata = [Vec<bool>; MAX_PARALLELOGRAMS];

    fn install_prediction_metadata(&mut self, creases: Self::Metadata) {
        self.set_creases(Creases::new(creases));
    }
}

/// The decode-side predictor: wraps the core prediction schemes, which carry
/// their own decoded metadata.
pub(crate) enum Predictor<'p, const N: usize, D: GenericAttributeDs>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    NoPrediction,
    Delta(DeltaPrediction<'p, N, D>),
    Parallelogram(MeshParallelogramPrediction<'p, N, D>),
    ConstrainedMultiParallelogram(Box<MeshConstrainedMultiParallelogramPrediction<'p, N, D>>),
    TexCoords(MeshPredictionForTextureCoordinates<'p, N, D>),
    Normal(MeshNormalPrediction<'p, N, D>),
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
        creases: [Vec<bool>; MAX_PARALLELOGRAMS],
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
            PredictionSchemeType::MeshConstrainedMultiParallelogramPrediction => {
                let mut scheme = Box::new(MeshConstrainedMultiParallelogramPrediction::new(
                    parents, ads,
                ));
                scheme.install_prediction_metadata(creases);
                Predictor::ConstrainedMultiParallelogram(scheme)
            }
            PredictionSchemeType::MeshPredictionForTextureCoordinates => {
                if N != 2 {
                    return Err(Err::MalformedAttribute(
                        "texture coordinate prediction requires a 2-component attribute",
                    ));
                }
                let mut scheme = MeshPredictionForTextureCoordinates::new(parents, ads);
                scheme.install_prediction_metadata(orientations);
                Predictor::TexCoords(scheme)
            }
            PredictionSchemeType::MeshNormalPrediction => {
                if N != 2 {
                    return Err(Err::MalformedAttribute(
                        "normal prediction requires a 2-component octahedral attribute",
                    ));
                }
                if oct_center <= 0 {
                    return Err(Err::MalformedAttribute(
                        "normal prediction needs an octahedral prediction transform",
                    ));
                }
                let mut scheme = MeshNormalPrediction::new(parents, ads);
                scheme.set_octahedral_center(oct_center);
                scheme.install_prediction_metadata(flips);
                Predictor::Normal(scheme)
            }
            // No encoder-side implementation emits these yet.
            PredictionSchemeType::MeshMultiParallelogramPrediction
            | PredictionSchemeType::DerivativePrediction => return Err(Err::Unimplemented),
            PredictionSchemeType::Invalid => {
                return Err(Err::MalformedAttribute("invalid prediction scheme"))
            }
        })
    }

    /// Runs `dispatch` against the concrete scheme. The variant match happens
    /// here, once, so the dispatched computation's predict loop is monomorphic.
    pub(crate) fn dispatch_mut<V>(&mut self, dispatch: V) -> V::Out
    where
        V: SchemeDispatch<'p, N, D>,
    {
        match self {
            Predictor::NoPrediction => dispatch.run(&mut NoPrediction::new()),
            Predictor::Delta(scheme) => dispatch.run(scheme),
            Predictor::Parallelogram(scheme) => dispatch.run(scheme),
            Predictor::ConstrainedMultiParallelogram(scheme) => dispatch.run(scheme.as_mut()),
            Predictor::TexCoords(scheme) => dispatch.run(scheme),
            Predictor::Normal(scheme) => dispatch.run(scheme),
        }
    }
}
