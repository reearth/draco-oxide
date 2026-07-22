use crate::attribute::Attribute;
use crate::types::{
    CornerIdx, FaceIdx, PointIdx, VecCornerIdx, VecPointIdx, VecVertexIdx, VertexIdx,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Err {
    #[error("Corner table construction error: {0}")]
    ConstructionError(String),
}

#[derive(Debug, Clone)]
pub struct DS {
    corner_to_point_map: VecCornerIdx<PointIdx>,
    num_faces: usize,
    num_points: usize,
}

impl DS {
    pub fn new(corner_to_point_map: VecCornerIdx<PointIdx>) -> Self {
        let num_faces = corner_to_point_map.len() / 3;
        let num_points = corner_to_point_map
            .iter()
            .map(|p| usize::from(*p))
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        Self {
            corner_to_point_map,
            num_faces,
            num_points,
        }
    }

    pub fn point_idx(&self, corner: CornerIdx) -> PointIdx {
        self.corner_to_point_map[corner]
    }

    /// The corner-to-point map as a plain slice. Used by the identity data
    /// structure of the finest attribute, whose vertices coincide with the
    /// points, so this map is directly its corner-to-vertex map.
    pub fn corner_to_point(&self) -> &[PointIdx] {
        self.corner_to_point_map.as_slice()
    }

    pub fn num_faces(&self) -> usize {
        self.num_faces
    }

    pub fn num_corners(&self) -> usize {
        self.corner_to_point_map.len()
    }

    pub fn num_points(&self) -> usize {
        self.num_points
    }
}

#[derive(Debug, Clone)]
pub struct AttributeDS<'a> {
    global_ds: &'a DS,
    corner_table: AttributeCornerTable<'a>,
    vertex_to_left_most_corner_map: VecVertexIdx<CornerIdx>,
    point_to_vertex_map: VecPointIdx<VertexIdx>,
    att: Attribute,
}

impl<'a> AttributeDS<'a> {
    pub fn new(
        global_ds: &'a DS,
        corner_table: AttributeCornerTable<'a>,
        vertex_to_left_most_corner_map: VecVertexIdx<CornerIdx>,
        point_to_vertex_map: VecPointIdx<VertexIdx>,
        att: Attribute,
    ) -> Self {
        Self {
            global_ds,
            corner_table,
            vertex_to_left_most_corner_map,
            point_to_vertex_map,
            att,
        }
    }

    pub fn global_ds(&self) -> &DS {
        self.global_ds
    }

    pub fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        self.point_to_vertex_map[self.global_ds.corner_to_point_map[corner]]
    }

    pub fn point_idx(&self, corner: CornerIdx) -> PointIdx {
        self.global_ds.point_idx(corner)
    }

    pub fn num_points(&self) -> usize {
        self.global_ds.num_points()
    }

    pub fn num_faces(&self) -> usize {
        self.global_ds.num_faces()
    }

    pub fn num_corners(&self) -> usize {
        self.global_ds.num_corners()
    }

    pub fn num_vertices(&self) -> usize {
        self.vertex_to_left_most_corner_map.len()
    }

    pub fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        self.vertex_to_left_most_corner_map[vertex]
    }

    pub fn corner_table(&self) -> &AttributeCornerTable<'a> {
        &self.corner_table
    }

    pub fn is_on_boundary(&self, vertex: VertexIdx) -> bool {
        let left_most_corner = self.left_most_corner(vertex);
        self.corner_table.swing_left(left_most_corner).is_none()
    }

    /// Returns the valence (degree) of `vertex`, i.e. the number of edges
    /// incident to it, with respect to this attribute's connectivity
    /// (attribute seams break the one-ring, so a seam vertex is counted per
    /// connectivity fan).
    ///
    /// Starting from the left-most corner, it swings right around the vertex
    /// counting the incident edges. For an interior vertex the fan is closed,
    /// so the count equals the number of incident faces; for a boundary vertex
    /// the fan is open, so it is faces + 1.
    pub fn vertex_valence(&self, vertex: VertexIdx) -> usize {
        let start = self.left_most_corner(vertex);
        let mut c = start;
        let mut count = 2;
        // Swing right until the open (right) boundary of the fan, or all the
        // way around a closed (interior) fan.
        while let Some(next_c) = self.corner_table.swing_right(c) {
            if next_c == start {
                count -= 1;
                break;
            }
            count += 1;
            c = next_c;
        }
        count
    }

    pub fn att_data(&self) -> &Attribute {
        &self.att
    }

    pub fn att_data_mut(&mut self) -> &mut Attribute {
        &mut self.att
    }
}

