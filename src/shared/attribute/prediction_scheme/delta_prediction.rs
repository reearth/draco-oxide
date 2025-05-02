use crate::core::{attribute::Attribute, shared::Vector};
use std::marker::PhantomData;
use super::PredictionSchemeImpl;

pub struct DeltaPrediction<Data: Vector> {
	_marker: PhantomData<Data>,
}


impl<Data: Vector + Clone> PredictionSchemeImpl<'_> for DeltaPrediction<Data>
{
	const ID: u32 = 1;
	
	type Data = Data;

	type AdditionalDataForMetadata = ();
	
	fn new(_parents: &[&Attribute]) -> Self {
		Self { _marker: PhantomData }
	}
	
	// No metadata
	fn compute_metadata(&mut self, _additional_data: Self::AdditionalDataForMetadata) {}

	fn get_values_impossible_to_predict(&mut self, _value_indeces: &mut Vec<std::ops::Range<usize>>) 
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
		values_up_till_now: &[Data]
	) -> Self::Data 
	{
		values_up_till_now.last().unwrap().clone()
	}
}