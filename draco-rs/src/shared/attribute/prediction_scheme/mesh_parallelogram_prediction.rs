use std::{
    mem,
    ops
};

use super::PredictionSchemeImpl;
use crate::core::attribute::AttributeType;
use crate::core::{attribute::Attribute, shared::Vector};
use crate::utils::merge_indices;

pub struct MeshParallelogramPrediction<'parents, Data> {
    faces: &'parents[[usize; 3]],
	_marker: std::marker::PhantomData<Data>
}

impl<'parents, Data> PredictionSchemeImpl<'parents> for MeshParallelogramPrediction<'parents, Data> 
    where Data: Vector + Clone + ops::Add<Output = Data> + ops::Sub<Output = Data> 
{
    const ID: u32 = 2;
    
    type Data = Data;

    type AdditionalDataForMetadata = ();
	
	fn new(parents: &[&'parents Attribute] ) -> Self {
        let faces = unsafe {
            parents.iter().find(|x| x.get_attribute_type() == AttributeType::Connectivity)
                .expect("MeshParallelogramPrediction: No connectivity attribute found")
                .as_slice_unchecked::<[usize;3]>()
        };
        Self {
            faces,
            _marker: std::marker::PhantomData,
        }
    }

	fn compute_metadata(&mut self, _additional_data: Self::AdditionalDataForMetadata) {
        
    }
	
	fn get_values_impossible_to_predict(&mut self, seq: &mut Vec<std::ops::Range<usize>>) 
        -> Vec<std::ops::Range<usize>>
    {
        let mut is_already_encoded: Vec<bool> = Vec::new();
        let mut vertices_without_parallelogram: Vec<ops::Range<usize>> = Vec::new();

        for face in self.faces {
            let mut face = *face;
            face.sort();
            let num_unvisited_vertices = face.iter()
                .filter(|&&v| v>=is_already_encoded.len() || !is_already_encoded[v])
                .count();
            if num_unvisited_vertices == 3 {
                // In the standard edgebreaker decoding, only unpredictable faces are 
                // the first ones getting encoded among a connected component.
                // In the reverse-play decoding, only unpredictable faces are
                // the ones that correspond to the 'E' symbol.
                if face[0]+1 == face[1] && face[1]+1 == face[2] {
                    vertices_without_parallelogram.push(face[0]..face[2]+1);
                } else if face[0]+1 == face[1] {
                    vertices_without_parallelogram.push(face[0]..face[1]+1);
                    vertices_without_parallelogram.push(face[2]..face[2]+1);
                } else if face[1]+1 == face[2] {
                    vertices_without_parallelogram.push(face[0]..face[0]+1);
                    vertices_without_parallelogram.push(face[1]..face[2]+1);
                } else {
                    vertices_without_parallelogram.push(face[0]..face[0]+1);
                    vertices_without_parallelogram.push(face[1]..face[1]+1);
                    vertices_without_parallelogram.push(face[2]..face[2]+1);
                }
            } else if num_unvisited_vertices == 2 {
                let unvisited_vertices = face.into_iter()
                    .filter(|&v| v>=is_already_encoded.len() || !is_already_encoded[v])
                    .collect::<Vec<_>>();
                let idx1 = unvisited_vertices[0];
                let idx2 = unvisited_vertices[1];
                if idx1+1 == idx2 {
                    vertices_without_parallelogram.push(idx1..idx2+1);
                } else {
                    vertices_without_parallelogram.push(idx1..idx1+1);
                    vertices_without_parallelogram.push(idx2..idx2+1);
                }
            }
            for v in face {
                if v >= is_already_encoded.len() {
                    is_already_encoded.resize(v + 1, false);
                }
                // ToDo: Remove check
                is_already_encoded[v] = true;
            }
        }
        vertices_without_parallelogram.sort_by(|a,b| a.start.cmp(&b.start));
        // merge 'vertices_without_parallelogram' with 'value_indices'
        let merged = merge_indices(vec![seq.clone(), vertices_without_parallelogram]);
        // modify seq not to contain the merged ranges
        let mut new_seq = Vec::new();
        let mut seq_iter = mem::take(seq).into_iter();
        let mut merged_iter = merged.iter();
        new_seq.push(seq_iter.next().unwrap());
        // Safety: just added an element to 'new_seq'
        let mut r = unsafe {
            new_seq.last().unwrap_unchecked().clone() // this clone is cheap
        };
        let mut m = merged_iter.next().unwrap();
        loop {
            if m.start < r.start {
                m = if let Some(m) = merged_iter.next() {
                    m
                } else {
                    seq_iter.for_each(|r| new_seq.push(r.clone()));
                    break;
                };
                continue;
            }

            if m.start > r.end {
                let new_r = if let Some(r) = seq_iter.next() {
                    r
                } else {
                    break;
                };
                new_seq.push(new_r);
                // Safety: just added an element to 'new_seq'
                r = unsafe {
                    new_seq.last().unwrap_unchecked().clone() // this clone is cheap
                };
                continue;
            }
            // The following cases are impossible since the 'seq' contains 'merged': 
            
            // [    m    )
            //    [    r    )
            debug_assert!(!(r.start > m.start && r.start < m.end && r.end > m.end));

            //     [    m    )
            // [    r    )
            debug_assert!(!(r.start > m.start && r.end > m.start && r.end < m.end));

            // [    m    )
            //   [  r  )
            debug_assert!(!(r.start < m.start && r.end > m.start && r.end < m.end));

            
            // The following cases are the only possibilities:

            // [  m  )
            // [    r    )
            if r.start == m.start && m.end < r.end {
                unsafe {
                    *new_seq.last_mut().unwrap_unchecked() = m.end..r.end;
                };
                r = m.end..r.end;
            }

            //   [  m  )
            // [    r    )
            else if r.start < m.start && m.end < r.end {
                unsafe {
                    *new_seq.last_mut().unwrap_unchecked() = r.start..m.start;
                };
                new_seq.push(m.end..r.end);
                r = m.end..r.end;
            }

            // [  m  )
            // [  r  )
            else if r == *m {
                new_seq.pop();
                
                r = if let Some(r) = seq_iter.next() {
                    r
                } else {
                    break;
                };
                new_seq.push(r.clone());
                m = if let Some(m) = merged_iter.next() {
                    m
                } else {
                    seq_iter.for_each(|r| new_seq.push(r.clone()));
                    break;
                };
            }

            // No overlap
            else {
                m = if let Some(m) = merged_iter.next() {
                    m
                } else {
                    seq_iter.for_each(|r| new_seq.push(r.clone()));
                    break;
                };
            }
        }
        
        mem::swap(seq, &mut new_seq);

        merged
    }
	
	fn predict(
		&self,
		values_up_till_now: &[Data],
	) -> Self::Data {
        let n_points = values_up_till_now.len();

        // Find the the first opposite face.
        // 'diagonal' is the vertex opposite to 'n_points', and 'a' and 'b' are the other points such that 'a<b',
        // so that 'a', 'n_points', 'b', 'diagonal' form a parallelogram.
        let [a,b,diagonal] = self.faces.iter()
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
                if a >= n_points || b >= n_points {
                    return None;
                }
                let face = self.faces.iter()
                    .filter(|f| f.contains(&a) && f.contains(&b) && !f.contains(&n_points))
                    .find(|f| f.iter().all(|&v| v<n_points));
                if let Some(face) = face {
                    let diagonal = *face.iter()
                        .find(|&&v| v != a && v != b )
                        .unwrap();
                    Some([a, b, diagonal])
                }
                else {
                    None
                }
            })
            .unwrap();

        let a_coord = values_up_till_now[a].clone();
        let b_coord = values_up_till_now[b].clone();
        let diagonal_coord = values_up_till_now[diagonal].clone();
        a_coord + b_coord - diagonal_coord
    }
}

