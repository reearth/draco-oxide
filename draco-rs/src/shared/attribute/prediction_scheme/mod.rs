pub mod delta_prediction;
pub mod mesh_parallelogram_prediction;
pub mod mesh_multi_parallelogram_prediction;
pub mod derivative_prediction;

use crate::{core::{attribute::Attribute, corner_table::GenericCornerTable, shared::{ConfigType, Vector, VertexIdx}}, prelude::{ByteReader, ByteWriter, NdVector}};

/// PredictionScheme traits are not generic and the structs implementing the 
/// trait are generic. This is so because some of the structs need to store
/// the previous values in order to compute the current value.
pub trait PredictionSchemeImpl<'parents, C, const N: usize>
	where C: GenericCornerTable,
	      NdVector<N,i32>: Vector<N, Component = i32>
{
	/// Id of the prediction method. This value is encoded to buffer in order
	/// for the decoder to identify the prediction method.
	const ID: u32 = 0;
	
	type AdditionalDataForMetadata;

	/// Creates the prediction.
	fn new(parents: &[&'parents Attribute], conn_att: &'parents C ) -> Self;
	
	/// Prediction computes the metadata beforehand (unlike transform or portabilization)
	fn compute_metadata(&mut self, additional_data: Self::AdditionalDataForMetadata);

	fn get_values_impossible_to_predict(&mut self, value_indices: &mut Vec<std::ops::Range<usize>>) 
		-> Vec<std::ops::Range<usize>>;
	
	/// predicts the attribute from the given information. 
	fn predict (
		&self,
		// Vertex/corner index to predict.
		i: usize,
		// Vertices/corners processed before the call to this function.
		// They must be sorted in the order they were processed.
		vertices_or_corners_processed_up_till_now: &[VertexIdx],
		// The attribute that is being predicted.
		// When used by the encoder, this is the complete attribute.
		// When used by the decoder, this is the data that is being decoded, and thus it is not complete.
		// Hence, expecially in the decoder, the element access can only be done by the index that is
		// an element of `vertices_or_corners_processed_up_till_now`.
		attribute: &[NdVector<N,i32>],
	) -> NdVector<N,i32>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredictionSchemeType
{
	DeltaPrediction,
	DerivativePrediction,
	MeshMultiParallelogramPrediction,
	MeshParallelogramPrediction,
	NoPrediction,
	Invalid, 
}

impl PredictionSchemeType {
	pub(crate) fn get_id(&self) -> u8 {
		match self {
			PredictionSchemeType::DeltaPrediction => 0,
			PredictionSchemeType::MeshParallelogramPrediction => 1,
			PredictionSchemeType::MeshMultiParallelogramPrediction => 2,
			PredictionSchemeType::DerivativePrediction => 5,

			PredictionSchemeType::NoPrediction => 0xFE, // -2 in i8
			PredictionSchemeType::Invalid => 0xFF, // -1 in i8
		}
	}

	pub(crate) fn write_to<W>(self, writer: &mut W) 
		where W: ByteWriter
	{
		let id = self.get_id();
		writer.write_u8(id);
	}

	pub(crate) fn read_from<R>(reader: &mut R) -> Result<Self, usize> 
		where R: ByteReader
	{
		let id = reader.read_u8().unwrap() as usize; // ToDo: handle error.
		let out = match id {
			0 => PredictionSchemeType::DeltaPrediction,
			1 => PredictionSchemeType::MeshParallelogramPrediction,
			2 => PredictionSchemeType::MeshMultiParallelogramPrediction,
			5 => PredictionSchemeType::DerivativePrediction,
			0xFE => PredictionSchemeType::NoPrediction, // -2 in i8
			0xFF => PredictionSchemeType::Invalid, // -1 in i8
			// If the id is not recognized, return an error.
			_ => return Err(id as usize),
		};
		Ok(out)
	}

	pub fn to_string(&self) -> String {
		match self {
			PredictionSchemeType::DeltaPrediction => "DeltaPrediction".to_string(),
			PredictionSchemeType::DerivativePrediction => "DerivativePrediction".to_string(),
			PredictionSchemeType::MeshMultiParallelogramPrediction => "MeshMultiParallelogramPrediction".to_string(),
			PredictionSchemeType::MeshParallelogramPrediction => "MeshParallelogramPrediction".to_string(),
			PredictionSchemeType::NoPrediction => "NoPrediction".to_string(),
			PredictionSchemeType::Invalid => "Invalid".to_string(),
		}
	}
}

#[remain::sorted]
pub(crate) enum PredictionScheme<'parents, C, const N: usize>
{
	DeltaPrediction(delta_prediction::DeltaPrediction<'parents, C, N>),
	DerivativePrediction(derivative_prediction::DerivativePredictionForTextureCoordinates<'parents, C, N>),
	MeshMultiParallelogramPrediction(mesh_multi_parallelogram_prediction::MeshMultiParallelogramPrediction<'parents, C, N>),
	MeshParallelogramPrediction(mesh_parallelogram_prediction::MeshParallelogramPrediction<'parents, C, N>),
	NoPrediction(NoPrediction),
}

impl<'parents, C, const N: usize> PredictionScheme<'parents, C, N>
	where 
		C: GenericCornerTable,
		NdVector<N,i32>: Vector<N, Component = i32>,
{
	pub(crate) fn new(ty: PredictionSchemeType, parents: &[&'parents Attribute], corner_table: &'parents C) -> Self {
		match ty {
			PredictionSchemeType::DeltaPrediction => {
				let prediction = delta_prediction::DeltaPrediction::new(parents, corner_table);
				PredictionScheme::DeltaPrediction(prediction)
			}
			PredictionSchemeType::DerivativePrediction => {
				let prediction = derivative_prediction::DerivativePredictionForTextureCoordinates::new(
					parents, corner_table
				);
				PredictionScheme::DerivativePrediction(prediction)
			}
			PredictionSchemeType::MeshMultiParallelogramPrediction => {
				let prediction = mesh_multi_parallelogram_prediction::MeshMultiParallelogramPrediction::new(
					parents, corner_table
				);
				PredictionScheme::MeshMultiParallelogramPrediction(prediction)
			}
			PredictionSchemeType::MeshParallelogramPrediction => {
				let prediction = mesh_parallelogram_prediction::MeshParallelogramPrediction::new(
					parents, corner_table
				);
				PredictionScheme::MeshParallelogramPrediction(prediction)
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

	pub(crate) fn read_from<R>(reader: &mut R, parents: &[&'parents Attribute], conn_att: &'parents C ) -> Result<Self, usize> 
		where R: ByteReader
	{
		let ty = PredictionSchemeType::read_from(reader)?;
		Ok(Self::new(ty, parents, conn_att))
	}

	pub(crate) fn get_values_impossible_to_predict(&mut self, value_indices: &mut Vec<std::ops::Range<usize>>) 
		-> Vec<std::ops::Range<usize>>
	{
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
			PredictionScheme::NoPrediction(_) => {
				Vec::new()
			}
		}
	}
	
	pub(crate) fn predict (
		&self,
		// Vertex/corner index to predict.
		i: usize,
		// Vertices/corners processed before the call to this function.
		// They must be sorted in the order they were processed.
		vertices_or_corners_processed_up_till_now: &[VertexIdx],
		// The attribute that is being predicted.
		// When used by the encoder, this is the complete attribute.
		// When used by the decoder, this is the data that is being decoded, and thus it is not complete.
		// Hence, expecially in the decoder, the element access can only be done by the index that is
		// an element of `vertices_or_corners_processed_up_till_now`.
		attribute: &[NdVector<N,i32>],
	) -> NdVector<N,i32> {
		match self {
			PredictionScheme::DeltaPrediction(prediction)=> {
				prediction.predict(i, vertices_or_corners_processed_up_till_now, attribute)
			}
			PredictionScheme::DerivativePrediction(prediction) => {
				prediction.predict(i, vertices_or_corners_processed_up_till_now, attribute)
			}
			PredictionScheme::MeshMultiParallelogramPrediction(prediction) => {
				prediction.predict(i, vertices_or_corners_processed_up_till_now, attribute)
			}
			PredictionScheme::MeshParallelogramPrediction(prediction) => {
				prediction.predict(i, vertices_or_corners_processed_up_till_now, attribute)
			}
			PredictionScheme::NoPrediction(_) => {
				NdVector::zero()
			}
		}
	}

	pub(crate) fn get_type(&self) -> PredictionSchemeType {
		match self {
			PredictionScheme::DeltaPrediction(_) => PredictionSchemeType::DeltaPrediction,
			PredictionScheme::DerivativePrediction(_) => PredictionSchemeType::DerivativePrediction,
			PredictionScheme::MeshMultiParallelogramPrediction(_) => PredictionSchemeType::MeshMultiParallelogramPrediction,
			PredictionScheme::MeshParallelogramPrediction(_) => PredictionSchemeType::MeshParallelogramPrediction,
			PredictionScheme::NoPrediction(_) => PredictionSchemeType::NoPrediction,
		}
	}
}

#[derive(Clone, Debug)]
pub struct Config
{
	pub ty: PredictionSchemeType,
	pub parents: Vec<usize>,
}

impl ConfigType for Config {
	fn default() -> Self {
		Config {
			ty: PredictionSchemeType::DeltaPrediction,
			parents: Vec::new(),
		}
	}
}

pub struct NoPrediction {}

impl NoPrediction {
	pub fn new() -> Self {
		Self{}
	}
}

impl<'a, C, const N: usize> PredictionSchemeImpl<'a, C, N> for NoPrediction 
	where C: GenericCornerTable,
	      NdVector<N,i32>: Vector<N, Component = i32>,
{
	const ID: u32 = 0;
	type AdditionalDataForMetadata = ();
	fn new(_parents: &[&'a Attribute], _conn_att: &'a C) -> Self {
		unreachable!()
	}
	fn compute_metadata(&mut self, _additional_data: Self::AdditionalDataForMetadata) {
		unreachable!()
	}
	fn get_values_impossible_to_predict(&mut self, _value_indices: &mut Vec<std::ops::Range<usize>>) 
		-> Vec<std::ops::Range<usize>>
	{
		unreachable!()
	}
	fn predict(
		&self,
		_i: usize,
		_vertices_or_corners_processed_up_till_now: &[VertexIdx],
		_attribute: &[NdVector<N,i32>],
	) -> NdVector<N,i32> {
		unreachable!()
	}
}
