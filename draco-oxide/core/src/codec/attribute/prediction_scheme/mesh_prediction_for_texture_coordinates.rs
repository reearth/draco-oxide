use super::PredictionSchemeImpl;
use crate::attribute::Attribute;
use crate::mesh::ds::GenericAttributeDs;
use crate::safety_assert_eq;
use crate::types::NdVector;
use crate::types::{AttributeValueIdx, CornerIdx, PointIdx, VertexIdx};
use crate::types::{Dot, Vector};

/// Either the two orientation candidates or a directly determined value
/// (degenerate and fallback cases, which consume no orientation bit).
pub enum TexCoordsPrediction<const N: usize>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    Oriented(NdVector<2, i64>, NdVector<2, i64>),
    Single(NdVector<N, i32>),
}

/// A point's value through an optional point-to-value map.
#[inline]
fn read_point<const N: usize>(
    map: Option<&[AttributeValueIdx]>,
    vals: &[NdVector<N, i32>],
    p: PointIdx,
) -> NdVector<N, i32>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    match map {
        Some(m) => vals[usize::from(m[usize::from(p)])],
        None => vals[usize::from(p)],
    }
}

pub struct MeshPredictionForTextureCoordinates<'parents, const N: usize, D: GenericAttributeDs> {
    ads: &'parents D,
    pos_att: &'parents Attribute,
    /// One orientation bit per oriented prediction. `predict::<true>` appends in
    /// traversal order; `predict::<false>` pops from the back, since the encoder
    /// writes the stream reversed.
    orientation: Vec<bool>,
    visited: Vec<bool>,
    synced: usize,
}

