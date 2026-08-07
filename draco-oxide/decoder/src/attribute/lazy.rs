//! Decode of attribute groups whose connectivity carries interior seams.

use crate::entropy::unzigzag;
use crate::{AttributeTransform, Err};
use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::codec::attribute::prediction_scheme::mesh_normal_prediction::{
    canonical_normal_to_oct, sum_to_canonical_normal,
};
use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::mesh::ds::{CornerTable, GenericCornerTable};
use draco_oxide_core::types::{AttributeValueIdx, CornerIdx, PointIdx, VertexIdx};
use draco_oxide_core::types::{Cross, Dot, NdVector, Vector};

use super::inverse_transform::InverseTransform;
use super::{Corrections, Descriptor, Parsed, ParsedPayload};

/// Sentinel rank of a not-yet-emitted sector.
pub(super) const UNRANKED: u32 = u32::MAX;

/// Connectivity shared by every lazy group.
#[derive(Clone, Copy)]
pub(super) struct Shared<'a> {
    pub pos_ct: &'a CornerTable,
    pub c2v: &'a [VertexIdx],
}

/// The point partition: per-corner point ids and one representative corner per point.
pub(super) struct AssembledPoints {
    pub corner_to_point: Vec<PointIdx>,
    pub rep_corners: Vec<CornerIdx>,
}

/// Splits the assembling walk's sectors into union-seam points.
struct Assembler {
    union_mask: u8,
    corner_to_point: Vec<PointIdx>,
    rep_corners: Vec<CornerIdx>,
    last_run: Vec<CornerIdx>,
}

impl Assembler {
    fn new(union_mask: u8, num_corners: usize, num_values: usize) -> Self {
        Self {
            union_mask,
            corner_to_point: vec![PointIdx::INVALID; num_corners],
            rep_corners: Vec::with_capacity(num_values),
            last_run: Vec::new(),
        }
    }

    #[inline]
    fn open_point(&mut self, corner: CornerIdx) -> PointIdx {
        let p = PointIdx::from(self.rep_corners.len());
        self.rep_corners.push(corner);
        p
    }

    fn merge_wrap(&mut self, first: PointIdx) {
        for &cc in &self.last_run {
            self.corner_to_point[usize::from(cc)] = first;
        }
        self.rep_corners.pop();
    }

    fn finish(self) -> AssembledPoints {
        debug_assert!(
            self.corner_to_point.iter().all(|&p| p != PointIdx::INVALID),
            "point assembly left a corner unassigned"
        );
        AssembledPoints {
            corner_to_point: self.corner_to_point,
            rep_corners: self.rep_corners,
        }
    }
}

/// One group's traversal; the control flow replicates core's `Traverser` so
/// ranks match the encoder's order, and the rank map is the visited set.
struct Walker<'a> {
    pos_ct: &'a CornerTable,
    seam_bits: &'a [u8],
    mask: u8,
    rank: Vec<u32>,
    visited_faces: Vec<bool>,
    stack: Vec<CornerIdx>,
    next_rank: u32,
    asm: Option<Assembler>,
}

impl<'a> Walker<'a> {
    fn new(
        pos_ct: &'a CornerTable,
        seam_bits: &'a [u8],
        mask: u8,
        seeds: &[CornerIdx],
        assemble: Option<u8>,
        num_values_hint: usize,
    ) -> Self {
        Self {
            pos_ct,
            seam_bits,
            mask,
            rank: vec![UNRANKED; seam_bits.len()],
            visited_faces: vec![false; seam_bits.len() / 3],
            stack: seeds.to_vec(),
            next_rank: 0,
            asm: assemble
                .map(|union_mask| Assembler::new(union_mask, seam_bits.len(), num_values_hint)),
        }
    }

    #[inline]
    fn opposite(&self, c: CornerIdx) -> Option<CornerIdx> {
        // SAFETY: `c` is a corner of the table, and `seam_bits` has one entry
        // per corner.
        if unsafe { *self.seam_bits.get_unchecked(usize::from(c)) } & self.mask != 0 {
            None
        } else {
            self.pos_ct.opposite(c)
        }
    }

    #[inline]
    fn is_ranked(&self, c: CornerIdx) -> bool {
        // SAFETY: `c` is a corner of the table; `rank` has one entry per corner.
        unsafe { *self.rank.get_unchecked(usize::from(c)) != UNRANKED }
    }