#[cfg(not(feature = "evaluation"))]
#[cfg(test)]
mod test {
    use std::vec;

    use super::*;
    use crate::core::attribute::AttributeId;
    use crate::core::buffer::writer::Writer;
    use crate::core::buffer::MsbFirst;
    use crate::core::shared::{ConfigType, NdVector}; 
    use crate::encode::connectivity::{edgebreaker::{Config, Edgebreaker}, ConnectivityEncoder}; 
    use crate::shared::attribute::prediction_scheme::PredictionSchemeImpl;

    #[test]
    fn test_get_impossible_to_predict_1() {
        // create a mesh that is a disjoint union of two meshes
        let faces = {
            let mut torus_in_decoded_order = vec![
                [0,1,2], [1,3,4], [0,1,3], [0,3,5], [2,6,7], [4,7,8], [6,7,8], [5,6,8], 
                [5,8,9], [0,5,9], [0,9,10], [0,2,10], [2,7,10], [7,10,11], [4,7,11], [3,4,11], 
                [3,11,12], [3,5,12], [5,6,12], [6,12,13], [2,6,13], [1,2,13], [1,13,14], [1,4,14], 
                [4,8,14], [8,9,14], [9,14,15], [9,10,15], [10,11,15], [11,12,15], [12,13,15], [13,14,15]
            ];

            let mut square_in_decoded_order = vec![
                [0,1,2], [3,4,5], [4,6,7], [3,4,6], [3,6,8], [3,8,9], [8,9,10], [9,10,11], 
                [10,11,12], [11,12,13], [1,11,13], [1,13,14], [0,1,14], [0,14,15], [15,16,17], [0,15,16], 
                [0,16,18], [0,2,18], [2,18,19], [20,21,22], [19,20,21], [2,19,21], [2,21,23], [1,2,23],
                [1,11,23], [9,11,23], [9,23,24], [3,9,24], [3,5,24], [5,22,24], [21,22,24], [21,23,24]
            ];

            let num_pts_in_torus = torus_in_decoded_order.iter().flatten().max().unwrap()+1;
            for f in &mut square_in_decoded_order {
                for i in 0..3 {
                    f[i] += num_pts_in_torus;
                }
            }
            torus_in_decoded_order.append(&mut square_in_decoded_order);
            torus_in_decoded_order
        };

        let points_len = faces.iter().flatten().max().unwrap()+1;
        let points = vec![NdVector::<3,f64>::zero(); points_len];

        let parents = vec![
            Attribute::from_faces(
                AttributeId::new(0),
                faces,
                vec![]
            ),
            Attribute::from(
                AttributeId::new(1),
                points,
                AttributeType::Position,
                vec![]
            )
        ];
        let parents = [
            &parents[0],
            &parents[1]
        ];
        
        let mut mesh_prediction = MeshParallelogramPrediction::<NdVector<3, f32>>::new(&parents);
        let mut seq = vec![0..points_len];
        let impossible_to_predict = mesh_prediction.get_values_impossible_to_predict(&mut seq);
        assert_eq!(seq, vec![5..6, 8..16, 24..32, 34..36, 39..41]);
        assert_eq!(&impossible_to_predict, &vec![0..5, 6..8, 16..24, 32..34, 36..39]);

    }

