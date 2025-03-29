mod delta_predition;
mod mesh_parallelogram_prediction;

use crate::core::attribute::Attribute;
use crate::core::shared::DataValue;
use crate::core::shared::Vector;

/// PredictionScheme traits are not generic and the structs implementing the 
/// trait are generic. This is so because some of the structs need to store
/// the previous values in order to compute the current value.
pub(crate) trait PredictionScheme
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

	fn get_values_impossible_to_predict(&mut self, value_indeces: Vec<usize>, faces: &[[usize; 3]]) 
		-> Vec<usize>;
	
	/// predicts the attribute from the given information. 
	/// 'PointData' is a type representing a position i.e. it is an array of f32
	/// or f64 of size (typically) 2 or 3. It has to be generic since the data
	/// is not known at compile time.
	fn predict (
		&self,
		// Values that are encoded/decoded before the call to this function.
		values_encoded_up_till_now: &[Self::Data],

		// Parent attribute that is used for prediction.
		parent: Vec<&Attribute>,

		// faces
		faces: &[[usize; 3]]
	) -> Self::Data;
}