    /// Ranks the unranked sector containing `c`; returns whether it is open.
    #[inline]
    fn mark_sector(&mut self, c: CornerIdx) -> bool {
        if self.asm.is_some() {
            self.mark_sector_assembling(c)
        } else {
            self.mark_sector_plain(c)
        }
    }

    #[inline]
    fn mark_sector_plain(&mut self, c: CornerIdx) -> bool {
        let k = self.next_rank;
        self.next_rank += 1;
        // SAFETY: every index written is a corner of the table (`c` and results
        // of swings within it).
        unsafe {
            *self.rank.get_unchecked_mut(usize::from(c)) = k;
        }
        let mut cur = c;
        loop {
            match self.opposite(cur.next()) {
                None => break,
                Some(o) => {
                    let l = o.next();
                    if l == c {
                        return false;
                    }
                    unsafe {
                        *self.rank.get_unchecked_mut(usize::from(l)) = k;
                    }
                    cur = l;
                }
            }
        }
        let mut cur = c;
        while let Some(o) = self.opposite(cur.previous()) {
            let r = o.previous();
            unsafe {
                *self.rank.get_unchecked_mut(usize::from(r)) = k;
            }
            cur = r;
        }
        true
    }

    /// [`Self::mark_sector_plain`] plus point splitting by the union mask; a
    /// closed fan with a seam-free wrap edge merges its last run into its
    /// first point.
    #[inline]
    fn mark_sector_assembling(&mut self, c: CornerIdx) -> bool {
        let k = self.next_rank;
        self.next_rank += 1;
        let asm = self.asm.as_mut().expect("checked by mark_sector");
        unsafe {
            *self.rank.get_unchecked_mut(usize::from(c)) = k;
        }
        let first = asm.open_point(c);
        asm.corner_to_point[usize::from(c)] = first;
        asm.last_run.clear();
        asm.last_run.push(c);
        let mut cur_pt = first;

        let mut cur = c;
        let open = loop {
            let e = cur.next();
            // SAFETY: `e` is a corner of the table; `seam_bits` has one entry
            // per corner.
            let bits = unsafe { *self.seam_bits.get_unchecked(usize::from(e)) };
            if bits & self.mask != 0 {
                break true;
            }
            let Some(o) = self.pos_ct.opposite(e) else {
                break true;
            };
            let l = o.next();
            if l == c {
                if bits & asm.union_mask == 0 && cur_pt != first {
                    asm.merge_wrap(first);
                }
                return false;
            }
            unsafe {
                *self.rank.get_unchecked_mut(usize::from(l)) = k;
            }
            if bits & asm.union_mask != 0 {
                cur_pt = asm.open_point(l);
                asm.last_run.clear();
            }
            asm.corner_to_point[usize::from(l)] = cur_pt;
            asm.last_run.push(l);
            cur = l;
        };
        debug_assert!(open);

        cur_pt = first;
        let mut cur = c;
        loop {
            let e = cur.previous();
            // SAFETY: as above.
            let bits = unsafe { *self.seam_bits.get_unchecked(usize::from(e)) };
            if bits & self.mask != 0 {
                break;
            }
            let Some(o) = self.pos_ct.opposite(e) else {
                break;
            };
            let r = o.previous();
            unsafe {
                *self.rank.get_unchecked_mut(usize::from(r)) = k;
            }
            if bits & asm.union_mask != 0 {
                cur_pt = asm.open_point(r);
            }
            asm.corner_to_point[usize::from(r)] = cur_pt;
            cur = r;
        }
        true
    }

    /// Emits every attribute vertex in decode order.
    fn drive(&mut self, mut emit: impl FnMut(CornerIdx, usize, &[u32])) {
        while let Some(curr) = self.stack.pop() {
            let face = curr.face_idx();
            // SAFETY: face indices of corners of the table are below num_faces.
            if unsafe { *self.visited_faces.get_unchecked(usize::from(face)) } {
                continue;
            }
            let next_c = curr.next_with_face_idx(face);
            let prev_c = curr.previous_with_face_idx(face);
            if !self.is_ranked(next_c) || !self.is_ranked(prev_c) {
                self.stack.push(curr);
                if !self.is_ranked(next_c) {
                    let k = self.next_rank as usize;
                    self.mark_sector(next_c);
                    emit(next_c, k, &self.rank);
                }
                if !self.is_ranked(prev_c) {
                    let k = self.next_rank as usize;
                    self.mark_sector(prev_c);
                    emit(prev_c, k, &self.rank);
                }
                continue;
            }

            unsafe {
                *self.visited_faces.get_unchecked_mut(usize::from(face)) = true;
            }

            if !self.is_ranked(curr) {
                let k = self.next_rank as usize;
                let open = self.mark_sector(curr);
                emit(curr, k, &self.rank);
                if !open {
                    let right = self.opposite(next_c).expect("closed sector has all swings");
                    self.stack.push(right);
                    continue;
                }
            }

            self.push_fan_neighbors(next_c, prev_c);
        }
    }