impl<'parents, const N: usize, D: GenericAttributeDs>
    MeshPredictionForTextureCoordinates<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// A vertex's 3D position; out-of-range indices read as zero.
    #[inline]
    fn get_position_for_vertex(
        pos_map: Option<&[AttributeValueIdx]>,
        pos_vals: &[NdVector<3, i32>],
        point_idx: PointIdx,
    ) -> NdVector<3, i32> {
        let idx = match pos_map {
            Some(m) => m.get(usize::from(point_idx)).map(|&i| usize::from(i)),
            None => Some(usize::from(point_idx)),
        };
        idx.and_then(|i| pos_vals.get(i))
            .copied()
            .unwrap_or_else(NdVector::<3, i32>::zero)
    }

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

    /// Fallback when the projected prediction is not possible. The
    /// previous-vertex fallback is absent to match a reference draco bug.
    fn fallback_predict(
        &self,
        c: CornerIdx,
        vertices_up_till_now: &[VertexIdx],
        att_map: Option<&[AttributeValueIdx]>,
        att_vals: &[NdVector<N, i32>],
    ) -> NdVector<N, i32> {
        let next_corner = c.next();
        let next_vertex = self.ads.vertex_idx(next_corner);
        if self.visited[usize::from(next_vertex)] {
            return read_point(att_map, att_vals, self.ads.point_idx(next_corner));
        }

        if let Some(&last_vertex) = vertices_up_till_now.last() {
            return read_point(
                att_map,
                att_vals,
                self.ads.point_idx(self.ads.left_most_corner(last_vertex)),
            );
        }

        NdVector::<N, i32>::zero()
    }

    /// Folds vertices appended since the last call into the visited set.
    fn sync_visited(&mut self, vertices_up_till_now: &[VertexIdx]) {
        for &v in &vertices_up_till_now[self.synced..] {
            self.visited[usize::from(v)] = true;
        }
        self.synced = vertices_up_till_now.len();
    }

    /// The prediction shared by encoder and decoder; reads only
    /// already-processed values.
    fn compute_prediction(
        &self,
        i: CornerIdx,
        vertices_up_till_now: &[VertexIdx],
        attribute: &Attribute,
    ) -> TexCoordsPrediction<N> {
        safety_assert_eq!(N, 2, "Texture coordinate prediction is only for 2D vectors");

        let att_map = attribute.point_map_as_slice();
        let att_vals = attribute.unique_vals_as_slice::<NdVector<N, i32>>();
        let pos_map = self.pos_att.point_map_as_slice();
        let pos_vals = self.pos_att.unique_vals_as_slice::<NdVector<3, i32>>();

        let next_corner = i.next();
        let prev_corner = i.previous();

        let next_pt = self.ads.point_idx(next_corner);
        let prev_pt = self.ads.point_idx(prev_corner);
        let curr_pt = self.ads.point_idx(i);

        let next_vertex = self.ads.vertex_idx(next_corner);
        let prev_vertex = self.ads.vertex_idx(prev_corner);

        if self.visited[usize::from(next_vertex)] && self.visited[usize::from(prev_vertex)] {
            let next_uv: NdVector<N, i32> = read_point(att_map, att_vals, next_pt);
            let next_uv =
                NdVector::<2, i64>::from([*next_uv.get(0) as i64, *next_uv.get(1) as i64]);
            let prev_uv: NdVector<N, i32> = read_point(att_map, att_vals, prev_pt);
            let prev_uv =
                NdVector::<2, i64>::from([*prev_uv.get(0) as i64, *prev_uv.get(1) as i64]);
            if next_uv == prev_uv {
                return TexCoordsPrediction::Single(read_point(att_map, att_vals, prev_pt));
            }

            let curr_pos = Self::get_position_for_vertex(pos_map, pos_vals, curr_pt);
            let curr_pos = NdVector::<3, i64>::from([
                *curr_pos.get(0) as i64,
                *curr_pos.get(1) as i64,
                *curr_pos.get(2) as i64,
            ]);
            let next_pos = Self::get_position_for_vertex(pos_map, pos_vals, next_pt);
            let next_pos = NdVector::<3, i64>::from([
                *next_pos.get(0) as i64,
                *next_pos.get(1) as i64,
                *next_pos.get(2) as i64,
            ]);
            let prev_pos = Self::get_position_for_vertex(pos_map, pos_vals, prev_pt);
            let prev_pos = NdVector::<3, i64>::from([
                *prev_pos.get(0) as i64,
                *prev_pos.get(1) as i64,
                *prev_pos.get(2) as i64,
            ]);

            let pn = prev_pos - next_pos; // prev_pos - next_pos
            let pn = NdVector::<3, i64>::from([*pn.get(0), *pn.get(1), *pn.get(2)]);
            let pn_norm2_squared = pn.dot(pn) as u64;

            if pn_norm2_squared != 0 {
                let cn = curr_pos - next_pos; // curr_pos - next_pos
                let cn = NdVector::<3, i64>::from([*cn.get(0), *cn.get(1), *cn.get(2)]);
                let cn_dot_pn = pn.dot(cn);

                let pn_uv = prev_uv - next_uv;

                let n_uv_absmax = next_uv.get(0).abs().max(next_uv.get(1).abs());
                if n_uv_absmax > i64::MAX / pn_norm2_squared as i64 {
                    return TexCoordsPrediction::Single(self.fallback_predict(
                        i,
                        vertices_up_till_now,
                        att_map,
                        att_vals,
                    ));
                }

                let pn_uv_absmax = pn_uv.get(0).abs().max(pn_uv.get(1).abs());
                if cn_dot_pn.abs() > i64::MAX / pn_uv_absmax {
                    return TexCoordsPrediction::Single(self.fallback_predict(
                        i,
                        vertices_up_till_now,
                        att_map,
                        att_vals,
                    ));
                }

                let x_uv = next_uv * pn_norm2_squared as i64 + pn_uv * cn_dot_pn;

                let pn_absmax = pn.get(0).abs().max(pn.get(1).abs()).max(pn.get(2).abs());
                if cn_dot_pn.abs() > i64::MAX / pn_absmax {
                    return TexCoordsPrediction::Single(self.fallback_predict(
                        i,
                        vertices_up_till_now,
                        att_map,
                        att_vals,
                    ));
                }

                let x_pos = next_pos + pn * cn_dot_pn / pn_norm2_squared as i64;
                let cx_norm2_squared = (curr_pos - x_pos).dot(curr_pos - x_pos) as u64;

                let mut cx_uv = NdVector::<2, i64>::from([*pn_uv.get(1), -pn_uv.get(0)]);

                let norm_squared = self.int_sqrt(cx_norm2_squared * pn_norm2_squared);
                cx_uv *= norm_squared as i64;

                let predicted_uv_0 = (x_uv + cx_uv) / (pn_norm2_squared as i64);
                let predicted_uv_1 = (x_uv - cx_uv) / (pn_norm2_squared as i64);
                return TexCoordsPrediction::Oriented(predicted_uv_0, predicted_uv_1);
            }
        }

        TexCoordsPrediction::Single(self.fallback_predict(
            i,
            vertices_up_till_now,
            att_map,
            att_vals,
        ))
    }

    /// The orientation bits recorded while encoding.
    pub fn orientation(&self) -> &[bool] {
        &self.orientation
    }

    /// Installs the orientation bits decoded from the stream, which
    /// `predict::<false>` consumes from the back.
    pub fn set_orientation(&mut self, orientation: Vec<bool>) {
        self.orientation = orientation;
    }
}

