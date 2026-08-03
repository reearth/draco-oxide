use crate::codec::attribute::geom::{canonicalize_integer_vector, integer_vector_to_oct};
use crate::types::{CornerIdx, Cross, VertexIdx};

use super::PredictionSchemeImpl;
use crate::attribute::Attribute;
use crate::attribute::AttributeType;
use crate::mesh::ds::GenericAttributeDs;
use crate::types::NdVector;
use crate::types::Vector;

pub struct MeshNormalPrediction<'parents, const N: usize, D: GenericAttributeDs> {
    ads: &'parents D,
    pos: &'parents Attribute,
    /// Per-vertex canonicalized prediction, indexed by `VertexIdx`. The sign
    /// flip applies here, before the octahedral projection; the two do not
    /// commute.
    predicted: Vec<NdVector<3, i32>>,
    /// Zero until [`MeshNormalPrediction::set_octahedral_center`], which every
    /// construction must call before predicting.
    center: i32,
    flips: Vec<bool>,
}

/// Accumulates each face's cross-product normal into per-vertex sums.
/// The face normal is the cross product of the two edges leaving the face's
/// first corner; it is computed once per face and added to the sums of all
/// three of the face's vertices. `pos` must hold `NdVector<3, i32>` values.
pub fn accumulate_face_normal_sums<D: GenericAttributeDs>(
    ads: &D,
    pos: &Attribute,
    num_vertices: usize,
) -> Vec<NdVector<3, i64>> {
    let vals = pos.unique_vals_as_slice::<NdVector<3, i32>>();
    let map = pos.point_map_as_slice();
    let pos_of = |c: CornerIdx| -> NdVector<3, i32> {
        let p = usize::from(ads.point_idx(c));
        match map {
            Some(m) => vals[usize::from(m[p])],
            None => vals[p],
        }
    };

    let mut sums = vec![NdVector::<3, i64>::zero(); num_vertices];
    for f in 0..ads.num_faces() {
        let c0 = CornerIdx::from(3 * f);
        let c1 = CornerIdx::from(3 * f + 1);
        let c2 = CornerIdx::from(3 * f + 2);

        let pos_c = pos_of(c0);
        let delta_next = pos_of(c1) - pos_c;
        let delta_prev = pos_of(c2) - pos_c;

        // i64: at high position quantization the products overflow i32.
        let widen = |v: NdVector<3, i32>| {
            NdVector::<3, i64>::from([*v.get(0) as i64, *v.get(1) as i64, *v.get(2) as i64])
        };
        let cross = widen(delta_next).cross(widen(delta_prev));

        sums[usize::from(ads.vertex_idx(c0))] += cross;
        sums[usize::from(ads.vertex_idx(c1))] += cross;
        sums[usize::from(ads.vertex_idx(c2))] += cross;
    }
    sums
}

/// Scales a per-vertex face-normal sum down into i32 and canonicalizes it.
#[inline]
pub fn sum_to_canonical_normal(mut sum: NdVector<3, i64>, center: i32) -> NdVector<3, i32> {
    let upper_bound = 1 << 29;
    let abs_sum = sum.get(0).abs() + sum.get(1).abs() + sum.get(2).abs();
    if abs_sum > upper_bound {
        let quotient = abs_sum / upper_bound;
        sum /= quotient;
    }
    let mut out =
        NdVector::<3, i32>::from([*sum.get(0) as i32, *sum.get(1) as i32, *sum.get(2) as i32]);
    canonicalize_integer_vector(&mut out, center);
    out
}

/// Projects a canonicalized prediction onto the octahedral square, widened to
/// `N` components; only the first two are meaningful.
#[inline]
pub fn canonical_normal_to_oct<const N: usize>(
    vec: NdVector<3, i32>,
    center: i32,
) -> NdVector<N, i32>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    let st = integer_vector_to_oct(vec, center);
    let mut out = NdVector::<N, i32>::zero();
    *out.get_mut(0) = *st.get(0);
    *out.get_mut(1) = *st.get(1);
    out
}