    /// The right corner is pushed last so it is traversed first.
    #[inline]
    fn push_fan_neighbors(&mut self, next_c: CornerIdx, prev_c: CornerIdx) {
        let right = self.opposite(next_c);
        let left = self.opposite(prev_c);
        let right_visited = right.is_some_and(|c| {
            // SAFETY: face indices of corners of the table are below num_faces.
            unsafe { *self.visited_faces.get_unchecked(usize::from(c.face_idx())) }
        });
        let left_visited = left.is_some_and(|c| unsafe {
            *self.visited_faces.get_unchecked(usize::from(c.face_idx()))
        });

        if right.is_some() && right_visited {
            if !(left.is_some() && left_visited) {
                if let Some(lc) = left {
                    self.stack.push(lc);
                }
            }
        } else if left.is_some() && left_visited {
            if let Some(rc) = right {
                self.stack.push(rc);
            }
        } else {
            if let Some(lc) = left {
                self.stack.push(lc);
            }
            if let Some(rc) = right {
                self.stack.push(rc);
            }
        }
    }
}

/// One member attribute's inputs to a group walk.
pub(super) struct LazyMember<'p> {
    pub payload: ParsedPayload<'p>,
    pub dequant: AttributeTransform,
    pub desc: &'p Descriptor,
    pub parent: Option<&'p Attribute>,
}

/// The walk-independent inputs of one group run.
pub(super) struct LazyIo<'c> {
    pub num_values: usize,
    pub seeds: &'c [CornerIdx],
    pub replay: Option<Vec<CornerIdx>>,
    pub record_walk: bool,
    pub rank: &'c mut Option<Vec<u32>>,
    pub assemble: Option<u8>,
}

/// The decoded members plus the walk byproducts.
pub(super) struct LazyOutcome {
    pub members: Vec<(Attribute, AttributeTransform)>,
    pub recorded: Option<Vec<CornerIdx>>,
    pub points: Option<AssembledPoints>,
}

/// Runs one lazy group: one walk (or replay) decoding every member per emit.
pub(super) fn run_group(
    shared: Shared<'_>,
    seam_bits: &[u8],
    mask: u8,
    members: Vec<LazyMember<'_>>,
    io: LazyIo<'_>,
) -> Result<LazyOutcome, Err> {
    let LazyIo {
        num_values,
        seeds,
        replay,
        record_walk,
        rank,
        assemble,
    } = io;
    let mut steppers: Vec<AnyLazyStepper<'_>> = Vec::with_capacity(members.len());
    for m in members {
        steppers.push(build_stepper(
            m.payload, m.dequant, m.desc, shared, seam_bits, mask, m.parent, num_values,
        )?);
    }

    // The walk emits corners in rank order, so the emitted sequence plus the
    // completed rank map replays the walk exactly (the `rank < k` guards read
    // the decode-time state either way).
    let mut points = None;
    let (seq, walked) = match replay {
        Some(seq) => {
            debug_assert!(assemble.is_none(), "assembly runs on a real walk");
            (seq, false)
        }
        None => {
            let mut walker =
                Walker::new(shared.pos_ct, seam_bits, mask, seeds, assemble, num_values);
            let mut seq = Vec::with_capacity(num_values);
            walker.drive(|c, _, _| seq.push(c));
            points = walker.asm.take().map(Assembler::finish);
            *rank = Some(walker.rank);
            (seq, true)
        }
    };
    if seq.len() != num_values {
        return Err(Err::MalformedAttribute(
            "traversal did not reach every attribute vertex",
        ));
    }
    let rank_map = rank.as_deref().ok_or(Err::MalformedAttribute(
        "replayed traversal group was never walked",
    ))?;
    for s in steppers.iter_mut() {
        s.run(&seq, rank_map);
        s.finalize(rank_map);
    }

    Ok(LazyOutcome {
        members: steppers.into_iter().map(AnyLazyStepper::finish).collect(),
        recorded: (walked && record_walk).then_some(seq),
        points,
    })
}

