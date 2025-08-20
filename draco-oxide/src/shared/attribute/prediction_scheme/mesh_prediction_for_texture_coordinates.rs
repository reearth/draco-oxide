use super::PredictionSchemeImpl;
use crate::core::corner_table::GenericCornerTable;
use crate::core::shared::{CornerIdx, PointIdx, VertexIdx};
use crate::core::{attribute::Attribute, shared::{Vector, Dot}};
use crate::encode::entropy::rans::RabsCoder;
use crate::prelude::{ByteWriter, NdVector};
use crate::utils::bit_coder::leb128_write;

pub(crate) struct MeshPredictionForTextureCoordinates<'parents, C, const N: usize> {
    corner_table: &'parents C,
    pos_att: &'parents Attribute,
    orientation: Vec<bool>, // Stores orientation for encoder
}

impl<'parents, C, const N: usize> MeshPredictionForTextureCoordinates<'parents, C, N>
where
    C: GenericCornerTable,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// Get 3D position for a vertex from the position attribute
    fn get_position_for_vertex(&self, point_idx: PointIdx) -> NdVector<3, i32> {
        // Get position data as a slice of 3D vectors
        if usize::from(point_idx) < self.pos_att.len() {
            // Use the generic get method to retrieve the position vector
            self.pos_att.get::<NdVector<3, i32>, 3>(point_idx)
        } else {
            NdVector::<3, i32>::zero()
        }
    }

    /// Integer square root
    fn int_sqrt(&self, value: u64) -> u64 {
        if value == 0 {
            return 0;
        }
        let mut act_number = value;
        let mut sqrt = 1;
        while act_number >= 2 {
            sqrt *= 2;
            act_number /= 4;
        }

        sqrt = (sqrt + value / sqrt) / 2;        
        while sqrt * sqrt > value {
            sqrt = (sqrt + value / sqrt) / 2;
        }
        sqrt
    }

    /// Fallback prediction when complex prediction is not possible
    fn fallback_predict(
        &self,
        c: CornerIdx,
        vertices_up_till_now: &[VertexIdx],
        attribute: &Attribute,
    ) -> NdVector<N, i32> {
        // Check if next vertex has been processed  
        let next_corner = self.corner_table.next(c);
        let next_vertex = self.corner_table.vertex_idx(next_corner);
        if vertices_up_till_now.contains(&next_vertex) {
            return attribute.get(self.corner_table.point_idx(next_corner));
        }
        
        // The following chunk of code is supposed to be there, but it is commented out
        // as draco contains a bug that avoids using the previous vertex for prediction.
        
        // // Check if previous vertex has been processed
        // let prev_corner = self.corner_table.previous(i);
        // let prev_vertex = self.corner_table.vertex_idx(prev_corner);
        // if vertices_up_till_now.contains(&prev_vertex) {
        //     return attribute.get(prev_vertex);
        // }
        
        // Use the most recently processed vertex
        if let Some(&last_vertex) = vertices_up_till_now.last() {
            return attribute.get(self.corner_table.point_idx(self.corner_table.left_most_corner(last_vertex)));
        }
        
        // If none applies, then this is the first prediction. return zero
        NdVector::<N, i32>::zero()
    }
}

