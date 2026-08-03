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

/// The point space: the per-corner point map.
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

    /// The corner-to-point map as a plain slice.
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

/// The seam-aware attribute data structure over the shared point space.
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

    #[inline]
    /// Whether `vertex` lies on an open fan.
    pub fn is_on_boundary(&self, vertex: VertexIdx) -> bool {
        let left_most_corner = self.left_most_corner(vertex);
        self.corner_table.swing_left(left_most_corner).is_none()
    }

    /// The valence of `vertex` in this attribute's connectivity; a boundary fan
    /// counts faces + 1.
    pub fn vertex_valence(&self, vertex: VertexIdx) -> usize {
        let start = self.left_most_corner(vertex);
        let mut c = start;
        let mut count = 2;
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

/// The surface the traversal and prediction algorithms need from an
/// attribute data structure.
pub trait GenericAttributeDs {
    type Ct: GenericCornerTable;

    fn corner_table(&self) -> &Self::Ct;
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx;
    fn point_idx(&self, corner: CornerIdx) -> PointIdx;
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx;
    /// Exclusive upper bound of `vertex_idx`; may exceed the referenced count
    /// when the numbering carries phantom ids.
    fn vertex_index_bound(&self) -> usize;
    fn num_points(&self) -> usize;
    fn num_faces(&self) -> usize;
    fn num_corners(&self) -> usize;
    fn att_data(&self) -> &Attribute;
    fn att_data_mut(&mut self) -> &mut Attribute;

    /// The number of vertices referenced by some corner.
    fn num_referenced_vertices(&self) -> usize {
        let mut seen = vec![false; self.vertex_index_bound()];
        let mut count = 0;
        for c in 0..self.num_corners() {
            let v = usize::from(self.vertex_idx(CornerIdx::from(c)));
            if !seen[v] {
                seen[v] = true;
                count += 1;
            }
        }
        count
    }

    #[inline]
    fn is_on_boundary(&self, vertex: VertexIdx) -> bool {
        let left_most_corner = self.left_most_corner(vertex);
        self.corner_table().swing_left(left_most_corner).is_none()
    }

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
    fn num_referenced_vertices(&self) -> usize {
        AttributeDS::num_vertices(self)
    }
    #[inline]
    /// Whether `vertex` lies on an open fan.
    fn is_on_boundary(&self, vertex: VertexIdx) -> bool {
        AttributeDS::is_on_boundary(self, vertex)
    }
    #[inline]
    /// The valence of `vertex` in this attribute's connectivity.
    fn vertex_valence(&self, vertex: VertexIdx) -> usize {
        AttributeDS::vertex_valence(self, vertex)
    }
}

/// The identity structure for a seamless mesh: points coincide with position
/// vertices, borrowing the reconstruction's maps.
pub struct IdentityDS<'a, CT, V> {
    corner_table: CT,
    corner_to_vertex: &'a [V],
    left_most: &'a [CornerIdx],
    index_bound: usize,
    num_faces: usize,
    att: Attribute,
}

impl<'a> IdentityDS<'a, &'a CornerTable, VertexIdx> {
    pub fn seamless(
        corner_table: &'a CornerTable,
        corner_to_vertex: &'a [VertexIdx],
        vertex_corners: &'a [CornerIdx],
        vertex_index_bound: usize,
        att: Attribute,
    ) -> Self {
        Self {
            corner_table,
            corner_to_vertex,
            left_most: vertex_corners,
            index_bound: vertex_index_bound,
            num_faces: corner_to_vertex.len() / 3,
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
        let v: usize = self.corner_to_vertex[usize::from(corner)].into();
        PointIdx::from(v)
    }
    #[inline]
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        self.left_most[usize::from(vertex)]
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
    fn num_referenced_vertices(&self) -> usize {
        (0..self.index_bound)
            .filter(|&v| self.left_most[v] != CornerIdx::INVALID)
            .count()
    }
}

/// The corner-table surface: the opposite relation and derived swings.
pub trait GenericCornerTable {
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx>;

    #[inline]
    fn swing_right(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(corner.previous()).map(CornerIdx::previous)
    }

    #[inline]
    fn swing_left(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(corner.next()).map(CornerIdx::next)
    }

    #[inline]
    fn get_left_corner(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(corner.previous())
    }

    #[inline]
    fn get_right_corner(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(corner.next())
    }

    /// `face` must equal `corner.face_idx()` in the `_with_face_idx` variants.
    #[inline]
    fn swing_right_with_face_idx(&self, corner: CornerIdx, face: FaceIdx) -> Option<CornerIdx> {
        self.opposite(corner.previous_with_face_idx(face))
            .map(CornerIdx::previous)
    }

    #[inline]
    fn swing_left_with_face_idx(&self, corner: CornerIdx, face: FaceIdx) -> Option<CornerIdx> {
        self.opposite(corner.next_with_face_idx(face))
            .map(CornerIdx::next)
    }

    #[inline]
    fn get_left_corner_with_face_idx(&self, corner: CornerIdx, face: FaceIdx) -> Option<CornerIdx> {
        self.opposite(corner.previous_with_face_idx(face))
    }

    #[inline]
    fn get_right_corner_with_face_idx(
        &self,
        corner: CornerIdx,
        face: FaceIdx,
    ) -> Option<CornerIdx> {
        self.opposite(corner.next_with_face_idx(face))
    }
}

/// A shared reference stands in as a corner table.
impl<T: GenericCornerTable + ?Sized> GenericCornerTable for &T {
    #[inline]
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx> {
        (**self).opposite(corner)
    }
}

/// Per-corner opposite corners; boundary edges hold an internal sentinel.
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

    /// Takes an opposite array already using the `INVALID` sentinel as-is.
    pub fn from_opposite_sentinels(opposite_corners: Vec<CornerIdx>) -> Self {
        Self(opposite_corners.into())
    }
}

impl GenericCornerTable for CornerTable {
    #[inline]
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx> {
        let opp = self.0[corner];
        (opp != CornerIdx::INVALID).then_some(opp)
    }
}

/// The position corner table cut by attribute seams.
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

    /// True if any interior edge is a seam.
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
