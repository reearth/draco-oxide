use std::ops;

use super::PredictionScheme;
use crate::core::{attribute::Attribute, shared::Vector};

pub(crate) struct MeshParallelogramPrediction<Data> {
	_marker: std::marker::PhantomData<Data>
}

impl<Data> MeshParallelogramPrediction<Data> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData
        }
    }
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
	
	fn get_values_impossible_to_predict(&mut self, value_indeces: &mut Vec<std::ops::Range<usize>>, faces: &[[usize; 3]]) 
        -> Vec<std::ops::Range<usize>>
    {
        // let mut is_already_encoded = Vec::new();
        // let mut vertices_without_parallelogram = Vec::new();

        // for face in faces {
        //     if face.iter()
        //         .all(|&v| v>=is_already_encoded.len() || is_already_encoded[v] == false) 
        //     {
        //         vertices_without_parallelogram.append(&mut face.into());
        //     }
        //     for &v in face {
        //         if v >= is_already_encoded.len() {
        //             is_already_encoded.resize(v + 1, false);
        //         }
        //         is_already_encoded[v] = true;
        //     }
        // }
        // vertices_without_parallelogram.sort();

        // // splice 'vertices_without_parallelogram' with 'value_indeces'
        // {
        //     let mut iter1 = vertices_without_parallelogram.into_iter();
        //     let mut iter2 = value_indeces.into_iter();
        //     let mut value = iter1.next().unwrap();
        //     let mut iter1_not_iter2 = false;
        //     let mut splice = Vec::new();
        //     while let Some(next) = 
        //         if iter1_not_iter2{
        //             iter1.next()
        //         } else {
        //             iter2.next()
        //     } {
        //         if value == next {
        //             splice.push(value);
        //         } else if value < next {
        //             value = next;
        //             iter1_not_iter2 = !iter1_not_iter2;
        //         }
        //     }
        //     splice
        // }
        unimplemented!()
    }
	
	fn predict(
		&self,
		values_up_till_now: &[Data],
		_: &Vec<&Attribute>,
        faces: &[[usize; 3]]
	) -> Self::Data {
        let n_points = values_up_till_now.len();

        let first_face_opposite_to_point = faces.iter()
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

        let [a, b, diagonal] = first_face_opposite_to_point;

        let a_coord = values_up_till_now[a].clone();
        let b_coord = values_up_till_now[b].clone();
        let diagonal_coord = values_up_till_now[diagonal].clone();
        a_coord + b_coord - diagonal_coord
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use crate::{core::shared::NdVector, shared::attribute::prediction_scheme::PredictionScheme};
    
    fn get_torus() -> Vec<[usize;3]> {
        let mut faces = vec![
            [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
        ];
        faces.sort();

        faces
    }

    #[test]
    fn test_get_impossible_to_predict() {        
        let faces = [[0,1,2],[1,2,3],[2,3,4],[5,6,7],[6,7,8],[7,8,9]];
        let points = vec![0.0; faces.iter().flatten().max().unwrap()+1];
        let mut mesh_prediction = MeshParallelogramPrediction::<NdVector<3, f32>>::new();
        let impossible_to_predict = mesh_prediction.get_values_impossible_to_predict(&mut vec![0..points.len()], &faces);
        assert_eq!(&impossible_to_predict, &vec![0..3, 5..8]);

        let faces = (0..10).map(|i| [3*i, 3*i+1, 3*i+2]).collect::<Vec<_>>();
        let points = vec![0.0; faces.iter().flatten().max().unwrap()+1];
        let mut mesh_prediction = MeshParallelogramPrediction::<NdVector<3, f32>>::new();
        let impossible_to_predict = mesh_prediction.get_values_impossible_to_predict(&mut vec![0..points.len()], &faces);
        assert_eq!(impossible_to_predict, vec![0..points.len()]);
    }
}