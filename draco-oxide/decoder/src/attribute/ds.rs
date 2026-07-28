//! Builds the decoder-side point data structure and per-attribute vertex maps
//! from the position fans the edgebreaker reconstruction already produced: the
//! seamless case reads them straight from `corner_to_vertex`, the seamed cases
//! sweep each fan from its reconstruction seed to recover the corner order the
//! sector split needs.

use std::mem::{ManuallyDrop, MaybeUninit};

use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::mesh::ds::{
    AttributeCornerTable, AttributeDS, CornerTable, GenericCornerTable, IdentityDS, DS,
};
use draco_oxide_core::types::{
    CornerIdx, PointIdx, VecCornerIdx, VecPointIdx, VecVertexIdx, VertexIdx,
};

/// A corner-indexed map that is allocated uninitialized and filled entirely
/// before it is read. The fan walk assigns every corner, so a prefill would be
/// overwritten in full.
struct UninitCornerMap<T: Copy> {
    inner: VecCornerIdx<MaybeUninit<T>>,
    /// Debug-only witness that every entry was written, so a missed corner
    /// fails loudly in tests instead of reading uninitialized memory.
    #[cfg(debug_assertions)]
    written: VecCornerIdx<bool>,
}

impl<T: Copy> UninitCornerMap<T> {
    fn new(len: usize) -> Self {
        let mut inner = Vec::with_capacity(len);
        // SAFETY: `MaybeUninit<T>` needs no initialization to be a valid
        // element, and `len` equals the capacity just reserved.
        unsafe { inner.set_len(len) };
        Self {
            inner: inner.into(),
            #[cfg(debug_assertions)]
            written: vec![false; len].into(),
        }
    }

    /// # Safety
    /// `idx` must be less than the length this map was created with.
    #[inline]
    unsafe fn set(&mut self, idx: CornerIdx, value: T) {
        #[cfg(debug_assertions)]
        {
            self.written[idx] = true;
        }
        self.inner.get_unchecked_mut(idx).write(value);
    }

    /// # Safety
    /// Every entry must have been written by [`Self::set`].
    unsafe fn assume_init(self) -> VecCornerIdx<T> {
        #[cfg(debug_assertions)]
        assert!(
            self.written.iter().all(|&w| w),
            "corner map read before every corner was assigned"
        );
        let mut inner = ManuallyDrop::new(self.inner.into_inner());
        // SAFETY: `MaybeUninit<T>` shares the layout of `T`, so the allocation
        // matches what `Vec<T>` expects, and the caller guarantees every entry
        // is initialized.
        Vec::from_raw_parts(inner.as_mut_ptr() as *mut T, inner.len(), inner.capacity()).into()
    }
}

/// One attribute's sector decomposition of the position fans: per point (union
/// sector) its attribute vertex, and per vertex its left-most corner. Vertices
/// mirror the encoder's attribute vertex construction, so both sides agree on
/// which corners share an attribute value. Point-to-vertex is a function
/// because points are the finest common refinement of all attributes' sectors,
/// so every point lies inside exactly one sector of each attribute.
#[derive(Clone)]
pub(crate) struct RawAttributeDS {
    pub point_to_vertex: Vec<VertexIdx>,
    pub vertex_to_left_most_corner: Vec<CornerIdx>,
}

impl RawAttributeDS {
    fn new() -> Self {
        Self {
            point_to_vertex: Vec::new(),
            vertex_to_left_most_corner: Vec::new(),
        }
    }
}

/// The sector-left-most start index of one closed fan: the first seam reached
/// walking left from index 0, or index 1 when the fan carries no seam (the
/// right neighbor of the position-left-most corner, matching the encoder's
/// walk).
fn sector_start(is_seam: impl Fn(usize) -> bool, m: usize) -> usize {
    if m == 1 || is_seam(0) {
        return 0;
    }
    let mut j = m - 1;
    while j > 1 {
        if is_seam(j) {
            return j;
        }
        j -= 1;
    }
    1
}