impl<'parents, const N: usize, D: GenericAttributeDs> MeshNormalPrediction<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// Fixes the target lattice and builds the predictions. Must run before any
    /// prediction.
    pub fn set_octahedral_center(&mut self, center: i32) {
        assert!(center > 0, "octahedral center must be positive");
        self.center = center;
        let sums = accumulate_face_normal_sums(self.ads, self.pos, self.ads.vertex_index_bound());
        self.predicted = sums
            .into_iter()
            .map(|s| sum_to_canonical_normal(s, center))
            .collect();
    }

    /// The canonicalized prediction at corner `c`, before the sign flip.
    #[inline]
    pub fn predicted_value(&self, c: CornerIdx) -> NdVector<3, i32> {
        self.predicted[usize::from(self.ads.vertex_idx(c))]
    }

    /// Projects a prediction onto this scheme's lattice.
    #[inline]
    pub fn project(&self, vec: NdVector<3, i32>) -> NdVector<N, i32> {
        canonical_normal_to_oct(vec, self.center)
    }
}

impl<'parents, const N: usize, D: GenericAttributeDs> PredictionSchemeImpl<'parents, N, D>
    for MeshNormalPrediction<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    const ID: u32 = 2;

    type AdditionalDataForMetadata = ();

    fn new(parents: &[&'parents Attribute], ads: &'parents D) -> Self {
        assert!(parents.len() == 1, "MeshNormalPrediction requires exactly one parent attribute for position. but it has {} parents.", parents.len());
        assert!(
            parents[0].get_attribute_type() == AttributeType::Position,
            "MeshNormalPrediction requires the first parent attribute to be of type Position."
        );
        let pos = parents[0]; // we made sure that the first parent is the position attribute

        Self {
            ads,
            pos,
            predicted: Vec::new(),
            center: 0,
            flips: Vec::new(),
        }
    }

    fn get_values_impossible_to_predict(
        &mut self,
        _seq: &mut Vec<std::ops::Range<usize>>,
    ) -> Vec<std::ops::Range<usize>> {
        unimplemented!();
    }

    fn predict(
        &mut self,
        c: CornerIdx,
        _vertices_up_till_now: &[VertexIdx],
        attribute: &Attribute,
    ) -> NdVector<N, i32> {
        let v = self.ads.vertex_idx(c);
        let pred_3d = self.predicted[usize::from(v)];

        // The closer direction wins, measured the way the correction is: the
        // octahedral square wraps, so distances are taken modulo its edge.
        let pos = self.project(pred_3d);
        let neg = self.project(pred_3d * -1);

        let actual_val = attribute.get::<NdVector<N, i32>, N>(self.ads.point_idx(c));
        let cost = |cand: NdVector<N, i32>| -> i32 {
            let max_quantized = 2 * self.center + 1;
            (0..2)
                .map(|i| {
                    let d = *actual_val.get(i) - *cand.get(i);
                    let d = if d > self.center {
                        d - max_quantized
                    } else if d < -self.center {
                        d + max_quantized
                    } else {
                        d
                    };
                    d.abs()
                })
                .sum()
        };
        if cost(pos) > cost(neg) {
            self.flips.push(true);
            neg
        } else {
            self.flips.push(false);
            pos
        }
    }

    fn encode_prediction_metadtata<W>(&self, writer: &mut W) -> Result<(), super::Err>
    where
        W: crate::bit_coder::ByteWriter,
    {
        encode_flip_metadata(&self.flips, writer)
    }
}

/// Encodes the per-value sign-flip bits of mesh-normal prediction into `writer`,
/// using the exact rABS layout the decoder expects: a `zero_prob` byte, then the
/// leb128 length of the coded buffer, then the buffer itself.
///
/// This is factored out of [`MeshNormalPrediction::encode_prediction_metadtata`]
/// so the zero-CPU "trust prediction" encode path can emit neutral (all-false)
/// flips for `count` values without constructing the predictor at all — an
/// all-false slice is a valid input and reproduces the byte layout of a run in
/// which every predicted normal was kept as-is.
pub fn encode_flip_metadata<W>(flips: &[bool], writer: &mut W) -> Result<(), super::Err>
where
    W: crate::bit_coder::ByteWriter,
{
    super::encode_rabs_bit_stream(flips, writer)
}
