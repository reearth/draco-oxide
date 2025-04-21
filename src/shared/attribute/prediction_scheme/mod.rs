pub mod delta_prediction;
pub mod mesh_parallelogram_prediction;
pub mod mesh_multi_parallelogram_prediction;
pub mod derivative_prediction;

use crate::core::{attribute::Attribute, shared::ConfigType};

/// PredictionScheme traits are not generic and the structs implementing the 
/// trait are generic. This is so because some of the structs need to store
/// the previous values in order to compute the current value.
pub trait PredictionScheme
{
	/// Id of the prediction method. This value is encoded to buffer in order
	/// for the decoder to identify the prediction method.
	const ID: u32 = 0;
	
	/// The type of the data that the prediction scheme will predict.
	/// The original data and the predicted data are of the same type.
	type Data;
	

	type AdditionalDataForMetadata;
	
	/// Clean the data from previous encoding.
	fn init(&mut self);
	
	/// Prediction computes the metadata beforehand (unlike transform or portabilization)
	fn compute_metadata(&mut self, faces: &[[usize; 3]], additional_data: Self::AdditionalDataForMetadata);

	fn get_values_impossible_to_predict(&mut self, value_indeces: &mut Vec<std::ops::Range<usize>>, faces: &[[usize; 3]]) 
		-> Vec<std::ops::Range<usize>>;
	
	/// predicts the attribute from the given information. 
	/// 'PointData' is a type representing a position i.e. it is an array of f32
	/// or f64 of size (typically) 2 or 3. It has to be generic since the data
	/// is not known at compile time.
	fn predict (
		&self,
		// Values that are encoded/decoded before the call to this function.
		values_encoded_up_till_now: &[Self::Data],

		// Parent attribute that is used for prediction.
		parents: &Vec<&Attribute>,

		// faces
		faces: &[[usize; 3]]
	) -> Self::Data;
}

#[derive(Clone, Copy)]
pub enum PredictionSchemeType
{
	DeltaPrediction,
	MeshParallelogramPrediction,
	MeshMultiParallelogramPrediction,
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

impl<Data> PredictionScheme for NoPrediction<Data> {
	const ID: u32 = 0;
	type Data = Data;
	type AdditionalDataForMetadata = ();
	fn init(&mut self) {
		unreachable!()
	}
	fn compute_metadata(&mut self, _faces: &[[usize; 3]], _additional_data: Self::AdditionalDataForMetadata) {
		unreachable!()
	}
	fn get_values_impossible_to_predict(&mut self, _value_indeces: &mut Vec<std::ops::Range<usize>>, _faces: &[[usize; 3]]) 
		-> Vec<std::ops::Range<usize>>
	{
		unreachable!()
	}
	fn predict(
		&self,
		_values_up_till_now: &[Self::Data],
		_parent: &Vec<&Attribute>,
		_faces: &[[usize; 3]]
	) -> Self::Data 
	{
		unreachable!()
	}
}