/// One attribute's decode state through a walk.
struct LazyStepper<'p, const N: usize>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    vals: Vec<NdVector<N, i32>>,
    predictor: LazyPredictor<'p, N>,
    transform: InverseTransform,
    corrections: Corrections<'p, N>,
    zigzagged: bool,
    dequant: AttributeTransform,
    att_meta: (
        draco_oxide_core::attribute::AttributeId,
        Vec<draco_oxide_core::attribute::AttributeId>,
    ),
    att_type: draco_oxide_core::attribute::AttributeType,
    domain: draco_oxide_core::attribute::AttributeDomain,
}

impl<'p, const N: usize> LazyStepper<'p, N>
where
    NdVector<N, i32>: Vector<N, Component = i32> + Portable,
{
    #[allow(clippy::too_many_arguments)]
    fn new(
        parsed: Parsed<'p, N>,
        dequant: AttributeTransform,
        desc: &Descriptor,
        shared: Shared<'p>,
        seam_bits: &'p [u8],
        mask: u8,
        parent: Option<&'p Attribute>,
        num_values: usize,
    ) -> Result<Self, Err> {
        let parents_ids = parent.map(|p| vec![p.get_id()]).unwrap_or_default();
        let predictor = LazyPredictor::new(
            &parsed.scheme_ty,
            shared,
            seam_bits,
            mask,
            parent,
            parsed.flips,
            parsed.orientations,
            parsed.transform.oct_center(),
        )?;
        let zigzagged = parsed.transform.corrections_are_zigzagged();
        Ok(Self {
            vals: vec![NdVector::<N, i32>::zero(); num_values],
            predictor,
            transform: parsed.transform,
            corrections: parsed.corrections,
            zigzagged,
            dequant,
            att_meta: (
                draco_oxide_core::attribute::AttributeId::new(desc.uid as usize),
                parents_ids,
            ),
            att_type: desc.att_type,
            domain: desc.domain,
        })
    }

    /// Decodes every value along the emitted corner sequence `seq` over the
    /// completed rank map; normal attributes wait for [`Self::finalize`].
    /// Consulted neighbors are never the current sector, so `rank < k` means
    /// decoded, on walks and replays alike.
    fn run(&mut self, seq: &[CornerIdx], rank: &[u32]) {
        let LazyStepper {
            vals,
            predictor,
            transform,
            corrections,
            zigzagged,
            ..
        } = self;
        match predictor {
            LazyPredictor::Normal { .. } => {}
            LazyPredictor::NoPrediction => run_steps(
                seq,
                rank,
                vals,
                corrections,
                transform,
                *zigzagged,
                |_, _, _, _| NdVector::zero(),
            ),
            LazyPredictor::Delta => run_steps(
                seq,
                rank,
                vals,
                corrections,
                transform,
                *zigzagged,
                |_, k, _, vals| delta_predict(k, vals),
            ),
            LazyPredictor::Parallelogram {
                pos_ct,
                seam_bits,
                mask,
            } => run_steps(
                seq,
                rank,
                vals,
                corrections,
                transform,
                *zigzagged,
                |c, k, rank, vals| {
                    parallelogram_predict(c, k, rank, vals, pos_ct, seam_bits, *mask)
                },
            ),
            LazyPredictor::TexCoords {
                c2v,
                pos_map,
                pos_vals,
                orientations,
            } => run_steps(
                seq,
                rank,
                vals,
                corrections,
                transform,
                *zigzagged,
                |c, k, rank, vals| {
                    texcoords_predict(c, k, rank, vals, c2v, pos_map, pos_vals, orientations)
                },
            ),
        }
    }

    /// Decodes a normal attribute over the completed rank map: face-normal
    /// sums per rank, then ranks finalize in order, which is the correction
    /// stream's order. Arithmetic identical to `accumulate_face_normal_sums`.
    fn finalize(&mut self, rank: &[u32]) {
        let LazyPredictor::Normal {
            c2v,
            pos_map,
            pos_vals,
            center,
            flips,
        } = &self.predictor
        else {
            return;
        };
        // Folds at monomorphization; other component counts carry no body.
        assert!(
            N == 2,
            "normal prediction is only for 2D octahedral vectors"
        );
        let num_values = self.vals.len();

        let pos32 = |c: CornerIdx| -> NdVector<3, i32> {
            let p = usize::from(c2v[usize::from(c)]);
            pos_map
                .get(p)
                .and_then(|&idx| pos_vals.get(usize::from(idx)))
                .copied()
                .unwrap_or_else(NdVector::<3, i32>::zero)
        };
        let widen = |v: NdVector<3, i32>| {
            NdVector::<3, i64>::from([*v.get(0) as i64, *v.get(1) as i64, *v.get(2) as i64])
        };
        let mut sums = vec![NdVector::<3, i64>::zero(); num_values];
        for f in 0..rank.len() / 3 {
            let c0 = CornerIdx::from(3 * f);
            let c1 = CornerIdx::from(3 * f + 1);
            let c2 = CornerIdx::from(3 * f + 2);

            let pos_c = pos32(c0);
            let delta_next = pos32(c1) - pos_c;
            let delta_prev = pos32(c2) - pos_c;
            let cross = widen(delta_next).cross(widen(delta_prev));

            sums[rank[usize::from(c0)] as usize] += cross;
            sums[rank[usize::from(c1)] as usize] += cross;
            sums[rank[usize::from(c2)] as usize] += cross;
        }

        for (k, &sum) in sums.iter().enumerate() {
            let mut pred_3d = sum_to_canonical_normal(sum, *center);
            if flips.get(k).copied().unwrap_or(false) {
                pred_3d *= -1;
            }
            let pred = canonical_normal_to_oct::<N>(pred_3d, *center);
            // SAFETY: k < num_values, the length of the value buffer and the
            // correction stream, consumed in rank order.
            let mut corr = unsafe { self.corrections.next_unchecked(k) };
            if self.zigzagged {
                for i in 0..N {
                    *corr.get_mut(i) = unzigzag(*corr.get(i) as u32);
                }
            }
            self.vals[k] = self.transform.compute_original(pred, corr);
        }
    }

    fn finish(self) -> (Attribute, AttributeTransform) {
        let att = Attribute::from_without_removing_duplicates::<NdVector<N, i32>, N>(
            self.att_meta.0,
            self.vals,
            self.att_type,
            self.domain,
            self.att_meta.1,
        );
        (att, self.dequant)
    }
}