/// The position connectivity fed to point assignment, reusing the structure
/// the edgebreaker reconstruction already produced: the per-corner vertex map,
/// one seed corner per vertex, and per-vertex hole flags. Sharing these avoids a
/// second walk that would rediscover the fans the reconstruction already knows.
pub(crate) struct Input<'a> {
    pub pos_ct: &'a CornerTable,
    pub corner_to_vertex: &'a [VertexIdx],
    pub vertex_corners: &'a [CornerIdx],
    pub is_vert_hole: &'a [bool],
    pub num_vertices: usize,
    pub num_corners: usize,
}

/// A left swing around the position vertex, or `None` at an open fan's left
/// boundary. Mirrors `swing_right`: `swing_left(c) = opposite(c.next()).next()`.
#[inline]
fn swing_left(pos_ct: &CornerTable, c: CornerIdx) -> Option<CornerIdx> {
    pos_ct.opposite(c.next()).map(CornerIdx::next)
}

/// The left-most corner of the open fan through `c`, found by swinging left to
/// the boundary. Callers pass only hole (open) vertices; the `l == start` guard
/// is a defensive stop so a mislabeled closed fan cannot loop forever.
fn open_fan_left_most(pos_ct: &CornerTable, c: CornerIdx) -> CornerIdx {
    let start = c;
    let mut c = c;
    while let Some(l) = swing_left(pos_ct, c) {
        if l == start {
            break;
        }
        c = l;
    }
    c
}

/// Builds the decoder-side point [`DS`] (splitting every position fan into
/// points, the sectors of the union of all seams, parallel to `seam_sets`, each
/// attribute's sector decomposition as an [`RawAttributeDS`].
pub(crate) fn build_ds(input: Input, seam_sets: &[&[bool]]) -> (DS, Vec<RawAttributeDS>) {
    // Boundary edges are seams for every attribute and never split a fan, so
    // only interior seams route an attribute off the shared fast paths.
    let mut seamed_indices = seam_sets
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.iter()
                .enumerate()
                .any(|(c, &b)| b && input.pos_ct.opposite(CornerIdx::from(c)).is_some())
        })
        .map(|(i, _)| i);
    let (corner_to_point, outputs) = match (seamed_indices.next(), seamed_indices.next()) {
        (None, _) => {
            let (corner_to_point, fv) = fan_vertices_seamless(input);
            (corner_to_point, vec![fv; seam_sets.len()])
        }
        (Some(k), None) => {
            let (corner_to_point, seamless, seamed) = fan_vertices_one_seamed(input, seam_sets[k]);
            let last_seamless = (0..seam_sets.len()).rev().find(|&i| i != k);
            let mut seamless = Some(seamless);
            let mut seamed = Some(seamed);
            let outputs = (0..seam_sets.len())
                .map(|i| {
                    if i == k {
                        seamed.take().unwrap()
                    } else if Some(i) == last_seamless {
                        seamless.take().unwrap()
                    } else {
                        seamless.clone().unwrap()
                    }
                })
                .collect();
            (corner_to_point, outputs)
        }
        _ => fan_vertices_general(input, seam_sets),
    };
    (DS::new(corner_to_point), outputs)
}

/// Either the general seam-aware attribute structure or the identity structure
/// for a seamed mesh's finest attribute, so a decode over a mix of attributes
/// holds them in one homogeneous collection. Hot traversal code never calls
/// through this enum: a group walk matches the variant once and runs
/// monomorphized on the concrete structure (`GroupWalkDs` in the attribute
/// module).
pub(crate) enum GeneralDs<'a> {
    Seamed(AttributeDS<'a>),
    Finest(IdentityDS<'a, AttributeCornerTable<'a>, PointIdx>),
}

