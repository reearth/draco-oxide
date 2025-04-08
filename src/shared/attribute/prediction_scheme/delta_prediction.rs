use crate::core::{attribute::Attribute, shared::Vector};
use std::marker::PhantomData;
use super::PredictionScheme;

pub struct DeltaPrediction<Data: Vector> {
	_marker: PhantomData<Data>,
}

impl<Data: Vector> DeltaPrediction<Data> {
	pub fn new() -> Self {
		Self {
			_marker: PhantomData,
		}
	}
}


impl<Data: Vector + Clone> PredictionScheme for DeltaPrediction<Data>
{
	const ID: u32 = 1;
	
	type Data = Data;

	type AdditionalDataForMetadata = ();
	
	fn init(&mut self) {

	}
	
	// No metadata
	fn compute_metadata(&mut self, _faces: &[[usize;3]],_additional_data: Self::AdditionalDataForMetadata) {}

	fn get_values_impossible_to_predict(&mut self, value_indeces: &mut Vec<std::ops::Range<usize>>, _faces: &[[usize; 3]]) 
		-> Vec<std::ops::Range<usize>>
	{
		unimplemented!()
		// if let Some(indeces) = value_indeces.into_iter().next() {
		// 	if indeces == 0 {
		// 		return vec![0]
		// 	}
		// }
		// Vec::new()
	}
	
	fn predict(
		&self,
		values_up_till_now: &[Data],
		_parent: &Vec<&Attribute>,
		_faces: &[[usize; 3]]
	) -> Self::Data 
	{
		values_up_till_now.last().unwrap().clone()
	}
}