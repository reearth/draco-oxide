pub mod delta_prediction;
pub mod mesh_constrained_multi_parallelogram_prediction;
pub mod mesh_normal_prediction;
pub mod mesh_parallelogram_prediction;
pub mod mesh_prediction_for_texture_coordinates;

use crate::attribute::Attribute;
use crate::bit_coder::ByteWriter;
use crate::mesh::ds::GenericAttributeDs;
use crate::types::NdVector;
use crate::types::{ConfigType, CornerIdx, Vector, VertexIdx};

/// PredictionScheme traits are not generic and the structs implementing the
/// trait are generic. This is so because some of the structs need to store
/// the previous values in order to compute the current value.
///
/// `D` is the attribute data structure the scheme reads its already decoded
/// values from; the caller monomorphizes it (e.g. the general `AttributeDS` or
/// the identity fast path) and the scheme never needs to know which.
pub trait PredictionSchemeImpl<'parents, const N: usize, D: GenericAttributeDs>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// Creates the prediction.
    fn new(parents: &[&'parents Attribute], ads: &'parents D) -> Self;

    /// Predicts the attribute from the given information.
    ///
    /// `ENCODING` selects the direction. A scheme with prediction metadata
    /// derives it from the true values when encoding and records it in `self`;
    /// when decoding it consumes the metadata already installed in `self`
    /// instead. The branch is resolved at monomorphization, so neither side
    /// carries the other's code.
    fn predict<const ENCODING: bool>(
        &mut self,
        // Corner index to predict.
        c: CornerIdx,
        // Vertices processed before the call to this function.
        // They must be sorted in the order they were processed.
        vertices_processed_up_till_now: &[VertexIdx],
        // The attribute that is being predicted.
        // When used by the encoder, this is the complete attribute.
        // When used by the decoder, this is the data that is being decoded, and thus it is not complete.
        // Hence, expecially in the decoder, the element access can only be done by the index that is
        // an element of `vertices_processed_up_till_now`.
        attribute: &Attribute,
    ) -> NdVector<N, i32>;
}

/// A computation dispatched once against a concrete scheme, so per-value
/// predict loops carry no per-call variant match.
pub trait SchemeDispatch<'parents, const N: usize, D: GenericAttributeDs>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    type Out;

    fn run<P: PredictionSchemeImpl<'parents, N, D>>(self, scheme: &mut P) -> Self::Out;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PredictionSchemeType {
    DerivativePrediction,
    MeshConstrainedMultiParallelogramPrediction,
    MeshMultiParallelogramPrediction,
    MeshParallelogramPrediction,
    MeshNormalPrediction,
    MeshPredictionForTextureCoordinates,
    DeltaPrediction,
    NoPrediction,
    Invalid,
}

impl PredictionSchemeType {
    pub fn get_id(&self) -> u8 {
        match self {
            PredictionSchemeType::DeltaPrediction => 0,
            PredictionSchemeType::MeshParallelogramPrediction => 1,
            PredictionSchemeType::MeshMultiParallelogramPrediction => 2,
            PredictionSchemeType::MeshConstrainedMultiParallelogramPrediction => 4,
            PredictionSchemeType::MeshPredictionForTextureCoordinates => 5,
            PredictionSchemeType::MeshNormalPrediction => 6,
            PredictionSchemeType::DerivativePrediction => 7,

            PredictionSchemeType::NoPrediction => 0xFE, // -2 in i8
            PredictionSchemeType::Invalid => 0xFF,      // -1 in i8
        }
    }

    pub fn write_to<W>(&self, writer: &mut W)
    where
        W: ByteWriter,
    {
        let id = self.get_id();
        writer.write_u8(id);
    }
}

