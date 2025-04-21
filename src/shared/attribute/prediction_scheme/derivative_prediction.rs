use crate::core::attribute::{Attribute, AttributeType, ComponentDataType};
use crate::core::shared::{Cross, DataValue, Float, NdVector, Vector};
use super::PredictionScheme;
use std::{
	ops,
	mem,
};
use crate::core::shared::Dot;
use crate::utils::merge_indeces;

pub struct DerivativePredictionForTextureCoordinates<Data> {
    _marker: std::marker::PhantomData<Data>,
}

impl<Data> DerivativePredictionForTextureCoordinates<Data> 
	where 
		Data: Vector, 
		Data::Component: DataValue
{
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

	fn predict_impl<F>(&self, values_up_till_now: &[Data], points: &[NdVector<3,F>], faces: &[[usize; 3]]) -> Data 
		where
			F: Float,
			NdVector<3,F>: Vector<Component = F>,
	{
		let n_points = values_up_till_now.len();
		
		// Find the the first opposite face.
        // 'diagonal' is the vertex opposite to 'n_points', and 'a' and 'b' are the other points such that 'a<b'.
        let [a,b,diagonal] = faces.iter()
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
                let face = faces.iter()
                    .find(|f| f.contains(&a) && f.contains(&b) && !f.contains(&n_points));
                if let Some(face) = face {
                    let diagonal = *face.iter()
                        .find(|&&v| v != a && v != b)
                        .unwrap();
                    Some([a, b, diagonal])
                }
                else {
                    None
                }
            })
            .unwrap();

		let x_pos = points[n_points];
        println!("x_pos = {:?}", x_pos);

		let a_tex = values_up_till_now[a];
		let b_tex = values_up_till_now[b];
		let diagonal_tex = values_up_till_now[diagonal];
		let a_pos = points[a];
		let b_pos = points[b];
		let diagonal_pos = points[diagonal];

		let u_tex = a_tex - diagonal_tex;
		let v_tex = b_tex - diagonal_tex;

		let u_pos = a_pos - diagonal_pos;
		let v_pos = b_pos - diagonal_pos;

		let delta_pos_projected_on_tp = {
			let delta_pos = x_pos - diagonal_pos;
			let normal = u_pos.cross(v_pos);
			let s = -normal.dot(delta_pos) / normal.dot(normal);
			let out = normal*s + delta_pos;
			debug_assert!(
				out.dot(normal).abs() < F::from_f64(1e-6),
				"delta_pos_projected_on_tp must be on the plane defined by u_pos and v_pos, but it is not. \
				delta_pos_projected_on_tp = {:.5?}, normal = {:.5?}, delta_pos = {:.5?}",
				out, normal, delta_pos
			);
			out
		};

		let u_cross_v = u_pos.cross(v_pos);
		let u_cross_v_norm_squared = u_cross_v.dot(u_cross_v);
		let s = delta_pos_projected_on_tp.cross(v_pos).dot(u_cross_v) / u_cross_v_norm_squared;
		let t = u_pos.cross(delta_pos_projected_on_tp).dot(u_cross_v) / u_cross_v_norm_squared;

		debug_assert!(
			(u_pos*s+v_pos*t - delta_pos_projected_on_tp).norm() < F::from_f64(1e-6),
			"u_pos*s+v_pos*t must equal delta_pos_projected_on_tp, but it is not. \
			u_pos*s+v_pos*t = {:?}, delta_pos_projected_on_tp = {:?}",
			u_pos*s+v_pos*t, delta_pos_projected_on_tp
		);

		// ToDo: The following type conversion if okay but not great.
		let s = Data::Component::from_f64(s.to_f64());
		let t = Data::Component::from_f64(t.to_f64());

		let delta_tex = u_tex * s + v_tex * t;

		diagonal_tex + delta_tex
	}
}