impl<'parents, const N: usize, D: GenericAttributeDs> PredictionSchemeImpl<'parents, N, D>
    for MeshPredictionForTextureCoordinates<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn new(parents: &[&'parents Attribute], ads: &'parents D) -> Self {
        Self {
            ads,
            pos_att: parents[0],
            orientation: Vec::new(), // Initialize orientation vector
            visited: vec![false; ads.vertex_index_bound()],
            synced: 0,
        }
    }

    #[inline]
    fn predict<const ENCODING: bool>(
        &mut self,
        i: CornerIdx,
        vertices_up_till_now: &[VertexIdx],
        attribute: &Attribute,
    ) -> NdVector<N, i32> {
        // Folds at monomorphization; other component counts carry no body.
        assert!(
            N == 2,
            "texture coordinate prediction is only for 2D vectors"
        );
        self.sync_visited(vertices_up_till_now);
        match self.compute_prediction(i, vertices_up_till_now, attribute) {
            TexCoordsPrediction::Single(p) => p,
            TexCoordsPrediction::Oriented(predicted_uv_0, predicted_uv_1) => {
                if !ENCODING {
                    let chosen = if self.orientation.pop().unwrap_or(true) {
                        predicted_uv_0
                    } else {
                        predicted_uv_1
                    };
                    let mut out = NdVector::<N, i32>::zero();
                    *out.get_mut(0) = *chosen.get(0) as i32;
                    *out.get_mut(1) = *chosen.get(1) as i32;
                    return out;
                }
                let curr_pt = self.ads.point_idx(i);
                let curr_uv: NdVector<N, i32> = read_point(
                    attribute.point_map_as_slice(),
                    attribute.unique_vals_as_slice::<NdVector<N, i32>>(),
                    curr_pt,
                );
                let curr_uv =
                    NdVector::<2, i64>::from([*curr_uv.get(0) as i64, *curr_uv.get(1) as i64]);
                let predicted_uv = if (curr_uv - predicted_uv_0).dot(curr_uv - predicted_uv_0)
                    < (curr_uv - predicted_uv_1).dot(curr_uv - predicted_uv_1)
                {
                    self.orientation.push(true);
                    predicted_uv_0
                } else {
                    self.orientation.push(false);
                    predicted_uv_1
                };

                let mut out = NdVector::<N, i32>::zero();
                *out.get_mut(0) = *predicted_uv.get(0) as i32;
                *out.get_mut(1) = *predicted_uv.get(1) as i32;
                out
            }
        }
    }
}
