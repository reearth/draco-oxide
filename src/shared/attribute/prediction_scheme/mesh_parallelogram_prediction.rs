use std::ops;

use super::PredictionScheme;
use crate::core::{attribute::Attribute, shared::Vector};

struct MeshParallelogramPrediction<Data> {
	_marker: std::marker::PhantomData<Data>
}

impl<Data> PredictionScheme for MeshParallelogramPrediction<Data> 
    where Data: Vector + Clone + ops::Add<Output = Data> + ops::Sub<Output = Data> 
{
    const ID: u32 = 2;
    
    type Data = Data;

    type AdditionalDataForMetadata = ();
	
	fn init(&mut self) {}

	fn compute_metadata(&mut self, faces: &[[usize; 3]], _additional_data: Self::AdditionalDataForMetadata) {

    }
	
	fn get_values_impossible_to_predict(&mut self, mut value_indeces: Vec<usize>, faces: &[[usize; 3]]) 
        -> Vec<usize> 
    {
        let mut is_already_encoded = Vec::new();
        let mut vertices_without_parallelogram = Vec::new();

        for face in faces {
            for vertex in face {
                if is_already_encoded.len() <= *vertex {
                    is_already_encoded.resize(*vertex + 1, false);
                    // Safety: just resized the vector to the correct size.
                    unsafe{
                        *is_already_encoded.get_unchecked_mut(*vertex) = true;
                    }
                    vertices_without_parallelogram.push(*vertex);
                } else if !is_already_encoded[*vertex] {
                    // Safety: just checked that the vertex is not encoded.
                    unsafe{
                        *is_already_encoded.get_unchecked_mut(*vertex) = true;
                    }
                    vertices_without_parallelogram.push(*vertex);
                }
            }
        }
        debug_assert!(vertices_without_parallelogram.is_sorted());
        vertices_without_parallelogram
    }
	
	fn predict(
		&self,
		values_up_till_now: &[Data],
		_: Vec<&Attribute>,
        faces: &[[usize; 3]]
	) -> Self::Data {
        let n_points = values_up_till_now.len();

        let first_face_oppsite_to_point = faces.iter()
            .filter(|f| f.contains(&n_points))
            .map(|&[a,b,c]| 
                if a == n_points {
                    [b, c]
                } else if b == n_points {
                    [a, c]
                } else {
                    [a, b]
                }
            )
            .find_map(|[a,b]| {
                // Todo: This can be highly optimized.
                let face = faces.iter()
                    .find(|f| f.contains(&a) && f.contains(&b) && !f.contains(&n_points));
                if let Some(face) = face {
                    let diagonal = face.iter()
                        .copied()
                        .find(|&v| v != a && v != b)
                        .unwrap();
                    Some([a, b, diagonal])
                }
                else {
                    None
                }
            })
            .unwrap();

        let [a, b, diagonal] = first_face_oppsite_to_point;

        let a_coord = values_up_till_now[a].clone();
        let b_coord = values_up_till_now[b].clone();
        let diagonal_coord = values_up_till_now[diagonal].clone();
        a_coord + b_coord - diagonal_coord
    }
}