/// The read/write surface the traversal and prediction algorithms need from an
/// attribute data structure. Implemented by [`AttributeDS`] (the general
/// seam-aware structure) and [`IdentityDS`] (the point-equals-vertex fast path
/// used when no attribute carries an interior seam). Algorithms are generic over
/// this trait so the caller can dispatch a concrete implementation and avoid the
/// point/seam layer entirely when it is an identity.
pub trait GenericAttributeDs {
    /// The connectivity this attribute is traversed over. [`CornerTable`] for the
    /// identity case, [`AttributeCornerTable`] when seams split the fans.
    type Ct: GenericCornerTable;

    fn corner_table(&self) -> &Self::Ct;
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx;
    fn point_idx(&self, corner: CornerIdx) -> PointIdx;
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx;
    /// The size of the vertex index space, i.e. an exclusive upper bound for
    /// every value [`Self::vertex_idx`] can return. This is what per-vertex
    /// working arrays must be sized by. It equals the number of referenced
    /// vertices for a compactly numbered structure, but may exceed it when the
    /// numbering includes phantom (unreferenced) vertex ids.
    fn vertex_index_bound(&self) -> usize;
    fn num_points(&self) -> usize;
    fn num_faces(&self) -> usize;
    fn num_corners(&self) -> usize;
    fn att_data(&self) -> &Attribute;
    fn att_data_mut(&mut self) -> &mut Attribute;

    /// Whether this attribute's connectivity differs from the position
    /// connectivity, i.e. some interior edge is a seam. Always false for the
    /// identity case, where the attribute rides the position corner table.
    fn has_interior_seams(&self) -> bool {
        false
    }

    /// Whether `vertex` lies on an open (boundary) fan, i.e. swinging left from
    /// its left-most corner reaches the fan's boundary.
    fn is_on_boundary(&self, vertex: VertexIdx) -> bool {
        let left_most_corner = self.left_most_corner(vertex);
        self.corner_table().swing_left(left_most_corner).is_none()
    }

    /// The valence (incident-edge count) of `vertex` in this attribute's
    /// connectivity. See [`AttributeDS::vertex_valence`] for the fan-walk
    /// reasoning; seams break the one-ring, so a seam vertex is counted per fan.
    fn vertex_valence(&self, vertex: VertexIdx) -> usize {
        let start = self.left_most_corner(vertex);
        let mut c = start;
        let mut count = 2;
        while let Some(next_c) = self.corner_table().swing_right(c) {
            if next_c == start {
                count -= 1;
                break;
            }
            count += 1;
            c = next_c;
        }
        count
    }
}

impl<'a> GenericAttributeDs for AttributeDS<'a> {
    type Ct = AttributeCornerTable<'a>;

    #[inline]
    fn corner_table(&self) -> &AttributeCornerTable<'a> {
        AttributeDS::corner_table(self)
    }
    #[inline]
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        AttributeDS::vertex_idx(self, corner)
    }
    #[inline]
    fn point_idx(&self, corner: CornerIdx) -> PointIdx {
        AttributeDS::point_idx(self, corner)
    }
    #[inline]
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        AttributeDS::left_most_corner(self, vertex)
    }
    #[inline]
    fn vertex_index_bound(&self) -> usize {
        AttributeDS::num_vertices(self)
    }
    #[inline]
    fn num_points(&self) -> usize {
        AttributeDS::num_points(self)
    }
    #[inline]
    fn num_faces(&self) -> usize {
        AttributeDS::num_faces(self)
    }
    #[inline]
    fn num_corners(&self) -> usize {
        AttributeDS::num_corners(self)
    }
    #[inline]
    fn att_data(&self) -> &Attribute {
        AttributeDS::att_data(self)
    }
    #[inline]
    fn att_data_mut(&mut self) -> &mut Attribute {
        AttributeDS::att_data_mut(self)
    }
    #[inline]
    fn has_interior_seams(&self) -> bool {
        self.corner_table.has_interior_seams()
    }
    #[inline]
    fn is_on_boundary(&self, vertex: VertexIdx) -> bool {
        AttributeDS::is_on_boundary(self, vertex)
    }
    #[inline]
    fn vertex_valence(&self, vertex: VertexIdx) -> usize {
        AttributeDS::vertex_valence(self, vertex)
    }
}

/// Per-vertex left-most corner. The edgebreaker reconstruction already holds
/// these as seeds (phantom vertices are `None`), so the seamless case borrows
/// them; the point-fan builder produces a fresh, phantom-free vector with no
/// longer-lived owner, so the finest case takes ownership.
enum LeftMost<'a> {
    Borrowed(&'a [Option<CornerIdx>]),
    Owned(Vec<CornerIdx>),
}

