use crate::{core::{attribute::Attribute, corner_table::GenericCornerTable}, prelude::{NdVector, Vector}};
use super::PredictionSchemeImpl;
use std::mem;

pub struct DeltaPrediction<'parents, C, const N: usize> 
{
	faces: &'parents [[usize; 3]],
	_marker: std::marker::PhantomData<&'parents C>,
}


impl<'parents, C, const N: usize> PredictionSchemeImpl<'parents, C, N> for DeltaPrediction<'parents, C, N>
	where C: GenericCornerTable,
	      NdVector<N, i32>: Vector<N, Component = i32>,
{
	const ID: u32 = 1;
	
	type AdditionalDataForMetadata = ();
	
	fn new(_parents: &[&'parents Attribute], _corner_table: &'parents C) -> Self {
        // Note: Connectivity is now passed via conn_att parameter instead of parent attributes
        // For now, use an empty slice as this prediction scheme needs to be updated for the new architecture
        let faces: &[[usize; 3]] = &[];

        Self {
            faces,
			_marker: std::marker::PhantomData,
        }   
	}
	
	fn get_values_impossible_to_predict(&mut self, value_indices: &mut Vec<std::ops::Range<usize>>) 
		-> Vec<std::ops::Range<usize>>
	{
		let mut self_ = Vec::new();
		let mut out = Vec::new();
		for r in value_indices.iter() {
			for i in r.clone() {
				if i == 0 {
					out.push(i);
				}
				// ToDo: Optimize this: 'self.faces' are the sorted array of sorted arrays.
				else if self.faces.iter().any(|f|f.contains(&(i-1))&&f.contains(&i)) {
					self_.push(i);
				} else {
					out.push(i);
				}
			}
		}
		let mut self_ = into_ranges(self_);
		mem::swap(&mut self_, value_indices);
		into_ranges(out)
	}
	
	fn predict(
		&mut self,
		_i: usize,
		vertices_or_corners_processed_up_till_now: &[usize],
		att: &Attribute,
	) -> NdVector<N, i32>
	{
		let prev_v = if let Some(prev_v) = vertices_or_corners_processed_up_till_now.last() {
			*prev_v
		} else {
			// If there are no previous vertices, we cannot predict the value.
			return NdVector::zero();
		};
		att.get(prev_v)
	}
}

fn into_ranges(v: Vec<usize>) -> Vec<std::ops::Range<usize>> {
	let mut out = Vec::new();
	if v.is_empty() {
		return out;
	}
	let mut start = v[0];
	let mut end = v[0];
	for &val in &v[1..] {
		if val != end + 1 {
			out.push(start..end + 1);
			start = val;
		}
		end = val;
	}
	out.push(start..end + 1);
	out
}

// ToDo: recover this test.
// #[cfg(test)]
// mod tests {
// 	use crate::{core::attribute::AttributeId, prelude::NdVector};

// use super::*;
	
// 	#[test]
// 	fn test_into_ranges() {
// 		let v = vec![1, 3, 6, 7, 8, 10, 11, 12, 15];
// 		let r = into_ranges(v);
// 		assert_eq!(r.len(), 5);	
// 		assert_eq!(r[0], 1..2);
// 		assert_eq!(r[1], 3..4);
// 		assert_eq!(r[2], 6..9);
// 		assert_eq!(r[3], 10..13);
// 		assert_eq!(r[4], 15..16);
// 	}

// 	#[test]
// 	fn test_get_values_impossible_to_predict() {
// 		let faces = vec![[0, 1, 2], [1, 2, 3], [4, 5, 6], [5, 6, 7]];
// 		let conn_att = Attribute::from_faces(
// 			AttributeId::new(0), 
// 			faces.clone(),
// 			Vec::new()
// 		);
// 		let mut delta = DeltaPrediction::<NdVector<3, f32>>::new(&[&conn_att]);
// 		let mut value_indices = vec![0..8];
// 		let impossible = delta.get_values_impossible_to_predict(&mut value_indices);
// 		assert_eq!(impossible.len(), 2);
// 		assert_eq!(impossible[0], 0..1);
// 		assert_eq!(impossible[1], 4..5);

// 		assert_eq!(value_indices.len(), 2);
// 		assert_eq!(value_indices[0], 1..4);
// 		assert_eq!(value_indices[1], 5..8);
// 	}
// }