    #[test]
    fn test_get_impossible_to_predict_2() {
        let faces = (0..10).map(|i| [3*i, 3*i+1, 3*i+2]).collect::<Vec<_>>();
        let points_len = faces.iter().flatten().max().unwrap()+1;
        let points = vec![NdVector::<3,f64>::zero(); points_len];
        let parents = vec![
            Attribute::from_faces(
                AttributeId::new(0),
                faces,
                Vec::new()
            ),
            Attribute::from(
                AttributeId::new(1),
                points,
                AttributeType::Position,
                vec![AttributeId::new(0)]
            )
        ];
        let parents = [
            &parents[0],
            &parents[1]
        ];
        let mut mesh_prediction = MeshParallelogramPrediction::<NdVector<3, f32>>::new(&parents);
        let mut seq = vec![0..points_len];
        let impossible_to_predict = mesh_prediction.get_values_impossible_to_predict(&mut seq);
        assert_eq!(seq, vec![]);
        assert_eq!(impossible_to_predict, vec![0..points_len]);
    }

    #[test]
    fn test_predict() {
        // square
        let mut faces = vec![
            [9,23,24], [8,9,23], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,22,23], [7,8,22], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,21,22], [6,7,21], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,20,21], [6,19,20], [5,6,19], [5,18,19], [4,5,18], [4,17,18], [4,15,17], [15,16,17]
        ];
        faces.sort();
        let points_len = 25;
        let points = {
            let mut points = Vec::new();
            for i in 0..points_len {
                let x = i % 5;
                let y = (i / 5) % 5;
                let z = x + y;
                points.push(NdVector::from([x as f32, y as f32, z as f32]));
            }
            let map = vec![
                24,9,10,11,12,
                23,8,1,2,13,
                22,7,0,3,14,
                21,6,5,4,15,
                20,19,18,17,16
            ];
            let mut sorted_points = vec![NdVector::zero(); points_len];
            for (i, f)in map.into_iter().enumerate() {
                sorted_points[f] = points[i]; 
            }
            sorted_points
        };

        let mut point_att = Attribute::from(
            AttributeId::new(1),
            points.clone(),
            AttributeType::Position,
            vec![AttributeId::new(0)]
        );

        let mut encoder = Edgebreaker::new(Config::default());
        let mut buff_writer = Writer::<MsbFirst>::new();
        let mut writer = |input: (u8, u64)| buff_writer.next(input);
        let result = encoder.encode_connectivity(&mut faces, &mut [&mut point_att], &mut writer);
        assert!(result.is_ok());


        let parent = Attribute::from_faces(
                AttributeId::new(0),
                faces.to_vec(),
                Vec::new()
            );
        let parents = [
            &parent
        ];

        let mut mesh_prediction = MeshParallelogramPrediction::<NdVector<3, f32>>::new(&parents);
        let mut seq = vec![0..points_len];
        let impossible_to_predict = mesh_prediction.get_values_impossible_to_predict(&mut seq);

        assert_eq!(impossible_to_predict, vec![0..8, 16..18, 20..23]); // faces corresponding to the 'E's
        
        let mut points_up_till_now = {
            // fill the answer for the vertices that are impossible to predict
            let mut out = vec![NdVector::from([0.0, 0.0, 0.0]); points_len];
            out[0] = NdVector::from([3.0, 3.0, 6.0]); // 0
            out[1] = NdVector::from([2.0, 3.0, 5.0]); // 1
            out[2] = NdVector::from([3.0, 2.0, 5.0]); // 2
            out[3] = NdVector::from([1.0, 1.0, 2.0]); // 3
            out[4] = NdVector::from([1.0, 0.0, 1.0]); // 4
            out[5] = NdVector::from([2.0, 0.0, 2.0]); // 5
            out[6] = NdVector::from([0.0, 1.0, 1.0]); // 6
            out[7] = NdVector::from([0.0, 0.0, 0.0]); // 7

            out[16] = NdVector::from([4.0, 3.0, 7.0]); // 16
            out[17] = NdVector::from([4.0, 2.0, 6.0]); // 17

            out[20] = NdVector::from([4.0, 0.0, 4.0]); // 20
            out[21] = NdVector::from([3.0, 1.0, 4.0]); // 21
            out[22] = NdVector::from([3.0, 0.0, 3.0]); // 22

            out
        };

        let mut face_max_idx = 0;
        for i in seq.into_iter().flatten() {
            while !faces[face_max_idx].contains(&i) {
                face_max_idx += 1;
            }
            let predicted = mesh_prediction.predict(&point_att.as_slice()[..i]);
            // In this test, prediction and the original point are the same
            assert_eq!(predicted, point_att.get(i), "i: {}, ", i);
            points_up_till_now[i] = predicted;
        }
    }
}