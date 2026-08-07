//! Constrained multi-parallelogram prediction (reference scheme id 4).
//!
//! The value at a vertex is predicted as the average of a chosen subset of the
//! parallelogram predictions available around it, up to
//! [`MAX_PARALLELOGRAMS`]. The encoder selects the subset per vertex and
//! records the choice as one crease bit per available parallelogram (1 marks
//! the parallelogram unused); the decoder replays those bits. Bits are grouped
//! into one stream per context, where the context is the number of available
//! parallelograms minus one. When no parallelogram is used, the previously
//! processed value serves as the prediction.

use super::PredictionSchemeImpl;
use crate::attribute::Attribute;
use crate::codec::entropy::shannon::{binary_entropy, signed_to_symbol, ShannonEntropyTracker};
use crate::mesh::ds::{GenericAttributeDs, GenericCornerTable};
use crate::types::{CornerIdx, NdVector, PointIdx, Vector, VertexIdx};

/// The most parallelograms a single prediction may combine.
pub const MAX_PARALLELOGRAMS: usize = 4;

/// The crease bits, one stream per parallelogram-count context. The encoder
/// appends with [`Creases::push`], the decoder installs a decoded set and walks
/// it with [`Creases::next`]; `pos` is unused in the encode direction.
#[derive(Default)]
pub struct Creases {
    bits: [Vec<bool>; MAX_PARALLELOGRAMS],
    pos: [usize; MAX_PARALLELOGRAMS],
}

impl Creases {
    pub fn new(bits: [Vec<bool>; MAX_PARALLELOGRAMS]) -> Self {
        Self {
            bits,
            pos: [0; MAX_PARALLELOGRAMS],
        }
    }

    /// The bit streams, in context order.
    pub fn bits(&self) -> &[Vec<bool>; MAX_PARALLELOGRAMS] {
        &self.bits
    }

    /// Appends a bit to context `ctx`.
    #[inline]
    fn push(&mut self, ctx: usize, bit: bool) {
        self.bits[ctx].push(bit);
    }

    /// The next bit of context `ctx`; a malformed stream that runs out of bits
    /// reads as crease, so every remaining parallelogram is left unused.
    #[inline]
    fn next(&mut self, ctx: usize) -> bool {
        let p = self.pos[ctx];
        self.pos[ctx] += 1;
        self.bits[ctx].get(p).copied().unwrap_or(true)
    }
}

/// A candidate configuration's cost: coded size first, absolute residual as the
/// tie-break.
#[derive(Clone, Copy)]
struct SelectionError {
    bits: i64,
    residual: u64,
}

impl SelectionError {
    #[inline]
    fn is_better_than(&self, other: &SelectionError) -> bool {
        (self.bits, self.residual) < (other.bits, other.residual)
    }
}

pub struct MeshConstrainedMultiParallelogramPrediction<
    'parents,
    const N: usize,
    D: GenericAttributeDs,
> {
    ads: &'parents D,
    /// O(1) visited membership; `synced` tracks the folded prefix of the
    /// running sequence.
    visited: Vec<bool>,
    synced: usize,
    /// Crease decisions per context: recorded by `predict::<true>`, consumed by
    /// `predict::<false>`.
    creases: Creases,
    /// Encoder-side residual entropy, driving the configuration selection.
    entropy: ShannonEntropyTracker,
    total_parallelograms: [u64; MAX_PARALLELOGRAMS],
    total_used_parallelograms: [u64; MAX_PARALLELOGRAMS],
}