/// Assembles each attribute's data structure, borrowing the shared point `ds`
/// and position corner table, and consuming the per-attribute vertex maps from
/// [`build_ds`], the decoded seam edges, and the placeholder attributes. The
/// three inputs are parallel and equal length (one entry per attribute). An
/// attribute whose vertex count equals the point count is the finest one (its
/// seams generate the whole refinement), so its points coincide with its
/// vertices and it takes the single-load identity structure.
pub(crate) fn build_attribute_ds<'a>(
    ds: &'a DS,
    pos_ct: &'a CornerTable,
    fans: Vec<RawAttributeDS>,
    seams: Vec<Vec<bool>>,
    placeholders: Vec<Attribute>,
) -> Vec<GeneralDs<'a>> {
    let num_points = ds.num_points();
    fans.into_iter()
        .zip(seams)
        .zip(placeholders)
        .map(|((fan, seam), placeholder)| {
            let corner_table = AttributeCornerTable::new(pos_ct, VecCornerIdx::from(seam));
            if fan.vertex_to_left_most_corner.len() == num_points {
                GeneralDs::Finest(IdentityDS::finest(
                    ds,
                    corner_table,
                    fan.vertex_to_left_most_corner,
                    placeholder,
                ))
            } else {
                GeneralDs::Seamed(AttributeDS::new(
                    ds,
                    corner_table,
                    VecVertexIdx::from(fan.vertex_to_left_most_corner),
                    VecPointIdx::from(fan.point_to_vertex),
                    placeholder,
                ))
            }
        })
        .collect()
}

/// Precomputed fan navigation: where a right swing lands from each corner.
struct FanNav {
    swing_right: VecCornerIdx<CornerIdx>,
}

impl FanNav {
    fn new(pos_ct: &CornerTable, num_corners: usize) -> Self {
        let mut swing_right = UninitCornerMap::new(num_corners);
        for c in 0..num_corners {
            let c = CornerIdx::from(c);
            let target = c.next();
            match pos_ct.opposite(c) {
                // SAFETY: `next` is a permutation of the corners, so `target`
                // is in range and each slot is written exactly once.
                Some(o) => unsafe { swing_right.set(target, o.previous()) },
                None => unsafe { swing_right.set(target, CornerIdx::INVALID) },
            }
        }
        Self {
            // SAFETY: `next` covers every corner, so every slot was written.
            swing_right: unsafe { swing_right.assume_init() },
        }
    }

    #[inline]
    fn swing_right(&self, c: CornerIdx) -> CornerIdx {
        // SAFETY: `c` is a corner of the table, so it is less than the length.
        unsafe { *self.swing_right.get_unchecked(c) }
    }
}

/// Calls `f` once per position fan with the fan's corners in right-sweep order,
/// and whether the fan is closed.
fn for_each_fan(input: Input, nav: &FanNav, mut f: impl FnMut(&[CornerIdx], bool)) {
    let mut fan: Vec<CornerIdx> = Vec::new();
    macro_rules! sweep {
        ($from:expr, $stop:expr) => {{
            fan.clear();
            let mut c = $from;
            loop {
                fan.push(c);
                let r = nav.swing_right(c);
                if r == $stop {
                    break;
                }
                c = r;
            }
        }};
    }

    for v in 0..input.num_vertices {
        let seed = input.vertex_corners[v];
        if seed == CornerIdx::INVALID {
            continue;
        }
        if input.is_vert_hole[v] {
            let left = open_fan_left_most(input.pos_ct, seed);
            sweep!(left, CornerIdx::INVALID);
            f(&fan, false);
        } else {
            sweep!(seed, seed);
            f(&fan, true);
        }
    }
}

