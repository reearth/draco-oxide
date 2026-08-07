use super::PredictionSchemeImpl;
use crate::attribute::Attribute;
use crate::mesh::ds::GenericAttributeDs;
use crate::types::{CornerIdx, NdVector, Vector, VertexIdx};

pub struct DeltaPrediction<'parents, const N: usize, D: GenericAttributeDs> {
    ads: &'parents D,
}

impl<'parents, const N: usize, D: GenericAttributeDs> PredictionSchemeImpl<'parents, N, D>
    for DeltaPrediction<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn new(_parents: &[&'parents Attribute], ads: &'parents D) -> Self {
        Self { ads }
    }

    #[inline]
    fn predict<const ENCODING: bool>(
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