impl<'parents, const N: usize, D: GenericAttributeDs>
    MeshConstrainedMultiParallelogramPrediction<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    #[inline]
    fn sync(&mut self, vertices_up_till_now: &[VertexIdx]) {
        for &v in &vertices_up_till_now[self.synced..] {
            self.visited[usize::from(v)] = true;
        }
        self.synced = vertices_up_till_now.len();
    }

    /// The parallelogram prediction across the edge opposite `ci`, if all
    /// three source vertices are already processed.
    #[inline]
    fn parallelogram_at(
        &self,
        ci: CornerIdx,
        read: &impl Fn(PointIdx) -> NdVector<N, i32>,
    ) -> Option<NdVector<N, i32>> {
        let opp = self.ads.corner_table().opposite(ci)?;
        let opp_next = opp.next();
        let opp_prev = opp.previous();
        let all_processed = self.visited[usize::from(self.ads.vertex_idx(opp))]
            && self.visited[usize::from(self.ads.vertex_idx(opp_next))]
            && self.visited[usize::from(self.ads.vertex_idx(opp_prev))];
        if !all_processed {
            return None;
        }
        let opp_val = read(self.ads.point_idx(opp));
        let next_val = read(self.ads.point_idx(opp_next));
        let prev_val = read(self.ads.point_idx(opp_prev));
        let mut pred = NdVector::<N, i32>::zero();
        for i in 0..N {
            *pred.get_mut(i) =
                (*next_val.get(i) as i64 + *prev_val.get(i) as i64 - *opp_val.get(i) as i64) as i32;
        }
        Some(pred)
    }

    /// Gathers the parallelogram predictions available at corner `c`, in the
    /// reference order: swinging left from `c` first, then right from `c` after
    /// hitting a boundary.
    fn gather(
        &self,
        c: CornerIdx,
        read: &impl Fn(PointIdx) -> NdVector<N, i32>,
    ) -> ([NdVector<N, i32>; MAX_PARALLELOGRAMS], usize) {
        let ct = self.ads.corner_table();
        let mut preds = [NdVector::<N, i32>::zero(); MAX_PARALLELOGRAMS];
        let mut n = 0;
        let mut first_pass = true;
        let mut corner = Some(c);
        while let Some(ci) = corner {
            if let Some(p) = self.parallelogram_at(ci, read) {
                preds[n] = p;
                n += 1;
                if n == MAX_PARALLELOGRAMS {
                    break;
                }
            }
            let stepped = if first_pass {
                ct.swing_left(ci)
            } else {
                ct.swing_right(ci)
            };
            corner = match stepped {
                Some(nc) if nc == c => None,
                Some(nc) => Some(nc),
                None if first_pass => {
                    first_pass = false;
                    ct.swing_right(c)
                }
                None => None,
            };
        }
        (preds, n)
    }

    /// Averages the parallelograms selected by `mask`, with the exact wrapping
    /// sum and truncating division of the reference decoder.
    fn combine(preds: &[NdVector<N, i32>], mask: u8) -> NdVector<N, i32> {
        let num_used = mask.count_ones() as i32;
        let mut sum = NdVector::<N, i32>::zero();
        for (j, p) in preds.iter().enumerate() {
            if mask & (1 << j) != 0 {
                for i in 0..N {
                    *sum.get_mut(i) = sum.get(i).wrapping_add(*p.get(i));
                }
            }
        }
        for i in 0..N {
            *sum.get_mut(i) /= num_used;
        }
        sum
    }

    /// The cost of coding `pred - actual` on top of the residuals selected so
    /// far.
    fn error_of(&mut self, pred: &NdVector<N, i32>, actual: &NdVector<N, i32>) -> SelectionError {
        let mut symbols = [0u32; N];
        let mut residual = 0u64;
        for (i, s) in symbols.iter_mut().enumerate() {
            let dif = pred.get(i).wrapping_sub(*actual.get(i));
            residual += dif.unsigned_abs() as u64;
            *s = signed_to_symbol(dif);
        }
        let data = self.entropy.peek(&symbols);
        SelectionError {
            bits: ShannonEntropyTracker::data_bits(&data)
                + ShannonEntropyTracker::rans_table_bits(&data),
            residual,
        }
    }

    /// Bits needed to store the crease bits of a context with
    /// `total_used` used parallelograms out of `total` recorded.
    fn overhead_bits(total_used: u64, total: u64) -> i64 {
        (total as f64 * binary_entropy(total, total_used)).ceil() as i64
    }

    /// The crease bits recorded while encoding.
    pub fn creases(&self) -> &Creases {
        &self.creases
    }

    /// Installs the crease bits decoded from the stream, to be consumed by
    /// `predict::<false>`.
    pub fn set_creases(&mut self, creases: Creases) {
        self.creases = creases;
    }
}

