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
        loop {
            let next_c = self.corner_table.swing_right(c);
            if next_c.is_none() {
                // Reached the open (right) boundary of the fan.
                break;
            }
            if next_c == start {
                // Swung all the way around a closed (interior) fan.
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

pub trait GenericCornerTable {
    fn opposite(&self, corner: CornerIdx) -> CornerIdx;

    fn swing_right(&self, corner: CornerIdx) -> CornerIdx {
        self.opposite(corner.previous()).previous()
    }

    fn swing_left(&self, corner: CornerIdx) -> CornerIdx {
        self.opposite(corner.next()).next()
    }

    fn get_left_corner(&self, corner: CornerIdx) -> Option<CornerIdx> {
        let c = self.opposite(corner.previous());
        if c.is_none() {
            None
        } else {
            Some(c)
        }
    }

    fn get_right_corner(&self, corner: CornerIdx) -> Option<CornerIdx> {
        let c = self.opposite(corner.next());
        if c.is_none() {
            None
        } else {
            Some(c)
        }
    }
}

#[derive(Debug, Clone)]
pub struct CornerTable(VecCornerIdx<CornerIdx>);

impl CornerTable {
    #[inline]
    pub fn first_corner(face_idx: FaceIdx) -> CornerIdx {
        CornerIdx::from(usize::from(face_idx) * 3)
    }

    pub fn from_raw_data(opposite_corners: VecCornerIdx<CornerIdx>) -> Self {
        Self(opposite_corners)
    }
}

impl GenericCornerTable for CornerTable {
    #[inline]
    fn opposite(&self, corner: CornerIdx) -> CornerIdx {
        self.0[corner]
    }
}

#[derive(Debug, Clone)]
pub struct AttributeCornerTable<'pos_ct> {
    pos_corner_table: &'pos_ct CornerTable,
    is_edge_on_seam: VecCornerIdx<bool>,
}

impl<'pos_ct> GenericCornerTable for AttributeCornerTable<'pos_ct> {
    fn opposite(&self, c: CornerIdx) -> CornerIdx {
        if self.is_corner_opposite_to_seam_edge(c) {
            CornerIdx::none()
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

    pub fn pos_corner_table(&self) -> &CornerTable {
        self.pos_corner_table
    }
}