/// Fast path when no attribute carries a seam: every fan is a single sector,
/// points coincide with position vertices, and all attributes share the same
/// vertex numbering.
fn fan_vertices_seamless(input: Input) -> (VecCornerIdx<PointIdx>, RawAttributeDS) {
    let mut vertex_to_point: Vec<PointIdx> = vec![PointIdx::INVALID; input.num_vertices];
    let mut corner_to_point = UninitCornerMap::new(input.num_corners);
    let mut out = RawAttributeDS::new();

    for c in 0..input.num_corners {
        let c = CornerIdx::from(c);
        let v = usize::from(input.corner_to_vertex[usize::from(c)]);
        let mut pt = vertex_to_point[v];
        if pt == PointIdx::INVALID {
            pt = PointIdx::from(out.vertex_to_left_most_corner.len());
            vertex_to_point[v] = pt;
            // A referenced vertex always has a seed; `c` is a defensive fallback.
            let seed = input.vertex_corners[v];
            let left_most = if seed == CornerIdx::INVALID { c } else { seed };
            debug_assert!(
                !input.is_vert_hole[v] || swing_left(input.pos_ct, left_most).is_none(),
                "hole vertex seed is not the boundary-left-most corner"
            );
            out.vertex_to_left_most_corner.push(left_most);
            out.point_to_vertex.push(VertexIdx::from(usize::from(pt)));
        }
        // SAFETY: `c` is a corner of the table, so it is less than `num_corners`.
        unsafe { corner_to_point.set(c, pt) };
    }

    // SAFETY: every corner is written exactly once in the scan above.
    (unsafe { corner_to_point.assume_init() }, out)
}

/// Fast path when exactly one attribute carries seams: the union of all seams
/// equals that attribute's seams, so points coincide with its vertices, and
/// every seamless attribute keeps one vertex per fan.
fn fan_vertices_one_seamed(
    input: Input,
    seams: &[bool],
) -> (VecCornerIdx<PointIdx>, RawAttributeDS, RawAttributeDS) {
    let mut corner_to_point = UninitCornerMap::new(input.num_corners);
    let mut seamless = RawAttributeDS::new();
    let mut seamed = RawAttributeDS::new();
    let mut num_points = 0usize;

    let nav = FanNav::new(input.pos_ct, input.num_corners);

    for_each_fan(input, &nav, |fan, closed| {
        let m = fan.len();

        let s = if closed {
            sector_start(|j| seams[usize::from(fan[j].next())], m)
        } else {
            0
        };
        let fan_vert = VertexIdx::from(seamless.vertex_to_left_most_corner.len());
        let seamless_start = if closed && m > 1 { 1 } else { 0 };
        seamless
            .vertex_to_left_most_corner
            .push(fan[seamless_start]);

        // Fused pass: point ids equal the seamed attribute's vertex ids.
        for jj in 0..m {
            let idx = (s + jj) % m;
            if jj == 0 || seams[usize::from(fan[idx].next())] {
                seamed.vertex_to_left_most_corner.push(fan[idx]);
                seamed.point_to_vertex.push(VertexIdx::from(num_points));
                seamless.point_to_vertex.push(fan_vert);
                num_points += 1;
            }
            // SAFETY: `fan` holds corner-table corners, all less than
            // `num_corners`.
            unsafe { corner_to_point.set(fan[idx], PointIdx::from(num_points - 1)) };
        }
    });

    // SAFETY: fans partition the corners and the pass above covers every corner
    // of each fan, so every entry is initialized.
    let corner_to_point = unsafe { corner_to_point.assume_init() };
    (corner_to_point, seamless, seamed)
}