/// [`LazyStepper`] behind the component-count dispatch.
enum AnyLazyStepper<'p> {
    N1(LazyStepper<'p, 1>),
    N2(LazyStepper<'p, 2>),
    N3(LazyStepper<'p, 3>),
    N4(LazyStepper<'p, 4>),
}

impl<'p> AnyLazyStepper<'p> {
    fn run(&mut self, seq: &[CornerIdx], rank: &[u32]) {
        match self {
            AnyLazyStepper::N1(s) => s.run(seq, rank),
            AnyLazyStepper::N2(s) => s.run(seq, rank),
            AnyLazyStepper::N3(s) => s.run(seq, rank),
            AnyLazyStepper::N4(s) => s.run(seq, rank),
        }
    }

    fn finalize(&mut self, rank: &[u32]) {
        match self {
            AnyLazyStepper::N1(s) => s.finalize(rank),
            AnyLazyStepper::N2(s) => s.finalize(rank),
            AnyLazyStepper::N3(s) => s.finalize(rank),
            AnyLazyStepper::N4(s) => s.finalize(rank),
        }
    }

    fn finish(self) -> (Attribute, AttributeTransform) {
        match self {
            AnyLazyStepper::N1(s) => s.finish(),
            AnyLazyStepper::N2(s) => s.finish(),
            AnyLazyStepper::N3(s) => s.finish(),
            AnyLazyStepper::N4(s) => s.finish(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_stepper<'p>(
    payload: ParsedPayload<'p>,
    dequant: AttributeTransform,
    desc: &Descriptor,
    shared: Shared<'p>,
    seam_bits: &'p [u8],
    mask: u8,
    parent: Option<&'p Attribute>,
    num_values: usize,
) -> Result<AnyLazyStepper<'p>, Err> {
    Ok(match payload {
        ParsedPayload::N1(p) => AnyLazyStepper::N1(LazyStepper::new(
            p, dequant, desc, shared, seam_bits, mask, parent, num_values,
        )?),
        ParsedPayload::N2(p) => AnyLazyStepper::N2(LazyStepper::new(
            p, dequant, desc, shared, seam_bits, mask, parent, num_values,
        )?),
        ParsedPayload::N3(p) => AnyLazyStepper::N3(LazyStepper::new(
            p, dequant, desc, shared, seam_bits, mask, parent, num_values,
        )?),
        ParsedPayload::N4(p) => AnyLazyStepper::N4(LazyStepper::new(
            p, dequant, desc, shared, seam_bits, mask, parent, num_values,
        )?),
    })
}

/// Rank-based predictors reading neighbors through the corner-to-rank map.
enum LazyPredictor<'p, const N: usize>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    NoPrediction,
    Delta,
    Parallelogram {
        pos_ct: &'p CornerTable,
        seam_bits: &'p [u8],
        mask: u8,
    },
    TexCoords {
        c2v: &'p [VertexIdx],
        pos_map: &'p [draco_oxide_core::types::AttributeValueIdx],
        pos_vals: &'p [NdVector<3, i32>],
        orientations: Vec<bool>,
    },
    Normal {
        c2v: &'p [VertexIdx],
        pos_map: &'p [AttributeValueIdx],
        pos_vals: &'p [NdVector<3, i32>],
        center: i32,
        flips: Vec<bool>,
    },
}

impl<'p, const N: usize> LazyPredictor<'p, N>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    #[allow(clippy::too_many_arguments)]
    fn new(
        scheme_ty: &PredictionSchemeType,
        shared: Shared<'p>,
        seam_bits: &'p [u8],
        mask: u8,
        parent: Option<&'p Attribute>,
        flips: Vec<bool>,
        orientations: Vec<bool>,
        oct_center: i32,
    ) -> Result<Self, Err> {
        Ok(match scheme_ty {
            PredictionSchemeType::NoPrediction => LazyPredictor::NoPrediction,
            PredictionSchemeType::DeltaPrediction => LazyPredictor::Delta,
            PredictionSchemeType::MeshParallelogramPrediction => LazyPredictor::Parallelogram {
                pos_ct: shared.pos_ct,
                seam_bits,
                mask,
            },
            PredictionSchemeType::MeshPredictionForTextureCoordinates => {
                if N != 2 {
                    return Err(Err::MalformedAttribute(
                        "texture coordinate prediction requires a 2-component attribute",
                    ));
                }
                let pos = parent.ok_or(Err::MalformedAttribute(
                    "geometric prediction requires an already decoded position attribute",
                ))?;
                LazyPredictor::TexCoords {
                    c2v: shared.c2v,
                    pos_map: pos.point_map_as_slice().ok_or(Err::MalformedAttribute(
                        "parent position attribute carries no point map",
                    ))?,
                    pos_vals: pos.unique_vals_as_slice::<NdVector<3, i32>>(),
                    orientations,
                }
            }
            PredictionSchemeType::MeshNormalPrediction => {
                if N != 2 {
                    return Err(Err::MalformedAttribute(
                        "normal prediction requires a 2-component octahedral attribute",
                    ));
                }
                if oct_center <= 0 {
                    return Err(Err::MalformedAttribute(
                        "normal prediction needs an octahedral prediction transform",
                    ));
                }
                let pos = parent.ok_or(Err::MalformedAttribute(
                    "geometric prediction requires an already decoded position attribute",
                ))?;
                LazyPredictor::Normal {
                    c2v: shared.c2v,
                    pos_map: pos.point_map_as_slice().ok_or(Err::MalformedAttribute(
                        "parent position attribute carries no point map",
                    ))?,
                    pos_vals: pos.unique_vals_as_slice::<NdVector<3, i32>>(),
                    center: oct_center,
                    flips,
                }
            }
            // Constrained multi-parallelogram over an interior-seam walk is
            // not implemented; the reference reaches it only when a seamed
            // attribute falls through to the generic predictor at speed <= 1.
            // The other two schemes are never emitted.
            PredictionSchemeType::MeshConstrainedMultiParallelogramPrediction
            | PredictionSchemeType::MeshMultiParallelogramPrediction
            | PredictionSchemeType::DerivativePrediction => return Err(Err::Unimplemented),
            PredictionSchemeType::Invalid => {
                return Err(Err::MalformedAttribute("invalid prediction scheme"))
            }
        })
    }
}

/// The per-value decode loop, monomorphic over `predict`. `seq` must hold
/// exactly as many corners as the value buffer holds values.
#[inline]
fn run_steps<const N: usize>(
    seq: &[CornerIdx],
    rank: &[u32],
    vals: &mut [NdVector<N, i32>],
    corrections: &mut Corrections<'_, N>,
    transform: &InverseTransform,
    zigzagged: bool,
    mut predict: impl FnMut(CornerIdx, usize, &[u32], &[NdVector<N, i32>]) -> NdVector<N, i32>,
) where
    NdVector<N, i32>: Vector<N, Component = i32> + Portable,
{
    for (k, &c) in seq.iter().enumerate() {
        let pred = predict(c, k, rank, vals);
        // SAFETY: the caller checked the sequence against the value count, so
        // k is below the length of both the value buffer and any eager
        // correction vector.
        let mut corr = unsafe { corrections.next_unchecked(k) };
        if zigzagged {
            for i in 0..N {
                *corr.get_mut(i) = unzigzag(*corr.get(i) as u32);
            }
        }
        unsafe {
            *vals.get_unchecked_mut(k) = transform.compute_original(pred, corr);
        }
    }
}

/// The preceding value, taken as zero at the first one.
#[inline]
fn delta_predict<const N: usize>(k: usize, vals: &[NdVector<N, i32>]) -> NdVector<N, i32>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    if k > 0 {
        vals[k - 1]
    } else {
        NdVector::zero()
    }
}

/// Parallelogram prediction over the seam-cut connectivity; falls back to
/// delta when the opposite face's sectors are not all decoded.
#[inline]
fn parallelogram_predict<const N: usize>(
    c: CornerIdx,
    k: usize,
    rank: &[u32],
    vals: &[NdVector<N, i32>],
    pos_ct: &CornerTable,
    seam_bits: &[u8],
    mask: u8,
) -> NdVector<N, i32>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    // SAFETY: `c` and its face-mates and opposite are corners of
    // the table, below the per-corner lengths of `seam` and
    // `rank`; every assigned rank is below the walk's value count,
    // the length of `vals`.
    // A neighbor is decoded iff its rank is below `k`: during the
    // walk every assigned rank but the current sector's is below
    // `k` (UNRANKED never is), and on a replay, where the rank map
    // is already complete, the comparison recovers the decode-time
    // state.
    let kk = k as u32;
    unsafe {
        let opp = if *seam_bits.get_unchecked(usize::from(c)) & mask != 0 {
            None
        } else {
            pos_ct.opposite(c)
        };
        if let Some(opp) = opp {
            let r_opp = *rank.get_unchecked(usize::from(opp));
            let r_next = *rank.get_unchecked(usize::from(c.next()));
            let r_prev = *rank.get_unchecked(usize::from(c.previous()));
            if r_opp < kk && r_next < kk && r_prev < kk {
                return *vals.get_unchecked(r_next as usize) + *vals.get_unchecked(r_prev as usize)
                    - *vals.get_unchecked(r_opp as usize);
            }
        }
    }
    delta_predict(k, vals)
}

/// The parent position at a corner; out-of-range indices read as zero.
#[inline]
fn pos_at(
    c: CornerIdx,
    c2v: &[VertexIdx],
    pos_map: &[AttributeValueIdx],
    pos_vals: &[NdVector<3, i32>],
) -> NdVector<3, i64> {
    let p = usize::from(c2v[usize::from(c)]);
    let v = pos_map
        .get(p)
        .and_then(|&idx| pos_vals.get(usize::from(idx)))
        .copied()
        .unwrap_or_else(NdVector::<3, i32>::zero);
    NdVector::<3, i64>::from([*v.get(0) as i64, *v.get(1) as i64, *v.get(2) as i64])
}

/// Integer square root with the reference draco rounding.
fn int_sqrt(value: u64) -> u64 {
    if value == 0 {
        return 0;
    }
    let mut act_number = value;
    let mut sqrt = 1u64;
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

/// Texture-coordinate prediction; arithmetic identical to core's
/// `compute_prediction`, only the data access differs.
#[allow(clippy::too_many_arguments)]
fn texcoords_predict<const N: usize>(
    c: CornerIdx,
    k: usize,
    rank: &[u32],
    vals: &[NdVector<N, i32>],
    c2v: &[VertexIdx],
    pos_map: &[AttributeValueIdx],
    pos_vals: &[NdVector<3, i32>],
    orientations: &mut Vec<bool>,
) -> NdVector<N, i32>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    // Folds at monomorphization; other component counts carry no body.
    assert!(
        N == 2,
        "texture coordinate prediction is only for 2D vectors"
    );
    let next_c = c.next();
    let prev_c = c.previous();
    let kk = k as u32;
    // SAFETY: face-mates of `c` are corners of the table, below `rank`'s
    // per-corner length; assigned ranks are below the walk's value count, the
    // length of `vals`.
    let r_next = unsafe { *rank.get_unchecked(usize::from(next_c)) };
    let r_prev = unsafe { *rank.get_unchecked(usize::from(prev_c)) };

    let fallback = |orient_free_vals: &[NdVector<N, i32>]| -> NdVector<N, i32> {
        if r_next < kk {
            // SAFETY: as above, an assigned rank is below `vals.len()`.
            return unsafe { *orient_free_vals.get_unchecked(r_next as usize) };
        }
        if k > 0 {
            return orient_free_vals[k - 1];
        }
        NdVector::zero()
    };

    if r_next >= kk || r_prev >= kk {
        return fallback(vals);
    }

    // SAFETY: assigned ranks are below `vals.len()`.
    let next_uv32 = unsafe { *vals.get_unchecked(r_next as usize) };
    let prev_uv32 = unsafe { *vals.get_unchecked(r_prev as usize) };
    let next_uv = NdVector::<2, i64>::from([*next_uv32.get(0) as i64, *next_uv32.get(1) as i64]);
    let prev_uv = NdVector::<2, i64>::from([*prev_uv32.get(0) as i64, *prev_uv32.get(1) as i64]);
    if next_uv == prev_uv {
        return prev_uv32;
    }

    let curr_pos = pos_at(c, c2v, pos_map, pos_vals);
    let next_pos = pos_at(next_c, c2v, pos_map, pos_vals);
    let prev_pos = pos_at(prev_c, c2v, pos_map, pos_vals);

    let pn = prev_pos - next_pos;
    let pn_norm2_squared = pn.dot(pn) as u64;
    if pn_norm2_squared != 0 {
        let cn = curr_pos - next_pos;
        let cn_dot_pn = pn.dot(cn);
        let pn_uv = prev_uv - next_uv;

        let n_uv_absmax = next_uv.get(0).abs().max(next_uv.get(1).abs());
        if n_uv_absmax > i64::MAX / pn_norm2_squared as i64 {
            return fallback(vals);
        }
        let pn_uv_absmax = pn_uv.get(0).abs().max(pn_uv.get(1).abs());
        if cn_dot_pn.abs() > i64::MAX / pn_uv_absmax {
            return fallback(vals);
        }
        let x_uv = next_uv * pn_norm2_squared as i64 + pn_uv * cn_dot_pn;
        let pn_absmax = pn.get(0).abs().max(pn.get(1).abs()).max(pn.get(2).abs());
        if cn_dot_pn.abs() > i64::MAX / pn_absmax {
            return fallback(vals);
        }
        let x_pos = next_pos + pn * cn_dot_pn / pn_norm2_squared as i64;
        let cx_norm2_squared = (curr_pos - x_pos).dot(curr_pos - x_pos) as u64;
        let mut cx_uv = NdVector::<2, i64>::from([*pn_uv.get(1), -pn_uv.get(0)]);
        let norm_squared = int_sqrt(cx_norm2_squared * pn_norm2_squared);
        cx_uv *= norm_squared as i64;

        let predicted_uv_0 = (x_uv + cx_uv) / (pn_norm2_squared as i64);
        let predicted_uv_1 = (x_uv - cx_uv) / (pn_norm2_squared as i64);
        let chosen = if orientations.pop().unwrap_or(true) {
            predicted_uv_0
        } else {
            predicted_uv_1
        };
        let mut out = NdVector::<N, i32>::zero();
        *out.get_mut(0) = *chosen.get(0) as i32;
        *out.get_mut(1) = *chosen.get(1) as i32;
        return out;
    }

    fallback(vals)
}
