//! Point-id assignment and per-attribute vertex maps, derived from a single
//! walk over the position fans shared by every attribute.

use std::mem::{ManuallyDrop, MaybeUninit};

use draco_oxide_core::mesh::ds::{CornerTable, GenericCornerTable};
use draco_oxide_core::types::{CornerIdx, PointIdx, VecCornerIdx, VertexIdx};

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
pub(crate) struct FanVertices {
    pub point_to_vertex: Vec<VertexIdx>,
    pub vertex_to_left_most_corner: Vec<CornerIdx>,
}

impl FanVertices {
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

/// Splits every position fan into points (the sectors of the union of all
/// seams, the decoder-side equivalent of Google's `AssignPointsToCorners`) and,
/// per attribute, into that attribute's seam-separated sectors. Returns the
/// corner-to-point map and, parallel to `seam_sets`, each attribute's
/// [`FanVertices`].
///
/// Each fan's corners are reached once through the corner table (see
/// [`for_each_fan`]); the point assignment and every attribute's vertex
/// numbering are then derived from the buffered sequence with plain array
/// reads, reproducing the per-attribute walk order exactly: sectors are
/// numbered from the sector-left-most corner rightward.
///
/// Points and vertices are numbered in fan-visit order, which is not ascending
/// corner order. Numbering is internal to the decode: values are placed by
/// traversal rank, and the traversal is driven by corner adjacency, so any
/// consistent numbering yields the same geometry.
pub(crate) fn fan_vertices(
    pos_ct: &CornerTable,
    seam_sets: &[&[bool]],
    num_corners: usize,
) -> (VecCornerIdx<PointIdx>, Vec<FanVertices>) {
    // Boundary edges are seams for every attribute and never split a fan, so
    // only interior seams route an attribute off the shared fast paths.
    let mut seamed_indices = seam_sets
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.iter()
                .enumerate()
                .any(|(c, &b)| b && pos_ct.opposite(CornerIdx::from(c)).is_some())
        })
        .map(|(i, _)| i);
    match (seamed_indices.next(), seamed_indices.next()) {
        (None, _) => {
            let (corner_to_point, fv) = fan_vertices_seamless(pos_ct, num_corners);
            (corner_to_point, vec![fv; seam_sets.len()])
        }
        (Some(k), None) => {
            let (corner_to_point, seamless, seamed) =
                fan_vertices_one_seamed(pos_ct, seam_sets[k], num_corners);
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
        _ => fan_vertices_general(pos_ct, seam_sets, num_corners),
    }
}

/// Precomputed fan navigation: where a right swing lands from each corner, and
/// the entry point of every open fan.
///
/// Both come from one sequential scan of `opposite`. Because
/// `swing_right(c) = opposite(c.previous()).map(previous)`, the load of
/// `opposite(c)` supplies `swing_right(c.next())`, and where that load is
/// `None` the edge is a boundary: `c.previous()` is then the left-most corner
/// of its fan and `c.next()` the right-most.
///
/// Resolving the swings up front keeps the `%3` in `next`/`previous` off the
/// walk's dependent load chain, where each step would otherwise wait on the
/// previous one.
struct FanNav {
    swing_right: VecCornerIdx<CornerIdx>,
    open_fan_seeds: Vec<CornerIdx>,
}

impl FanNav {
    fn new(pos_ct: &CornerTable, num_corners: usize) -> Self {
        let mut swing_right = UninitCornerMap::new(num_corners);
        let mut open_fan_seeds = Vec::new();
        for c in 0..num_corners {
            let c = CornerIdx::from(c);
            let target = c.next();
            match pos_ct.opposite(c) {
                // SAFETY: `next` is a permutation of the corners, so `target`
                // is in range and each slot is written exactly once.
                Some(o) => unsafe { swing_right.set(target, o.previous()) },
                None => {
                    unsafe { swing_right.set(target, CornerIdx::INVALID) };
                    open_fan_seeds.push(c.previous());
                }
            }
        }
        Self {
            // SAFETY: `next` covers every corner, so every slot was written.
            swing_right: unsafe { swing_right.assume_init() },
            open_fan_seeds,
        }
    }

    #[inline]
    fn swing_right(&self, c: CornerIdx) -> CornerIdx {
        // SAFETY: `c` is a corner of the table, so it is less than the length.
        unsafe { *self.swing_right.get_unchecked(c) }
    }
}

