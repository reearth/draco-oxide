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
    fn num_vertices(&self) -> usize;
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
    fn num_vertices(&self) -> usize {
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

/// The identity attribute data structure: points coincide with position
/// vertices and the attribute is traversed over the base [`CornerTable`]. Valid
/// only when no attribute carries an interior seam, in which case every
/// attribute shares the position connectivity and the point/seam layer of
/// [`AttributeDS`] would be an identity map. It borrows the connectivity the
/// edgebreaker reconstruction already produced instead of rebuilding it.
pub struct IdentityDS<'a> {
    corner_table: &'a CornerTable,
    /// Per-corner position vertex (also the point, since points equal vertices).
    corner_to_vertex: &'a [VertexIdx],
    /// Left-most corner per vertex; every referenced vertex has one.
    vertex_corners: &'a [Option<CornerIdx>],
    num_vertices: usize,
    num_faces: usize,
    att: Attribute,
}

impl<'a> IdentityDS<'a> {
    pub fn new(
        corner_table: &'a CornerTable,
        corner_to_vertex: &'a [VertexIdx],
        vertex_corners: &'a [Option<CornerIdx>],
        num_vertices: usize,
        att: Attribute,
    ) -> Self {
        Self {
            corner_table,
            corner_to_vertex,
            vertex_corners,
            num_vertices,
            num_faces: corner_to_vertex.len() / 3,
            att,
        }
    }
}

impl<'a> GenericAttributeDs for IdentityDS<'a> {
    type Ct = CornerTable;

    #[inline]
    fn corner_table(&self) -> &CornerTable {
        self.corner_table
    }
    #[inline]
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        self.corner_to_vertex[usize::from(corner)]
    }
    #[inline]
    fn point_idx(&self, corner: CornerIdx) -> PointIdx {
        // Points coincide with vertices in the identity case.
        PointIdx::from(usize::from(self.corner_to_vertex[usize::from(corner)]))
    }
    #[inline]
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        self.vertex_corners[usize::from(vertex)].unwrap_or(CornerIdx::INVALID)
    }
    #[inline]
    fn num_vertices(&self) -> usize {
        self.num_vertices
    }
    #[inline]
    fn num_points(&self) -> usize {
        self.num_vertices
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
