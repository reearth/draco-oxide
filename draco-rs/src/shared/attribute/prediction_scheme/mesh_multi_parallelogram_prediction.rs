use crate::core::attribute::AttributeType;
use crate::shared::attribute::prediction_scheme::PredictionSchemeImpl;
use crate::core::shared::{DataValue, Vector};
use crate::core::attribute::Attribute;  
use crate::utils::merge_indices;
use std::{
    ops,
    mem,
};

pub struct MeshMultiParallelogramPrediction<'parents, Data> {
    faces: &'parents[[usize; 3]],
    _marker: std::marker::PhantomData<Data>,
}

impl<'parents, Data> PredictionSchemeImpl<'parents> for MeshMultiParallelogramPrediction<'parents, Data> 
    where 
        Data: Vector + Clone,
        Data::Component: DataValue
{
    const ID: u32 = 3;

    type Data = Data;
    type AdditionalDataForMetadata = ();

    fn new(parents: &[&'parents Attribute]) -> Self {
        let faces = unsafe {
            parents.iter().find(|p| p.get_attribute_type() == AttributeType::Connectivity)
                .expect("MeshMultiParallelogramPrediction: No connectivity attribute found")
                .as_slice_unchecked()
        };

        Self {
            faces,
            _marker: std::marker::PhantomData,
        }
    }

    fn compute_metadata(&mut self, _additional_data: Self::AdditionalDataForMetadata) {
        // Compute metadata if needed.
    }

    fn get_values_impossible_to_predict(&mut self, seq: &mut Vec<std::ops::Range<usize>>) -> Vec<std::ops::Range<usize>> {
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
        values_up_till_now: &[Self::Data],
    ) -> Self::Data {
        let n_points = values_up_till_now.len();

        let faces_opposite_to_point = self.faces.iter()
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
                let face = self.faces.iter()
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


    // #[test]
    fn test_predict() {
        let mut faces = [
            [0,1,5], [1,5,6], [1,2,6], [2,6,7], [2,3,7], [3,7,8], [3,4,8], [4,8,9],
            [5,6,10], [6,10,11], [6,7,11], [7,11,12], [7,8,12], [8,12,13], [8,9,13], [9,13,14],
            [10,11,15], [11,15,16], [11,12,16], [12,16,17], [12,13,17], [13,17,18], [13,14,18], [14,18,19],
            [15,16,20], [16,20,21], [16,17,21], [17,21,22], [17,18,22], [18,22,23], [18,19,23], [19,23,24]
        ];
        faces.sort();
        let points = {
            let n_points = 25;
            let mut points = Vec::new();
            for i in 0..n_points {
                let x = i % 5;
                let y = (i / 5) % 5;
                let z = x + y;
                points.push(NdVector::from([x as f32, y as f32, z as f32]));
            }
            points
        };

        let mut atts = vec![
            Attribute::from_faces(
                AttributeId::new(0),
                faces.to_vec(),
                Vec::new(),
            ),
            Attribute::from(
                AttributeId::new(1),
                points.clone(),
                AttributeType::Position,
                vec![
                    AttributeId::new(0),
                ],
            ),
        ];

        let mut encoder = Edgebreaker::new(Config::default());
        let mut buff_writer = Writer::<MsbFirst>::new();
        let mut writer = |input: (u8, u64)| buff_writer.next(input);
        let rerult = encoder.encode_connectivity(&mut faces, &mut [&mut atts[1]], &mut writer);
        assert!(rerult.is_ok());

        let atts = vec![
            &atts[0],
            &atts[1],
        ];

        let mut mesh_prediction = MeshMultiParallelogramPrediction::<NdVector<3, f32>>::new(&*atts);
        let mut seq = vec![0..points.len()];
        let impossible_to_predict = mesh_prediction.get_values_impossible_to_predict(&mut seq);
        
        let mut points_up_till_now = {
            // fill the answer for the vertices that are impossible to predict
            let mut out = vec![NdVector::from([0.0, 0.0, 0.0]); points.len()];
            for i in impossible_to_predict.into_iter().flatten() {
                out[i] = points[i];
            }
            out
        };

        let mut face_max_idx = 0;
        for i in seq.into_iter().flatten() {
            while !faces[face_max_idx].contains(&i) {
                face_max_idx += 1;
            }
            let predicted = mesh_prediction.predict(&points[..i]);
            // In this test, predtion and the original point are the same
            assert_eq!(predicted, points[i]);
            points_up_till_now[i] = predicted;

        }
    }
}