impl<Data> PredictionScheme for DerivativePredictionForTextureCoordinates<Data>
    where 
		Data: Vector,
		Data::Component: DataValue
{
	const ID: u32 = 4;
	
	type Data = Data;

	type AdditionalDataForMetadata = ();
	
	/// Clean the data from previous encoding.
	fn init(&mut self) {

    }
	
	/// Prediction computes the metadata beforehand (unlike transform or portabilization)
	fn compute_metadata(&mut self, _faces: &[[usize; 3]], _additional_data: Self::AdditionalDataForMetadata) {

    }

	fn get_values_impossible_to_predict(&mut self, seq: &mut Vec<std::ops::Range<usize>>, faces: &[[usize; 3]]) 
		-> Vec<std::ops::Range<usize>> 
    {
		let mut is_already_encoded: Vec<bool> = Vec::new();
        let mut vertices_without_parallelogram: Vec<ops::Range<usize>> = Vec::new();

        for face in faces {
            debug_assert!(face.is_sorted());
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
                    .filter(|&&v| v>=is_already_encoded.len() || !is_already_encoded[v])
                    .copied()
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
            for &v in face {
                if v >= is_already_encoded.len() {
                    is_already_encoded.resize(v + 1, false);
                }
                // ToDo: Remove check
                is_already_encoded[v] = true;
            }
        }
        vertices_without_parallelogram.sort_by(|a,b| a.start.cmp(&b.start));
        // merge 'vertices_without_parallelogram' with 'value_indeces'
        let merged = merge_indeces(vec![seq.clone(), vertices_without_parallelogram]);
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
	
	/// predicts the attribute from the given information. 
	/// 'PointData' is a type representing a position i.e. it is an array of f32
	/// or f64 of size (typically) 2 or 3. It has to be generic since the data
	/// is not known at compile time.
	fn predict (
		&self,
		values_up_till_now: &[Self::Data],
		parents: &Vec<&Attribute>,
		faces: &[[usize; 3]]
	) -> Self::Data {
		let n_points = values_up_till_now.len();
		
		assert!(
			parents[0].get_num_components() == 3,
			"Derivative prediction only works for 3D data, but the input data is {}D.",
			parents[0].get_num_components()
		);
		assert!(
			parents[0].get_attribute_type() == AttributeType::Position,
			"Derivative prediction only works for point data, but it is of type {:?}.",
			parents[0].get_attribute_type()
		);
		// Safety: just checked

		let component_type = parents[0].get_component_type();
		unsafe{
			if component_type == ComponentDataType::F32 {
				let points = &parents[0].as_slice_unchecked::<NdVector<3, f32>>()[..=n_points];
				self.predict_impl(values_up_till_now, points, faces)
			} else if component_type == ComponentDataType::F64 {
				let points = &parents[0].as_slice_unchecked::<NdVector<3, f64>>()[..=n_points];
				self.predict_impl(values_up_till_now, points, faces)
			} else {
				panic!("Derivative prediction only works for f32 and f64, but it is {:?}", component_type);
			}
		}
    }
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::shared::NdVector;
	use crate::core::attribute::Attribute;

	#[test]
	fn test_derivative_prediction() {
		let prediction = DerivativePredictionForTextureCoordinates::<NdVector<2,f32>>::new();
		let faces = vec![[0,1,2], [0,1,3]];
		let values_up_till_now = vec![
            NdVector::from([1.0, 0.0]),
			NdVector::from([0.0, 1.0]),
			NdVector::from([0.0, 0.0]),
		];
		let points = vec![
            NdVector::from([1.0, 0.0, 2.0]),
			NdVector::from([0.0, 1.0, 2.0]),
			NdVector::from([0.0, 0.0, 1.0]),
			NdVector::from([2.0, 2.0, 2.0])
		];
		let pts_att = Attribute::from(points, AttributeType::Position);
		let parents = vec![&pts_att];

		let predicted_value = prediction.predict(&values_up_till_now[..], &parents, &faces[..]);
		
		assert_eq!(predicted_value, NdVector::from([1.0, 1.0]));
	}
}