use crate::core::shared::{CornerIdx, Cross, Dot};
use crate::encode::attribute::prediction_transform::geom::{into_faithful_oct_quantization, octahedral_transform};
use crate::encode::entropy::rans::RabsCoder;
use crate::utils::bit_coder::leb128_write;

use super::PredictionSchemeImpl;
use crate::core::corner_table::GenericCornerTable;
use crate::core::{attribute::Attribute, shared::Vector};
use crate::prelude::{AttributeType, NdVector};

pub(crate) struct MeshNormalPrediction<'parents, C, const N: usize> {
    corner_table: &'parents C,
    pos: &'parents Attribute,
    flips: Vec<bool>,
}

impl<'parents, C, const N: usize> MeshNormalPrediction<'parents, C, N> 
    where 
        C: GenericCornerTable,
        NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn compute_normal_of_face(&self, c: CornerIdx, pos_c: NdVector<3, i32>) -> NdVector<3, i64> {
        // corners.
        let c_next = self.corner_table.next(c);
        let c_prev = self.corner_table.previous(c);

        let pos_next = self.pos.get::<NdVector<3,i32>, 3>(self.corner_table.vertex_idx(c_next));
        let pos_prev = self.pos.get::<NdVector<3,i32>, 3>(self.corner_table.vertex_idx(c_prev));

        // Compute the difference to next and prev.
        let delta_next = pos_next - pos_c;
        let delta_prev = pos_prev - pos_c;

        // Take the cross product
        let cross = {
            let cross_i32 = delta_next.cross(delta_prev);
            let mut cross = NdVector::<3, i64>::zero();
            *cross.get_mut(0) = *cross_i32.get(0) as i64;
            *cross.get_mut(1) = *cross_i32.get(1) as i64;
            *cross.get_mut(2) = *cross_i32.get(2) as i64;
            cross
        };
        cross
    }
}

impl<'parents, C, const N: usize> PredictionSchemeImpl<'parents, C, N> for MeshNormalPrediction<'parents, C, N> 
    where 
        C: GenericCornerTable,
        NdVector<N, i32>: Vector<N, Component = i32>,
{
    const ID: u32 = 2;
    
    type AdditionalDataForMetadata = ();
	
	fn new(parents: &[&'parents Attribute], corner_table: &'parents C ) -> Self {
        assert!(parents.len() == 1, "MeshNormalPrediction requires exactly one parent attribute for position. but it has {} parents.", parents.len());
        assert!(
            parents[0].get_attribute_type() == AttributeType::Position, 
            "MeshNormalPrediction requires the first parent attribute to be of type Position."
        );
        Self {
            corner_table,
            pos: parents[0], // we made sure that the first parent is the position attribute
            flips: Vec::new()
        }
    }

	fn get_values_impossible_to_predict(&mut self, _seq: &mut Vec<std::ops::Range<usize>>) 
        -> Vec<std::ops::Range<usize>>
    {
        unimplemented!();
    }
	
	fn predict(
		&mut self,
        c: usize,
		_vertices_up_till_now: &[usize],
        attribute: &Attribute,
	) -> NdVector<N, i32> {
        let pos_c = self.pos.get(self.corner_table.vertex_idx(c));
        let mut sum = self.compute_normal_of_face(c, pos_c);
        let mut curr_c = c;
        while let Some(next_c) = self.corner_table.swing_right(curr_c) {
            curr_c = next_c;
            if curr_c == c {
                break;
            }
            sum += self.compute_normal_of_face(curr_c, pos_c);
        }

        // Cast down to i32. The following upper bound is from the draco library.
        let upper_bound = 1 << 29;
        let abs_sum = sum.get(0).abs() + sum.get(1).abs() + sum.get(2).abs();
        if abs_sum > upper_bound {
            let quotient = abs_sum / upper_bound;
            sum /= quotient;
        }
        let mut out = {
            let mut out = NdVector::<3, i32>::zero();
            *out.get_mut(0) = *sum.get(0) as i32;
            *out.get_mut(1) = *sum.get(1) as i32;
            *out.get_mut(2) = *sum.get(2) as i32;
            
            // Check if the normal is zero and handle gracefully
            if out == NdVector::<3, i32>::zero() {
                // Return a default normal pointing up (0, 0, 1) in octahedral space
                let mut default_out = NdVector::<N, i32>::zero();
                *default_out.get_mut(0) = 0;
                *default_out.get_mut(1) = 0;
                default_out
            } else {
                let val_oct = octahedral_transform(out) + NdVector::<2, f32>::from([1.0,1.0]);
                let quantized = val_oct * ((1<<8-1)-1) as f32; // TODO: Stop hardcoding the quantization bits.
                let mut out = NdVector::<2, i32>::zero();
                for i in 0..2 {
                    *out.get_mut(i) = *quantized.get(i) as i32;
                }
                let quant_out = into_faithful_oct_quantization(out);
                let mut out = NdVector::<N, i32>::zero();
                *out.get_mut(0) = *quant_out.get(0);
                *out.get_mut(1) = *quant_out.get(1);
                out
            }
        };
        let actual_val = attribute.get::<NdVector<N,i32>,N>(self.corner_table.pos_vertex_idx(c));
        let diff1 = out - actual_val;
        let diff2 = out * -1 - actual_val;
        if diff1.dot(diff1) > diff2.dot(diff2) {
            // if -out is closer to the actual value, we flip the sign.
            self.flips.push(true);
            out = out * -1;
        } else {
            self.flips.push(false);
        }
        out
    }


    fn encode_prediction_metadtata<W>(&self, writer: &mut W) -> Result<(), super::Err>
        where W: crate::prelude::ByteWriter 
    {
        let freq_count_0 = self.flips.iter().filter(|&&o| !o).count();
        let zero_prob = (((freq_count_0 as f32 / self.flips.len() as f32) * 256.0 + 0.5) as u16).clamp(1,255) as u8;
        let mut rabs_coder: RabsCoder<> = RabsCoder::new(zero_prob as usize, None);
        writer.write_u8(zero_prob);
        for &b in &self.flips {
            // Encode each flip as a single bit
            rabs_coder.write(if b { 1 } else { 0 })?;
        }
        let buffer = rabs_coder.flush()?;
        leb128_write(buffer.len() as u64, writer);
        for byte in buffer {
            writer.write_u8(byte);
        }
        Ok(())
    }
}