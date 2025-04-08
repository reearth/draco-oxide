use crate::shared::attribute::prediction_scheme::PredictionScheme;
use crate::core::shared::{DataValue, Vector};
use crate::core::attribute::Attribute;  

pub struct MeshMultiParallelogramPrediction<Data> {
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> MeshMultiParallelogramPrediction<Data> {
    pub fn new() -> Self {
        MeshMultiParallelogramPrediction {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Data> PredictionScheme for MeshMultiParallelogramPrediction<Data> 
    where 
        Data: Vector + Clone,
        Data::Component: DataValue
{
    const ID: u32 = 3;

    type Data = Data;
    type AdditionalDataForMetadata = ();

    fn init(&mut self) {
        // Initialize any necessary state here.
    }

    fn compute_metadata(&mut self, _faces: &[[usize; 3]], _additional_data: Self::AdditionalDataForMetadata) {
        // Compute metadata if needed.
    }

    fn get_values_impossible_to_predict(&mut self, value_indices: &mut Vec<std::ops::Range<usize>>, _faces: &[[usize; 3]]) -> Vec<std::ops::Range<usize>> {
        unimplemented!()
    }
    fn predict(
        &self,
        values_up_till_now: &[Self::Data],
        _parent: &Vec<&Attribute>,
        faces: &[[usize; 3]]
    ) -> Self::Data {
        let n_points = values_up_till_now.len();

        let faces_opposite_to_point = faces.iter()
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
            .filter_map(|[a,b]| {
                // Todo: This can be highly optimized.
                let face = faces.iter()
                    .filter(|f| f.iter().all(|&v| v <= n_points))
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
            .collect::<Vec<_>>();

        let mut sum = Self::Data::zero();
        let len = Data::Component::from_u64(faces_opposite_to_point.len() as u64);
        for [a, b, diagonal] in faces_opposite_to_point {
            let a_coord = values_up_till_now[a].clone();
            let b_coord = values_up_till_now[b].clone();
            let diagonal_coord = values_up_till_now[diagonal].clone();
            sum += a_coord + b_coord - diagonal_coord
        }

        sum/len
    }
}