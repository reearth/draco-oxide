use crate::core::{corner_table::{attribute_corner_table::AttributeCornerTable, CornerTable, GenericCornerTable}, shared::CornerIdx};


/// All-inclusive corner table that contains the universal corner table and the attribute corner tables (if any).
/// This structure is constructed as a return value of the edgebreaker connectivity encoding, and will be passed to
/// the attribute encoder for read-access.
pub(crate) struct AllInclusiveCornerTable<'faces> {
    universal: CornerTable<'faces>,
    attribute_tables: Vec<AttributeCornerTable>,
}

impl<'faces> AllInclusiveCornerTable<'faces> {
    pub fn new(
        universal: CornerTable<'faces>,
        attribute_tables: Vec<AttributeCornerTable>,
    ) -> Self {
        Self {
            universal,
            attribute_tables,
        }
    }

    pub fn attribute_corner_table<'table>(
        &'table self,
        idx: usize,
    ) -> RefAttributeCornerTable<'faces, 'table> {
        assert!(idx < self.attribute_tables.len());
        RefAttributeCornerTable::new(idx, self)
    }
}


/// Reference to an attribute corner table. 
/// Mostly used to read-access the attribute corner table when encoding attributes.
pub(crate) struct RefAttributeCornerTable<'faces, 'table> {
    idx: usize,
    corner_table: &'table AllInclusiveCornerTable<'faces>,
}

impl<'faces, 'table> RefAttributeCornerTable<'faces, 'table> {
    pub fn new(
        idx: usize,
        corner_table: &'table AllInclusiveCornerTable<'faces>,
    ) -> Self {
        Self { idx, corner_table }
    }
}

impl<'faces, 'table> GenericCornerTable for RefAttributeCornerTable<'faces, 'table> {
    fn face_idx_containing(&self, corner: usize) -> usize {
        // The face index is the same as in the universal corner table
        self.corner_table.universal.face_idx_containing(corner)
    }

    fn num_faces(&self) -> usize {
        // number of faces is the same as the number of faces in the universal corner table
        self.corner_table.universal.num_faces()
    }
    
    fn num_corners(&self) -> usize {
        // number of corners is the same as the number of corners in the universal corner table
        self.corner_table.universal.num_corners()
    }
    fn num_vertices(&self) -> usize {
        self.corner_table.attribute_tables.get(self.idx).unwrap().num_vertices()
    }
    fn vertex_idx(&self, corner: usize) -> crate::core::shared::VertexIdx {
        self.corner_table.attribute_tables.get(self.idx).unwrap().vertex_idx(corner)
    }
    fn next(&self, c: CornerIdx) -> CornerIdx {
        self.corner_table.attribute_tables.get(self.idx).unwrap().next(c, &self.corner_table.universal)
    }
    fn previous(&self, c: CornerIdx) -> CornerIdx {
        self.corner_table.attribute_tables.get(self.idx).unwrap().previous(c, &self.corner_table.universal)
    }
    fn opposite(&self, c: CornerIdx) -> Option<CornerIdx> {
        self.corner_table.attribute_tables.get(self.idx).unwrap().opposite(c, &self.corner_table.universal)
    }
    fn left_most_corner(&self, vertex: crate::core::shared::VertexIdx) -> CornerIdx {
        self.corner_table.attribute_tables.get(self.idx).unwrap().left_most_corner(vertex)
    }
}
