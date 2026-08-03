use super::PredictionSchemeImpl;
use crate::attribute::Attribute;
use crate::mesh::ds::GenericAttributeDs;
use crate::types::{CornerIdx, NdVector, Vector, VertexIdx};
use std::mem;

pub struct DeltaPrediction<'parents, const N: usize, D: GenericAttributeDs> {
    faces: &'parents [[usize; 3]],
    ads: &'parents D,
}

impl<'parents, const N: usize, D: GenericAttributeDs> PredictionSchemeImpl<'parents, N, D>
    for DeltaPrediction<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    const ID: u32 = 1;

    type AdditionalDataForMetadata = ();

    fn new(_parents: &[&'parents Attribute], ads: &'parents D) -> Self {
        let faces: &[[usize; 3]] = &[];

        Self { faces, ads }
    }

    fn get_values_impossible_to_predict(
        &mut self,
        value_indices: &mut Vec<std::ops::Range<usize>>,
    ) -> Vec<std::ops::Range<usize>> {
        let mut self_ = Vec::new();
        let mut out = Vec::new();
        for r in value_indices.iter() {
            for i in r.clone() {
                if i == 0 {
                    out.push(i);
                } else if self
                    .faces
                    .iter()
                    .any(|f| f.contains(&(i - 1)) && f.contains(&i))
                {
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

    #[inline]
    fn predict(
        &mut self,
        _i: CornerIdx,
        vertices_up_till_now: &[VertexIdx],
        att: &Attribute,
    ) -> NdVector<N, i32> {
        let prev_v = if let Some(prev_v) = vertices_up_till_now.last() {
            *prev_v
        } else {
            return NdVector::zero();
        };
        let prev_pt = self.ads.point_idx(self.ads.left_most_corner(prev_v));
        let vals = att.unique_vals_as_slice::<NdVector<N, i32>>();
        match att.point_map_as_slice() {
            Some(m) => vals[usize::from(m[usize::from(prev_pt)])],
            None => vals[usize::from(prev_pt)],
        }
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