impl LeftMost<'_> {
    #[inline]
    fn get(&self, vertex: usize) -> CornerIdx {
        match self {
            LeftMost::Borrowed(s) => s[vertex].unwrap_or(CornerIdx::INVALID),
            LeftMost::Owned(s) => s[vertex],
        }
    }
}

/// The identity attribute data structure: points coincide with vertices, so
/// [`Self::vertex_idx`] is a single direct load with no point-to-vertex
/// composition. It is valid for any attribute whose vertices equal the points
/// it is traversed over:
///
/// - a fully seamless mesh, where every attribute rides the position
///   [`CornerTable`] and the points are the position vertices (`CT =
///   &CornerTable`, `V = VertexIdx`); or
/// - the finest attribute of a seamed mesh, whose own seams generate the whole
///   point refinement so its vertices are the points, traversed over its
///   [`AttributeCornerTable`] (`CT = AttributeCornerTable`, `V = PointIdx`, the
///   corner-to-vertex map borrowed from [`DS::corner_to_point`]).
///
/// It borrows the corner-to-vertex and left-most-corner maps its source already
/// produced instead of rebuilding them.
pub struct IdentityDS<'a, CT, V> {
    corner_table: CT,
    corner_to_vertex: &'a [V],
    left_most: LeftMost<'a>,
    /// Size of the vertex (== point) index space; may include phantom ids.
    index_bound: usize,
    num_faces: usize,
    /// Whether this attribute's connectivity carries an interior seam. False
    /// for the seamless case, true for a seamed mesh's finest attribute.
    interior_seams: bool,
    att: Attribute,
}

impl<'a> IdentityDS<'a, &'a CornerTable, VertexIdx> {
    /// The identity structure for a fully seamless mesh, borrowing the maps the
    /// reconstruction produced. `vertex_index_bound` is the reconstruction's
    /// allocated vertex count, which may include phantom (isolated) vertices.
    pub fn seamless(
        corner_table: &'a CornerTable,
        corner_to_vertex: &'a [VertexIdx],
        vertex_corners: &'a [Option<CornerIdx>],
        vertex_index_bound: usize,
        att: Attribute,
    ) -> Self {
        Self {
            corner_table,
            corner_to_vertex,
            left_most: LeftMost::Borrowed(vertex_corners),
            index_bound: vertex_index_bound,
            num_faces: corner_to_vertex.len() / 3,
            interior_seams: false,
            att,
        }
    }
}

impl<'a> IdentityDS<'a, AttributeCornerTable<'a>, PointIdx> {
    /// The identity structure for the finest attribute of a seamed mesh. Its
    /// vertices are the points, so its corner-to-vertex map is
    /// [`DS::corner_to_point`]. The caller must have verified this attribute is
    /// finest, i.e. its vertex count equals `ds.num_points()`.
    pub fn finest(
        ds: &'a DS,
        corner_table: AttributeCornerTable<'a>,
        vertex_to_left_most_corner: Vec<CornerIdx>,
        att: Attribute,
    ) -> Self {
        Self {
            corner_table,
            corner_to_vertex: ds.corner_to_point(),
            left_most: LeftMost::Owned(vertex_to_left_most_corner),
            index_bound: ds.num_points(),
            num_faces: ds.num_faces(),
            interior_seams: true,
            att,
        }
    }
}

impl<'a, CT, V> GenericAttributeDs for IdentityDS<'a, CT, V>
where
    CT: GenericCornerTable,
    V: Copy + Into<usize>,
{
    type Ct = CT;

    #[inline]
    fn corner_table(&self) -> &CT {
        &self.corner_table
    }
    #[inline]
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        let v: usize = self.corner_to_vertex[usize::from(corner)].into();
        VertexIdx::from(v)
    }
    #[inline]
    fn point_idx(&self, corner: CornerIdx) -> PointIdx {
        // Points coincide with vertices in the identity case.
        let v: usize = self.corner_to_vertex[usize::from(corner)].into();
        PointIdx::from(v)
    }
    #[inline]
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        self.left_most.get(usize::from(vertex))
    }
    #[inline]
    fn vertex_index_bound(&self) -> usize {
        self.index_bound
    }
    #[inline]
    fn num_points(&self) -> usize {
        self.index_bound
    }
    #[inline]
    fn num_faces(&self) -> usize {
        self.num_faces
    }
    #[inline]
    fn num_corners(&self) -> usize {
        self.corner_to_vertex.len()
    }
    #[inline]
    fn att_data(&self) -> &Attribute {
        &self.att
    }
    #[inline]
    fn att_data_mut(&mut self) -> &mut Attribute {
        &mut self.att
    }
    #[inline]
    fn has_interior_seams(&self) -> bool {
        self.interior_seams
    }
}