/// Calls `f` once per position fan with the fan's corners in right-sweep order
/// from its position-left-most corner, and whether the fan is closed.
///
/// Open fans come first, entered directly at the left-most corner recorded by
/// [`FanNav`], so no walk ever swings left. Every remaining corner then belongs
/// to a closed fan, where a right swing always lands and the sweep ends by
/// returning to where it began.
fn for_each_fan(nav: &FanNav, num_corners: usize, mut f: impl FnMut(&[CornerIdx], bool)) {
    let mut visited: VecCornerIdx<bool> = vec![false; num_corners].into();
    let mut fan: Vec<CornerIdx> = Vec::new();
    // SAFETY: every corner swept comes from the corner table, so it is less
    // than `visited.len()`.
    macro_rules! sweep {
        ($from:expr, $stop:expr) => {{
            fan.clear();
            let mut c = $from;
            loop {
                fan.push(c);
                unsafe { *visited.get_unchecked_mut(c) = true };
                let r = nav.swing_right(c);
                if r == $stop {
                    break;
                }
                c = r;
            }
        }};
    }

    for &seed in &nav.open_fan_seeds {
        sweep!(seed, CornerIdx::INVALID);
        f(&fan, false);
    }

    for start in 0..num_corners {
        let start = CornerIdx::from(start);
        if visited[start] {
            continue;
        }
        // A closed fan's position-left-most corner is the one whose left swing
        // is `start`, i.e. `swing_right(start)`.
        let pos_left_most = nav.swing_right(start);
        sweep!(pos_left_most, pos_left_most);
        f(&fan, true);
    }
}

/// Fast path when no attribute carries a seam: every fan is a single sector,
/// points coincide with position vertices, and all attributes share the same
/// vertex numbering.
fn fan_vertices_seamless(
    pos_ct: &CornerTable,
    num_corners: usize,
) -> (VecCornerIdx<PointIdx>, FanVertices) {
    let nav = FanNav::new(pos_ct, num_corners);
    let mut corner_to_point = UninitCornerMap::new(num_corners);
    let mut out = FanVertices::new();

    for_each_fan(&nav, num_corners, |fan, closed| {
        let pt = PointIdx::from(out.vertex_to_left_most_corner.len());
        for &c in fan {
            // SAFETY: `fan` holds corner-table corners, all less than
            // `num_corners`.
            unsafe { corner_to_point.set(c, pt) };
        }

        // A closed seamless fan numbers its single sector from the right
        // neighbor of the position-left-most corner, matching the encoder.
        let left_most = if closed { fan[1 % fan.len()] } else { fan[0] };

        out.vertex_to_left_most_corner.push(left_most);
        out.point_to_vertex.push(VertexIdx::from(usize::from(pt)));
    });

    // SAFETY: fans partition the corners and every corner of a fan is written
    // above, so every entry is initialized.
    (unsafe { corner_to_point.assume_init() }, out)
}

/// Fast path when exactly one attribute carries seams: the union of all seams
/// equals that attribute's seams, so points coincide with its vertices, and
/// every seamless attribute keeps one vertex per fan.
fn fan_vertices_one_seamed(
    pos_ct: &CornerTable,
    seams: &[bool],
    num_corners: usize,
) -> (VecCornerIdx<PointIdx>, FanVertices, FanVertices) {
    let mut corner_to_point = UninitCornerMap::new(num_corners);
    let mut seamless = FanVertices::new();
    let mut seamed = FanVertices::new();
    let mut num_points = 0usize;

    let nav = FanNav::new(pos_ct, num_corners);

    for_each_fan(&nav, num_corners, |fan, closed| {
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
    pos_ct: &CornerTable,
    seam_sets: &[&[bool]],
    num_corners: usize,
) -> (VecCornerIdx<PointIdx>, Vec<FanVertices>) {
    let num_outputs = seam_sets.len();
    let mut outputs: Vec<FanVertices> = (0..num_outputs).map(|_| FanVertices::new()).collect();
    let mut corner_to_point = UninitCornerMap::new(num_corners);
    let mut num_points = 0usize;

    let nav = FanNav::new(pos_ct, num_corners);
    // Scratch reused across fans: per corner the edge index consulted for a
    // seam crossing (the edge swung across to reach it); whether that edge is a
    // seam of any attribute (a point boundary); the fan-local point ids; and
    // each output's start index.
    let mut crossed_edge: Vec<usize> = Vec::new();
    let mut is_point_boundary: Vec<bool> = Vec::new();
    let mut point_of: Vec<usize> = Vec::new();
    let mut starts: Vec<usize> = vec![0; num_outputs];

    for_each_fan(&nav, num_corners, |fan, closed| {
        let m = fan.len();
        crossed_edge.clear();
        crossed_edge.extend(fan.iter().map(|&c| usize::from(c.next())));
        is_point_boundary.clear();
        is_point_boundary.extend(crossed_edge.iter().map(|&e| seam_sets.iter().any(|s| s[e])));

        // On an open fan every sector numbering starts at the
        // position-left-most corner itself.
        let union_start = if !closed {
            starts.iter_mut().for_each(|s| *s = 0);
            0
        } else {
            for (k, s) in starts.iter_mut().enumerate() {
                *s = sector_start(|j| seam_sets[k][crossed_edge[j]], m);
            }
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
        for out in outputs.iter_mut() {
            out.point_to_vertex.resize(num_points, VertexIdx::INVALID);
        }

        // Attribute passes: number this attribute's sectors from its own start
        // and record the vertex once per point (point boundaries subsume every
        // attribute's seams, so the vertex is constant between them).
        for (k, out) in outputs.iter_mut().enumerate() {
            let seams = seam_sets[k];
            let s = starts[k];
            let mut vert = VertexIdx::from(out.vertex_to_left_most_corner.len());
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