pub enum PredictionScheme<'parents, const N: usize, D: GenericAttributeDs> {
    DeltaPrediction(delta_prediction::DeltaPrediction<'parents, N, D>),
    MeshConstrainedMultiParallelogramPrediction(
        Box<
            mesh_constrained_multi_parallelogram_prediction::MeshConstrainedMultiParallelogramPrediction<
                'parents,
                N,
                D,
            >,
        >,
    ),
    MeshParallelogramPrediction(
        mesh_parallelogram_prediction::MeshParallelogramPrediction<'parents, N, D>,
    ),
    MeshNormalPrediction(mesh_normal_prediction::MeshNormalPrediction<'parents, N, D>),
    MeshPredictionForTextureCoordinates(
        mesh_prediction_for_texture_coordinates::MeshPredictionForTextureCoordinates<
            'parents,
            N,
            D,
        >,
    ),
    NoPrediction(NoPrediction),
}

impl<'parents, const N: usize, D: GenericAttributeDs> PredictionScheme<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// `oct_center` is consulted only by the normal scheme.
    pub fn new(
        ty: PredictionSchemeType,
        parents: &[&'parents Attribute],
        ads: &'parents D,
        oct_center: i32,
    ) -> Self {
        match ty {
            PredictionSchemeType::DeltaPrediction => {
                let prediction = delta_prediction::DeltaPrediction::new(parents, ads);
                PredictionScheme::DeltaPrediction(prediction)
            }
            PredictionSchemeType::MeshConstrainedMultiParallelogramPrediction => {
                let prediction = Box::new(mesh_constrained_multi_parallelogram_prediction::MeshConstrainedMultiParallelogramPrediction::new(
                    parents, ads,
                ));
                PredictionScheme::MeshConstrainedMultiParallelogramPrediction(prediction)
            }
            PredictionSchemeType::MeshParallelogramPrediction => {
                let prediction =
                    mesh_parallelogram_prediction::MeshParallelogramPrediction::new(parents, ads);
                PredictionScheme::MeshParallelogramPrediction(prediction)
            }
            PredictionSchemeType::MeshNormalPrediction => {
                let mut prediction =
                    mesh_normal_prediction::MeshNormalPrediction::new(parents, ads);
                prediction.set_octahedral_center(oct_center);
                PredictionScheme::MeshNormalPrediction(prediction)
            }
            PredictionSchemeType::MeshPredictionForTextureCoordinates => {
                let prediction = mesh_prediction_for_texture_coordinates::MeshPredictionForTextureCoordinates::new(
                    parents, ads
                );
                PredictionScheme::MeshPredictionForTextureCoordinates(prediction)
            }
            PredictionSchemeType::NoPrediction => {
                let prediction = NoPrediction::new();
                PredictionScheme::NoPrediction(prediction)
            }
            // Config::validate rejects these before anything is constructed.
            PredictionSchemeType::DerivativePrediction
            | PredictionSchemeType::MeshMultiParallelogramPrediction
            | PredictionSchemeType::Invalid => {
                panic!("unimplemented prediction scheme type");
            }
        }
    }

    /// Runs `dispatch` against the concrete scheme, matching the variant once.
    pub fn dispatch_mut<V>(&mut self, dispatch: V) -> V::Out
    where
        V: SchemeDispatch<'parents, N, D>,
    {
        match self {
            PredictionScheme::DeltaPrediction(prediction) => dispatch.run(prediction),
            PredictionScheme::MeshConstrainedMultiParallelogramPrediction(prediction) => {
                dispatch.run(prediction.as_mut())
            }
            PredictionScheme::MeshParallelogramPrediction(prediction) => dispatch.run(prediction),
            PredictionScheme::MeshNormalPrediction(prediction) => dispatch.run(prediction),
            PredictionScheme::MeshPredictionForTextureCoordinates(prediction) => {
                dispatch.run(prediction)
            }
            PredictionScheme::NoPrediction(prediction) => dispatch.run(prediction),
        }
    }

    pub fn get_type(&self) -> PredictionSchemeType {
        match self {
            PredictionScheme::DeltaPrediction(_) => PredictionSchemeType::DeltaPrediction,
            PredictionScheme::MeshConstrainedMultiParallelogramPrediction(_) => {
                PredictionSchemeType::MeshConstrainedMultiParallelogramPrediction
            }
            PredictionScheme::MeshParallelogramPrediction(_) => {
                PredictionSchemeType::MeshParallelogramPrediction
            }
            PredictionScheme::MeshNormalPrediction(_) => PredictionSchemeType::MeshNormalPrediction,
            PredictionScheme::MeshPredictionForTextureCoordinates(_) => {
                PredictionSchemeType::MeshPredictionForTextureCoordinates
            }
            PredictionScheme::NoPrediction(_) => PredictionSchemeType::NoPrediction,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub ty: PredictionSchemeType,
}

impl ConfigType for Config {
    fn default() -> Self {
        Config {
            ty: PredictionSchemeType::DeltaPrediction,
        }
    }
}

pub struct NoPrediction {}

impl Default for NoPrediction {
    fn default() -> Self {
        Self::new()
    }
}

impl NoPrediction {
    pub fn new() -> Self {
        Self {}
    }
}

impl<'a, const N: usize, D: GenericAttributeDs> PredictionSchemeImpl<'a, N, D> for NoPrediction
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn new(_parents: &[&'a Attribute], _ads: &'a D) -> Self {
        Self {}
    }

    fn predict<const ENCODING: bool>(
        &mut self,
        _: CornerIdx,
        _: &[VertexIdx],
        _: &Attribute,
    ) -> NdVector<N, i32> {
        NdVector::zero()
    }
}
