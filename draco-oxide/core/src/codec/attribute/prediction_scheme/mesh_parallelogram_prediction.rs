use super::PredictionSchemeImpl;
use crate::attribute::Attribute;
use crate::mesh::ds::{GenericAttributeDs, GenericCornerTable};
use crate::types::NdVector;
use crate::types::Vector;
use crate::types::{CornerIdx, VertexIdx};

pub struct MeshParallelogramPrediction<'parents, const N: usize, D: GenericAttributeDs> {
    ads: &'parents D,
    /// O(1) visited membership; `synced` tracks the folded prefix of the
    /// running sequence.
    visited: Vec<bool>,
    synced: usize,
}

impl<'parents, const N: usize, D: GenericAttributeDs> PredictionSchemeImpl<'parents, N, D>
    for MeshParallelogramPrediction<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn new(_parents: &[&'parents Attribute], ads: &'parents D) -> Self {
        Self {
            visited: vec![false; ads.vertex_index_bound()],
            synced: 0,
            ads,
        }
    }

    #[inline]
    fn predict<const ENCODING: bool>(
        &mut self,
        c: CornerIdx,
        vertices_up_till_now: &[VertexIdx],
        attribute: &Attribute,
    ) -> NdVector<N, i32> {
        for &v in &vertices_up_till_now[self.synced..] {
            self.visited[usize::from(v)] = true;
        }
        self.synced = vertices_up_till_now.len();

        let map = attribute.point_map_as_slice();
        let vals = attribute.unique_vals_as_slice::<NdVector<N, i32>>();
        let read = |p: crate::types::PointIdx| -> NdVector<N, i32> {
            match map {
                Some(m) => vals[usize::from(m[usize::from(p)])],
                None => vals[usize::from(p)],
            }
        };

        let [a, b, diagonal] = {
            if let Some(opp) = self.ads.corner_table().opposite(c) {
                let opp_v = self.ads.vertex_idx(opp);
                let next_v = self.ads.vertex_idx(c.next());
                let prev_v = self.ads.vertex_idx(c.previous());
                if self.visited[usize::from(opp_v)]
                    && self.visited[usize::from(next_v)]
                    && self.visited[usize::from(prev_v)]
                {
                    [c.next(), c.previous(), opp]
                } else {
                    return if let Some(&last_v) = vertices_up_till_now.last() {
                        read(self.ads.point_idx(self.ads.left_most_corner(last_v)))
                    } else {
                        NdVector::<N, i32>::zero()
                    };
                }
            } else {
                return if let Some(&last_v) = vertices_up_till_now.last() {
                    read(self.ads.point_idx(self.ads.left_most_corner(last_v)))
                } else {
                    NdVector::<N, i32>::zero()
                };
            }
        };

        let a_coord = read(self.ads.point_idx(a));
        let b_coord = read(self.ads.point_idx(b));
        let diagonal_coord = read(self.ads.point_idx(diagonal));
        a_coord + b_coord - diagonal_coord
    }
}
