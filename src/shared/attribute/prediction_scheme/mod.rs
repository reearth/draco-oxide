pub mod delta_prediction;
pub mod mesh_parallelogram_prediction;
pub mod mesh_multi_parallelogram_prediction;
pub mod derivative_prediction;

use crate::core::{attribute::Attribute, shared::{ConfigType, Vector}};

/// PredictionScheme traits are not generic and the structs implementing the 
/// trait are generic. This is so because some of the structs need to store
/// the previous values in order to compute the current value.
pub trait PredictionSchemeImpl<'a>
{
	/// Id of the prediction method. This value is encoded to buffer in order
	/// for the decoder to identify the prediction method.
	const ID: u32 = 0;
	
	/// The type of the data that the prediction scheme will predict.
	/// The original data and the predicted data are of the same type.
	type Data;
	

	type AdditionalDataForMetadata;
	
	/// Creates the prediction.
	fn new(parents: &[&'a Attribute]) -> Self;
	
	/// Prediction computes the metadata beforehand (unlike transform or portabilization)
	fn compute_metadata(&mut self, additional_data: Self::AdditionalDataForMetadata);

	fn get_values_impossible_to_predict(&mut self, value_indeces: &mut Vec<std::ops::Range<usize>>) 
		-> Vec<std::ops::Range<usize>>;
	
	/// predicts the attribute from the given information. 
	fn predict (
		&self,
		// Values that are encoded/decoded before the call to this function.
		values_encoded_up_till_now: &[Self::Data],
	) -> Self::Data;
}

#[remain::sorted]
#[derive(Clone, Copy)]
pub enum PredictionSchemeType
{
	DeltaPrediction,
	DerivativePrediction,
	MeshMultiParallelogramPrediction,
	MeshParallelogramPrediction,
	NoPrediction
}

#[remain::sorted]
pub enum PredictionScheme<'parents, Data>
	where Data: Vector
{
	DeltaPrediction(delta_prediction::DeltaPrediction<Data>),
	DerivativePrediction(derivative_prediction::DerivativePredictionForTextureCoordinates<'parents, Data>),
	MeshMultiParallelogramPrediction(mesh_multi_parallelogram_prediction::MeshMultiParallelogramPrediction<'parents, Data>),
	MeshParallelogramPrediction(mesh_parallelogram_prediction::MeshParallelogramPrediction<'parents, Data>),
	NoPrediction(NoPrediction<Data>),
}

impl<'parents, Data> PredictionScheme<'parents, Data>
	where Data: Vector
{
	pub(crate) fn new(cfg: Config, parents: &[&'parents Attribute]) -> Self {
		match cfg.prediction_scheme {
			PredictionSchemeType::DeltaPrediction => {
				let prediction = delta_prediction::DeltaPrediction::new(parents);
				PredictionScheme::DeltaPrediction(prediction)
			}
			PredictionSchemeType::DerivativePrediction => {
				let prediction = derivative_prediction::DerivativePredictionForTextureCoordinates::new(parents);
				PredictionScheme::DerivativePrediction(prediction)
			}
			PredictionSchemeType::MeshMultiParallelogramPrediction => {
				let prediction = mesh_multi_parallelogram_prediction::MeshMultiParallelogramPrediction::new(parents);
				PredictionScheme::MeshMultiParallelogramPrediction(prediction)
			}
			PredictionSchemeType::MeshParallelogramPrediction => {
				let prediction = mesh_parallelogram_prediction::MeshParallelogramPrediction::new(parents);
				PredictionScheme::MeshParallelogramPrediction(prediction)
			}
			PredictionSchemeType::NoPrediction => {
				let prediction = NoPrediction::new();
				PredictionScheme::NoPrediction(prediction)
			}
		}
	}

	pub(crate) fn get_values_impossible_to_predict(&mut self, value_indeces: &mut Vec<std::ops::Range<usize>>) 
		-> Vec<std::ops::Range<usize>>
	{
		match self {
			PredictionScheme::DeltaPrediction(prediction) => {
				prediction.get_values_impossible_to_predict(value_indeces)
			}
			PredictionScheme::DerivativePrediction(prediction) => {
				prediction.get_values_impossible_to_predict(value_indeces)
			}
			PredictionScheme::MeshMultiParallelogramPrediction(prediction) => {
				prediction.get_values_impossible_to_predict(value_indeces)
			}
			PredictionScheme::MeshParallelogramPrediction(prediction) => {
				prediction.get_values_impossible_to_predict(value_indeces)
			}
			PredictionScheme::NoPrediction(_) => {
				Vec::new()
			}
		}
	}
	
	pub(crate) fn predict (
		&self,
		// Values that are encoded/decoded before the call to this function.
		values_encoded_up_till_now: &[Data],
	) -> Data {
		match self {
			PredictionScheme::DeltaPrediction(prediction)=> {
				prediction.predict(values_encoded_up_till_now)
			}
			PredictionScheme::DerivativePrediction(prediction) => {
				prediction.predict(values_encoded_up_till_now)
			}
			PredictionScheme::MeshMultiParallelogramPrediction(prediction) => {
				prediction.predict(values_encoded_up_till_now)
			}
			PredictionScheme::MeshParallelogramPrediction(prediction) => {
				prediction.predict(values_encoded_up_till_now)
			}
			PredictionScheme::NoPrediction(_) => {
				Data::zero()
			}
		}
	}
}

#[derive(Clone)]
pub struct Config
{
	pub prediction_scheme: PredictionSchemeType,
	pub parents: Vec<usize>,
}

impl ConfigType for Config {
	fn default() -> Self {
		Config {
			prediction_scheme: PredictionSchemeType::DeltaPrediction,
			parents: Vec::new(),
		}
	}
}

pub struct NoPrediction<Data> {
	_marker: std::marker::PhantomData<Data>,
}

impl<Data> NoPrediction<Data> {
	pub fn new() -> Self {
		Self {
			_marker: std::marker::PhantomData,
		}
	}
}

impl<'a, Data> PredictionSchemeImpl<'a> for NoPrediction<Data> {
	const ID: u32 = 0;
	type Data = Data;
	type AdditionalDataForMetadata = ();
	fn new(_parents: &[&'a Attribute]) -> Self {
		unreachable!()
	}
	fn compute_metadata(&mut self, _additional_data: Self::AdditionalDataForMetadata) {
		unreachable!()
	}
	fn get_values_impossible_to_predict(&mut self, _value_indeces: &mut Vec<std::ops::Range<usize>>) 
		-> Vec<std::ops::Range<usize>>
	{
		unreachable!()
	}
	fn predict(
		&self,
		_values_up_till_now: &[Self::Data],
	) -> Self::Data 
	{
		unreachable!()
	}
}
