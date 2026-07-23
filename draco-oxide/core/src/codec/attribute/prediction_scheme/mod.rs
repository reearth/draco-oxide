pub mod delta_prediction;
pub mod derivative_prediction;
pub mod mesh_multi_parallelogram_prediction;
pub mod mesh_normal_prediction;
pub mod mesh_parallelogram_prediction;
pub mod mesh_prediction_for_texture_coordinates;

use crate::attribute::Attribute;
use crate::bit_coder::{ByteWriter, Reader};
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
    /// Id of the prediction method. This value is encoded to buffer in order
    /// for the decoder to identify the prediction method.
    #[allow(dead_code)]
    const ID: u32 = 0;

    type AdditionalDataForMetadata;

    /// Creates the prediction.
    fn new(parents: &[&'parents Attribute], ads: &'parents D) -> Self;

    // This function is not used in the current implementation, but it will be used in the future
    // to allow multiple encoding groups for one attribute.
    #[allow(unused)]
    fn get_values_impossible_to_predict(
        &mut self,
        value_indices: &mut Vec<std::ops::Range<usize>>,
    ) -> Vec<std::ops::Range<usize>>;

    /// predicts the attribute from the given information.
    fn predict(
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

    /// Encodes the prediction metadata to the writer.
    /// The implementation of this function is optional.
    fn encode_prediction_metadtata<W>(&self, _writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PredictionSchemeType {
    DerivativePrediction,
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

    #[allow(unused)]
    pub fn read_from(reader: &mut Reader<'_>) -> Result<Self, usize> {
        let id = reader.read_u8().unwrap() as usize; // ToDo: handle error.
        let out = match id {
            0 => PredictionSchemeType::DeltaPrediction,
            1 => PredictionSchemeType::MeshParallelogramPrediction,
            2 => PredictionSchemeType::MeshMultiParallelogramPrediction,
            5 => PredictionSchemeType::MeshPredictionForTextureCoordinates,
            6 => PredictionSchemeType::MeshNormalPrediction,
            7 => PredictionSchemeType::DerivativePrediction,
            0xFE => PredictionSchemeType::NoPrediction, // -2 in i8
            0xFF => PredictionSchemeType::Invalid,      // -1 in i8
            // If the id is not recognized, return an error.
            _ => return Err(id),
        };
        Ok(out)
    }

    #[allow(unused, clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        match self {
            PredictionSchemeType::DeltaPrediction => "DeltaPrediction".to_string(),
            PredictionSchemeType::DerivativePrediction => "DerivativePrediction".to_string(),
            PredictionSchemeType::MeshMultiParallelogramPrediction => {
                "MeshMultiParallelogramPrediction".to_string()
            }
            PredictionSchemeType::MeshParallelogramPrediction => {
                "MeshParallelogramPrediction".to_string()
            }
            PredictionSchemeType::NoPrediction => "NoPrediction".to_string(),
            PredictionSchemeType::MeshNormalPrediction => "MeshNormalPrediction".to_string(),
            PredictionSchemeType::MeshPredictionForTextureCoordinates => {
                "MeshPredictionForTextureCoordinates".to_string()
            }
            // Invalid is used when the prediction scheme type is not recognized.
            PredictionSchemeType::Invalid => "Invalid".to_string(),
        }
    }
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum Err {
    #[error("ranscoder error: {0}")]
    RanscoderError(#[from] crate::codec::entropy::rans::Err),
}

pub enum PredictionScheme<'parents, const N: usize, D: GenericAttributeDs> {
    DeltaPrediction(delta_prediction::DeltaPrediction<'parents, N, D>),
    DerivativePrediction(
        derivative_prediction::DerivativePredictionForTextureCoordinates<'parents, N, D>,
    ),
    MeshMultiParallelogramPrediction(
        mesh_multi_parallelogram_prediction::MeshMultiParallelogramPrediction<'parents, N, D>,
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
    pub fn new(
        ty: PredictionSchemeType,
        parents: &[&'parents Attribute],
        ads: &'parents D,
    ) -> Self {
        match ty {
            PredictionSchemeType::DeltaPrediction => {
                let prediction = delta_prediction::DeltaPrediction::new(parents, ads);
                PredictionScheme::DeltaPrediction(prediction)
            }
            PredictionSchemeType::DerivativePrediction => {
                let prediction =
                    derivative_prediction::DerivativePredictionForTextureCoordinates::new(
                        parents, ads,
                    );
                PredictionScheme::DerivativePrediction(prediction)
            }
            PredictionSchemeType::MeshMultiParallelogramPrediction => {
                let prediction =
                    mesh_multi_parallelogram_prediction::MeshMultiParallelogramPrediction::new(
                        parents, ads,
                    );
                PredictionScheme::MeshMultiParallelogramPrediction(prediction)
            }
            PredictionSchemeType::MeshParallelogramPrediction => {
                let prediction =
                    mesh_parallelogram_prediction::MeshParallelogramPrediction::new(parents, ads);
                PredictionScheme::MeshParallelogramPrediction(prediction)
            }
            PredictionSchemeType::MeshNormalPrediction => {
                let prediction = mesh_normal_prediction::MeshNormalPrediction::new(parents, ads);
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
            PredictionSchemeType::Invalid => {
                panic!("Invalid prediction scheme type");
            }
        }
    }

    #[allow(unused)] // TODO: Remove this function when the decoder is complete
    pub fn read_from(
        reader: &mut Reader<'_>,
        parents: &[&'parents Attribute],
        ads: &'parents D,
    ) -> Result<Self, usize> {
        let ty = PredictionSchemeType::read_from(reader)?;
        Ok(Self::new(ty, parents, ads))
    }

    #[allow(unused)] // TODO: Remove this function when we support multiple encoding groups for one attribute
    pub fn get_values_impossible_to_predict(
        &mut self,
        value_indices: &mut Vec<std::ops::Range<usize>>,
    ) -> Vec<std::ops::Range<usize>> {
        match self {
            PredictionScheme::DeltaPrediction(prediction) => {
                prediction.get_values_impossible_to_predict(value_indices)
            }
            PredictionScheme::DerivativePrediction(prediction) => {
                prediction.get_values_impossible_to_predict(value_indices)
            }
            PredictionScheme::MeshMultiParallelogramPrediction(prediction) => {
                prediction.get_values_impossible_to_predict(value_indices)
            }
            PredictionScheme::MeshParallelogramPrediction(prediction) => {
                prediction.get_values_impossible_to_predict(value_indices)
            }
            PredictionScheme::MeshNormalPrediction(prediction) => {
                prediction.get_values_impossible_to_predict(value_indices)
            }
            PredictionScheme::MeshPredictionForTextureCoordinates(prediction) => {
                prediction.get_values_impossible_to_predict(value_indices)
            }
            PredictionScheme::NoPrediction(_) => Vec::new(),
        }
    }

    pub fn predict(
        &mut self,
        // Vertex/corner index to predict.
        i: CornerIdx,
        // Vertices/corners processed before the call to this function.
        // They must be sorted in the order they were processed.
        vertices_processed_up_till_now: &[VertexIdx],
        // The attribute that is being predicted.
        // When used by the encoder, this is the complete attribute.
        // When used by the decoder, this is the data that is being decoded, and thus it is not complete.
        // Hence, expecially in the decoder, the element access can only be done by the index that is
        // an element of `vertices_processed_up_till_now`.
        attribute: &Attribute,
    ) -> NdVector<N, i32> {
        match self {
            PredictionScheme::DeltaPrediction(prediction) => {
                prediction.predict(i, vertices_processed_up_till_now, attribute)
            }
            PredictionScheme::DerivativePrediction(prediction) => {
                prediction.predict(i, vertices_processed_up_till_now, attribute)
            }
            PredictionScheme::MeshMultiParallelogramPrediction(prediction) => {
                prediction.predict(i, vertices_processed_up_till_now, attribute)
            }
            PredictionScheme::MeshParallelogramPrediction(prediction) => {
                prediction.predict(i, vertices_processed_up_till_now, attribute)
            }
            PredictionScheme::MeshNormalPrediction(prediction) => {
                prediction.predict(i, vertices_processed_up_till_now, attribute)
            }
            PredictionScheme::MeshPredictionForTextureCoordinates(prediction) => {
                prediction.predict(i, vertices_processed_up_till_now, attribute)
            }
            PredictionScheme::NoPrediction(_) => NdVector::zero(),
        }
    }

    /// Encodes the prediction metadata to the writer.
    pub fn encode_prediction_metadtata<W>(&self, writer: &mut W) -> Result<(), Err>
    where
        W: ByteWriter,
    {
        match self {
            PredictionScheme::DeltaPrediction(prediction) => {
                prediction.encode_prediction_metadtata(writer)
            }
            PredictionScheme::DerivativePrediction(prediction) => {
                prediction.encode_prediction_metadtata(writer)
            }
            PredictionScheme::MeshMultiParallelogramPrediction(prediction) => {
                prediction.encode_prediction_metadtata(writer)
            }
            PredictionScheme::MeshParallelogramPrediction(prediction) => {
                prediction.encode_prediction_metadtata(writer)
            }
            PredictionScheme::MeshNormalPrediction(prediction) => {
                prediction.encode_prediction_metadtata(writer)
            }
            PredictionScheme::MeshPredictionForTextureCoordinates(prediction) => {
                prediction.encode_prediction_metadtata(writer)
            }
            PredictionScheme::NoPrediction(_) => {
                // No metadata to encode.
                Ok(())
            }
        }
    }

    pub fn get_type(&self) -> PredictionSchemeType {
        match self {
            PredictionScheme::DeltaPrediction(_) => PredictionSchemeType::DeltaPrediction,
            PredictionScheme::DerivativePrediction(_) => PredictionSchemeType::DerivativePrediction,
            PredictionScheme::MeshMultiParallelogramPrediction(_) => {
                PredictionSchemeType::MeshMultiParallelogramPrediction
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
    const ID: u32 = 0;
    type AdditionalDataForMetadata = ();
    fn new(_parents: &[&'a Attribute], _ads: &'a D) -> Self {
        unreachable!()
    }

    fn get_values_impossible_to_predict(
        &mut self,
        _value_indices: &mut Vec<std::ops::Range<usize>>,
    ) -> Vec<std::ops::Range<usize>> {
        unreachable!()
    }
    fn predict(&mut self, _: CornerIdx, _: &[VertexIdx], _: &Attribute) -> NdVector<N, i32> {
        unreachable!()
    }
}