pub trait GenericCornerTable {
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx>;

    fn swing_right(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(corner.previous()).map(CornerIdx::previous)
    }

    fn swing_left(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(corner.next()).map(CornerIdx::next)
    }

    fn get_left_corner(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(corner.previous())
    }

    fn get_right_corner(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(corner.next())
    }

    /// Same as [`Self::swing_right`], but takes the corner's face index.
    /// `face` must equal `corner.face_idx()`.
    fn swing_right_with_face_idx(&self, corner: CornerIdx, face: FaceIdx) -> Option<CornerIdx> {
        self.opposite(corner.previous_with_face_idx(face))
            .map(CornerIdx::previous)
    }

    /// Same as [`Self::swing_left`], but takes the corner's face index.
    /// `face` must equal `corner.face_idx()`.
    fn swing_left_with_face_idx(&self, corner: CornerIdx, face: FaceIdx) -> Option<CornerIdx> {
        self.opposite(corner.next_with_face_idx(face))
            .map(CornerIdx::next)
    }

    /// Same as [`Self::get_left_corner`], but takes the corner's face index.
    /// `face` must equal `corner.face_idx()`.
    fn get_left_corner_with_face_idx(&self, corner: CornerIdx, face: FaceIdx) -> Option<CornerIdx> {
        self.opposite(corner.previous_with_face_idx(face))
    }

    /// Same as [`Self::get_right_corner`], but takes the corner's face index.
    /// `face` must equal `corner.face_idx()`.
    fn get_right_corner_with_face_idx(
        &self,
        corner: CornerIdx,
        face: FaceIdx,
    ) -> Option<CornerIdx> {
        self.opposite(corner.next_with_face_idx(face))
    }
}

/// Lets a shared reference stand in as a corner table, so a data structure
/// generic over its corner table type can borrow one (e.g. the position
/// [`CornerTable`]) instead of owning it.
impl<T: GenericCornerTable + ?Sized> GenericCornerTable for &T {
    #[inline]
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx> {
        (**self).opposite(corner)
    }
}

/// Per-corner opposite corners, stored compactly: a boundary edge is kept as an
/// internal sentinel and surfaces as `None` through [`GenericCornerTable`].
#[derive(Debug, Clone)]
pub struct CornerTable(VecCornerIdx<CornerIdx>);

impl CornerTable {
    #[inline]
    pub fn first_corner(face_idx: FaceIdx) -> CornerIdx {
        CornerIdx::from(usize::from(face_idx) * 3)
    }

    pub fn from_opposites(opposite_corners: Vec<Option<CornerIdx>>) -> Self {
        Self(
            opposite_corners
                .into_iter()
                .map(|opp| opp.unwrap_or(CornerIdx::INVALID))
                .collect::<Vec<_>>()
                .into(),
        )
    }
}

impl GenericCornerTable for CornerTable {
    #[inline]
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx> {
        let opp = self.0[corner];
        (opp != CornerIdx::INVALID).then_some(opp)
    }
}

#[derive(Debug, Clone)]
pub struct AttributeCornerTable<'pos_ct> {
    pos_corner_table: &'pos_ct CornerTable,
    is_edge_on_seam: VecCornerIdx<bool>,
}

impl<'pos_ct> GenericCornerTable for AttributeCornerTable<'pos_ct> {
    #[inline]
    fn opposite(&self, c: CornerIdx) -> Option<CornerIdx> {
        if self.is_corner_opposite_to_seam_edge(c) {
            None
        } else {
            self.pos_corner_table.opposite(c)
        }
    }
}

impl<'pos_ct> AttributeCornerTable<'pos_ct> {
    pub fn new(
        pos_corner_table: &'pos_ct CornerTable,
        is_edge_on_seam: VecCornerIdx<bool>,
    ) -> Self {
        Self {
            pos_corner_table,
            is_edge_on_seam,
        }
    }

    pub fn is_corner_opposite_to_seam_edge(&self, corner: CornerIdx) -> bool {
        self.is_edge_on_seam[corner]
    }

    /// True if any interior (non-boundary) edge is a seam, i.e. this
    /// attribute's connectivity differs from the position connectivity.
    /// Boundary edges are always seams and do not count.
    pub fn has_interior_seams(&self) -> bool {
        (0..self.is_edge_on_seam.len()).any(|c| {
            let c = CornerIdx::from(c);
            self.is_edge_on_seam[c] && self.pos_corner_table.opposite(c).is_some()
        })
    }

    pub fn pos_corner_table(&self) -> &CornerTable {
        self.pos_corner_table
    }
}