fn fan_vertices_general(
    input: Input,
    seam_sets: &[&[bool]],
) -> (VecCornerIdx<PointIdx>, Vec<RawAttributeDS>) {
    let num_outputs = seam_sets.len();
    let mut outputs: Vec<RawAttributeDS> =
        (0..num_outputs).map(|_| RawAttributeDS::new()).collect();
    let mut corner_to_point = UninitCornerMap::new(input.num_corners);
    let mut num_points = 0usize;

    // The union of all seam sets, so classifying an edge is a single load
    // instead of a scan over every set.
    let mut union_seam: Vec<bool> = vec![false; input.num_corners];
    for s in seam_sets {
        for (u, &b) in union_seam.iter_mut().zip(s.iter()) {
            *u |= b;
        }
    }

    let nav = FanNav::new(input.pos_ct, input.num_corners);
    let mut crossed_edge: Vec<usize> = Vec::new();
    let mut is_point_boundary: Vec<bool> = Vec::new();
    let mut point_of: Vec<usize> = Vec::new();

    for_each_fan(input, &nav, |fan, closed| {
        let m = fan.len();
        // The sector-left-most corner of a fan no seam splits: the encoder's
        // walk starts one right of the position-left-most corner on a closed
        // fan (see `sector_start`), at the boundary corner on an open one.
        let unsplit_start = if closed && m > 1 { 1 } else { 0 };

        // Seam-free fan (the common case away from atlas cuts and creases):
        // one point, and one whole-fan vertex for every attribute.
        if fan.iter().all(|&c| !union_seam[usize::from(c.next())]) {
            let pt = PointIdx::from(num_points);
            num_points += 1;
            for &c in fan {
                // SAFETY: `fan` holds corner-table corners, all less than
                // `num_corners`.
                unsafe { corner_to_point.set(c, pt) };
            }
            for out in outputs.iter_mut() {
                out.point_to_vertex
                    .push(VertexIdx::from(out.vertex_to_left_most_corner.len()));
                out.vertex_to_left_most_corner.push(fan[unsplit_start]);
            }
            return;
        }

        crossed_edge.clear();
        crossed_edge.extend(fan.iter().map(|&c| usize::from(c.next())));
        is_point_boundary.clear();
        is_point_boundary.extend(crossed_edge.iter().map(|&e| union_seam[e]));

        // On an open fan every sector numbering starts at the
        // position-left-most corner itself.
        let union_start = if !closed {
            0
        } else {
            sector_start(|j| is_point_boundary[j], m)
        };

        // Union pass: assign point ids and the corner-to-point map.
        point_of.clear();
        point_of.resize(m, 0);
        let mut cur_pt = num_points;
        num_points += 1;
        for jj in 0..m {
            let idx = (union_start + jj) % m;
            if jj > 0 && is_point_boundary[idx] {
                cur_pt = num_points;
                num_points += 1;
            }
            point_of[idx] = cur_pt;
            // SAFETY: `fan` holds corner-table corners, all less than
            // `num_corners`.
            unsafe { corner_to_point.set(fan[idx], PointIdx::from(cur_pt)) };
        }

        // Attribute passes: number this attribute's sectors from its own start
        // and record the vertex once per point (point boundaries subsume every
        // attribute's seams, so the vertex is constant between them). An
        // attribute with no seam in this fan keeps one vertex across all of the
        // fan's points and skips the sector walk.
        for (k, out) in outputs.iter_mut().enumerate() {
            let seams = seam_sets[k];
            let vert = VertexIdx::from(out.vertex_to_left_most_corner.len());
            if !crossed_edge.iter().any(|&e| seams[e]) {
                out.vertex_to_left_most_corner.push(fan[unsplit_start]);
                out.point_to_vertex.resize(num_points, vert);
                continue;
            }
            let s = if !closed {
                0
            } else {
                sector_start(|j| seams[crossed_edge[j]], m)
            };
            out.point_to_vertex.resize(num_points, VertexIdx::INVALID);
            let mut vert = vert;
            out.vertex_to_left_most_corner.push(fan[s]);
            out.point_to_vertex[point_of[s]] = vert;
            for jj in 1..m {
                let idx = (s + jj) % m;
                if seams[crossed_edge[idx]] {
                    vert = VertexIdx::from(out.vertex_to_left_most_corner.len());
                    out.vertex_to_left_most_corner.push(fan[idx]);
                }
                if is_point_boundary[idx] {
                    out.point_to_vertex[point_of[idx]] = vert;
                }
            }
        }
    });

    // SAFETY: fans partition the corners and the union pass covers every corner
    // of each fan, so every entry is initialized.
    let corner_to_point = unsafe { corner_to_point.assume_init() };
    (corner_to_point, outputs)
}