impl<'parents, C, const N: usize> PredictionSchemeImpl<'parents, C, N> for MeshPredictionForTextureCoordinates<'parents, C, N> 
    where 
        C: GenericCornerTable,
        NdVector<N, i32>: Vector<N, Component = i32>,
{
    const ID: u32 = 2;
    
    type AdditionalDataForMetadata = ();
	
	fn new(parents: &[&'parents Attribute], corner_table: &'parents C ) -> Self {
        Self {
            corner_table,
            pos_att: parents[0],
            orientation: Vec::new(), // Initialize orientation vector
        }
    }

	fn get_values_impossible_to_predict(&mut self, _seq: &mut Vec<std::ops::Range<usize>>) 
        -> Vec<std::ops::Range<usize>>
    {
        unimplemented!();
    }
	
	fn predict(
		&mut self,
        i: CornerIdx,
		vertices_up_till_now: &[VertexIdx],
        attribute: &Attribute,
	) -> NdVector<N, i32> {
        // This prediction scheme is specifically for texture coordinates (2D)
        debug_assert_eq!(N, 2, "Texture coordinate prediction is only for 2D vectors");
        
        // Get next and previous corners for the current corner
        let next_corner = self.corner_table.next(i);
        let prev_corner = self.corner_table.previous(i);
        
        // Get vertex indices from corners
        let next_pt = self.corner_table.point_idx(next_corner);
        let prev_pt = self.corner_table.point_idx(prev_corner);
        let curr_pt = self.corner_table.point_idx(i);

        let next_vertex = self.corner_table.vertex_idx(next_corner);
        let prev_vertex = self.corner_table.vertex_idx(prev_corner);

        // Check if both neighboring vertices have already been processed
        if vertices_up_till_now.contains(&next_vertex) && vertices_up_till_now.contains(&prev_vertex) {
            // Get texture coordinates for next and previous vertices
            let curr_uv: NdVector<N, i32> = attribute.get(curr_pt);
            let curr_uv = NdVector::<2, i64>::from([*curr_uv.get(0) as i64, *curr_uv.get(1) as i64]);
            let next_uv: NdVector<N, i32> = attribute.get(next_pt);
            let next_uv = NdVector::<2, i64>::from([*next_uv.get(0) as i64, *next_uv.get(1) as i64]);
            let prev_uv: NdVector<N, i32> = attribute.get(prev_pt);
            let prev_uv = NdVector::<2, i64>::from([*prev_uv.get(0) as i64, *prev_uv.get(1) as i64]);
            // If the UV coordinates are identical, return one of them (degenerate case)
            if next_uv == prev_uv {
                let prev_uv = attribute.get(prev_pt);
                return prev_uv;
            }

            // Get 3D positions for all three vertices
            let curr_pos = self.get_position_for_vertex(curr_pt);
            let curr_pos = NdVector::<3, i64>::from([*curr_pos.get(0) as i64, *curr_pos.get(1) as i64, *curr_pos.get(2) as i64]);
            let next_pos = self.get_position_for_vertex(next_pt);
            let next_pos = NdVector::<3, i64>::from([*next_pos.get(0) as i64, *next_pos.get(1) as i64, *next_pos.get(2) as i64]);
            let prev_pos = self.get_position_for_vertex(prev_pt);
            let prev_pos = NdVector::<3, i64>::from([*prev_pos.get(0) as i64, *prev_pos.get(1) as i64, *prev_pos.get(2) as i64]);
            
            // Calculate vectors
            let pn = prev_pos - next_pos;  // prev_pos - next_pos
            let pn = NdVector::<3, i64>::from([*pn.get(0) as i64, *pn.get(1) as i64, *pn.get(2) as i64]);
            let pn_norm2_squared = pn.dot(pn) as u64;
            
            if pn_norm2_squared != 0 {
                let cn = curr_pos - next_pos;  // curr_pos - next_pos  
                let cn = NdVector::<3, i64>::from([*cn.get(0) as i64, *cn.get(1) as i64, *cn.get(2) as i64]);
                let cn_dot_pn = pn.dot(cn) as i64;
                
                let pn_uv = prev_uv - next_uv;
                
                // Check for potential overflow
                let n_uv_absmax = next_uv.get(0).abs().max(next_uv.get(1).abs()) as i64;
                if n_uv_absmax > i64::MAX / pn_norm2_squared as i64 {
                    // Overflow would occur, fallback to simple prediction
                    return self.fallback_predict(i, vertices_up_till_now, attribute);
                }
                
                let pn_uv_absmax = pn_uv.get(0).abs().max(pn_uv.get(1).abs()) as i64;
                if cn_dot_pn.abs() as i64 > i64::MAX / pn_uv_absmax {
                    // Overflow would occur, fallback to simple prediction
                    return self.fallback_predict(i, vertices_up_till_now, attribute);
                }
                
                // Calculate x_uv = next_uv * pn_norm2_squared + cn_dot_pn * pn_uv
                let x_uv = next_uv * pn_norm2_squared as i64 + pn_uv * cn_dot_pn;
                
                // Check for overflow in position calculation
                let pn_absmax = pn.get(0).abs().max(pn.get(1).abs()).max(pn.get(2).abs()) as i64;
                if cn_dot_pn.abs() > i64::MAX / pn_absmax {
                    // Overflow would occur, fallback to simple prediction
                    return self.fallback_predict(i, vertices_up_till_now, attribute);
                }
                
                // Calculate x_pos = next_pos + (cn_dot_pn * pn) / pn_norm2_squared
                let x_pos = next_pos + pn * cn_dot_pn / pn_norm2_squared as i64;
                let cx_norm2_squared = (curr_pos - x_pos).dot(curr_pos - x_pos) as u64;
                
                // Calculate cx_uv by rotating pn_uv by 90 degrees
                let mut cx_uv = NdVector::<2, i64>::from([*pn_uv.get(1), -pn_uv.get(0)]);
                
                // Scale by sqrt(cx_norm2_squared * pn_norm2_squared)
                let norm_squared = self.int_sqrt(cx_norm2_squared * pn_norm2_squared);
                cx_uv *= norm_squared as i64;

                // Try both orientations and choose the better one (encoder mode)
                let predicted_uv_0 = (x_uv + cx_uv) / (pn_norm2_squared as i64);
                let predicted_uv_1 = (x_uv - cx_uv) / (pn_norm2_squared as i64);
                
                // In encoder mode, we would choose the orientation that gives better prediction
                let predicted_uv = if (curr_uv-predicted_uv_0).dot(curr_uv-predicted_uv_0) < (curr_uv-predicted_uv_1).dot(curr_uv-predicted_uv_1) {
                    self.orientation.push(true);
                    predicted_uv_0
                } else {
                    self.orientation.push(false);
                    predicted_uv_1
                };

                let mut out = NdVector::<N, i32>::zero();
                *out.get_mut(0 ) = *predicted_uv.get(0) as i32;
                *out.get_mut(1) = *predicted_uv.get(1) as i32;
                return out;
            }
        }
        
        // Fallback to simple prediction if complex prediction is not possible
        self.fallback_predict(i, vertices_up_till_now, attribute)
    }

    fn encode_prediction_metadtata<W>(&self, writer: &mut W) -> Result<(), super::Err> 
        where W: ByteWriter 
    {
        let freq_count_0 = {
            let mut last = true;
            let mut compare = |o| {
                if o == last {
                    true
                } else {
                    last = o;
                    false
                }
            };
            self.orientation.iter().map(|&o| compare(o) ).filter(|&o| !o).count()
        };
        let zero_prob = (((freq_count_0 as f32 / self.orientation.len() as f32) * 256.0 + 0.5) as u16).clamp(1,255) as u8;
        let mut rabs_coder: RabsCoder<> = RabsCoder::new(zero_prob as usize, None);
        writer.write_u32(self.orientation.len() as u32);
        writer.write_u8(zero_prob);
        let mut last_orientation = true;
        let out = self.orientation.iter().rev().map(|&o| 
            // Encode orientation as a single bit
            if o == last_orientation {
                1
            } else {
                last_orientation = o;
                0
            }
        ).collect::<Vec<_>>();
        for bit in out.into_iter().rev() {
            rabs_coder.write(bit)?;
        }
        let buffer = rabs_coder.flush()?;
        leb128_write(buffer.len() as u64, writer);
        for byte in buffer {
            writer.write_u8(byte);
        }
        Ok(())
    }
}