impl<'parents, const N: usize, D: GenericAttributeDs> PredictionSchemeImpl<'parents, N, D>
    for MeshConstrainedMultiParallelogramPrediction<'parents, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    fn new(_parents: &[&'parents Attribute], ads: &'parents D) -> Self {
        Self {
            visited: vec![false; ads.vertex_index_bound()],
            synced: 0,
            creases: Creases::default(),
            entropy: ShannonEntropyTracker::new(),
            total_parallelograms: [0; MAX_PARALLELOGRAMS],
            total_used_parallelograms: [0; MAX_PARALLELOGRAMS],
            ads,
        }
    }

    /// The encode-side prediction at corner `c`: evaluates every subset of the
    /// available parallelograms (plus the delta fallback) against the running
    /// residual entropy, records the winning subset's crease bits, and returns
    /// its prediction.
    #[inline]
    fn predict<const ENCODING: bool>(
        &mut self,
        c: CornerIdx,
        vertices_up_till_now: &[VertexIdx],
        attribute: &Attribute,
    ) -> NdVector<N, i32> {
        self.sync(vertices_up_till_now);
        let map = attribute.point_map_as_slice();
        let vals = attribute.unique_vals_as_slice::<NdVector<N, i32>>();
        let read = |p: PointIdx| -> NdVector<N, i32> {
            match map {
                Some(m) => vals[usize::from(m[usize::from(p)])],
                None => vals[usize::from(p)],
            }
        };
        let Some(&last_v) = vertices_up_till_now.last() else {
            // The first value has no prediction source; its correction carries
            // the value itself and no crease bits are recorded.
            return NdVector::<N, i32>::zero();
        };
        if !ENCODING {
            let (preds, n) = self.gather(c, &read);
            let mut mask = 0u8;
            for j in 0..n {
                if !self.creases.next(n - 1) {
                    mask |= 1 << j;
                }
            }
            return if mask == 0 {
                read(self.ads.point_idx(self.ads.left_most_corner(last_v)))
            } else {
                Self::combine(&preds[..n], mask)
            };
        }
        let actual = read(self.ads.point_idx(c));
        let delta_pred = read(self.ads.point_idx(self.ads.left_most_corner(last_v)));
        let (preds, n) = self.gather(c, &read);

        let mut best_error = self.error_of(&delta_pred, &actual);
        if n > 0 {
            let ctx = n - 1;
            self.total_parallelograms[ctx] += n as u64;
            best_error.bits += Self::overhead_bits(
                self.total_used_parallelograms[ctx],
                self.total_parallelograms[ctx],
            );
        }
        let mut best_mask = 0u8;
        let mut best_pred = delta_pred;
        for mask in 1u8..(1 << n) {
            let ctx = n - 1;
            let pred = Self::combine(&preds[..n], mask);
            let mut error = self.error_of(&pred, &actual);
            error.bits += Self::overhead_bits(
                self.total_used_parallelograms[ctx] + mask.count_ones() as u64,
                self.total_parallelograms[ctx],
            );
            if error.is_better_than(&best_error) {
                best_error = error;
                best_mask = mask;
                best_pred = pred;
            }
        }
        if n > 0 {
            self.total_used_parallelograms[n - 1] += best_mask.count_ones() as u64;
            for j in 0..n {
                self.creases.push(n - 1, best_mask & (1 << j) == 0);
            }
        }
        let mut symbols = [0u32; N];
        for (i, s) in symbols.iter_mut().enumerate() {
            *s = signed_to_symbol(best_pred.get(i).wrapping_sub(*actual.get(i)));
        }
        self.entropy.push(&symbols);
        best_pred
    }
}
