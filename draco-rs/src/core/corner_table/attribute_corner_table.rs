use crate::{core::{corner_table::CornerTable, shared::{CornerIdx, VertexIdx}}, prelude::Attribute};
use crate::core::corner_table::GenericCornerTable;

#[derive(Debug, Clone)]
pub(crate) struct AttributeCornerTable {
    corner_to_vertex: Vec<VertexIdx>,
    vertex_to_attribute_entry: Vec<usize>,
    is_edge_on_seam: Vec<bool>,
    is_vertex_on_seam: Vec<bool>,
    left_most_corners: Vec<usize>,
    num_vertices: usize,
}

impl AttributeCornerTable {
    pub fn new(
        corner_table: &CornerTable,
        att: &Attribute,
    ) -> Self {
        let mut is_edge_on_seam = vec![false; corner_table.num_corners()];
        let mut is_vertex_on_seam = vec!(false; corner_table.num_vertices());

        // Find all necessary data for encoding attributes. For now we check which of
        // the mesh vertices is part of an attribute seam, because seams require
        // special handling.
        for c in 0..corner_table.num_corners() {
            let opp_corner = if let Some(opp_corner) = corner_table.opposite(c) {
                opp_corner
            } else {
                // Boundary. Mark it as seam edge.
                is_edge_on_seam[c] = true;
                // Mark seam vertices.
                let mut v = corner_table.vertex_idx(corner_table.next(c));
                is_vertex_on_seam[v] = true;
                v = corner_table.vertex_idx(corner_table.previous(c));
                is_vertex_on_seam[v] = true;
                continue;
            };
            if opp_corner < c {
                continue;  // Opposite corner was already processed.
            }

            let mut act_c = c;
            let mut act_sibling_c = opp_corner;
            for _ in 0..2 {
                // Get the sibling corners. I.e., the two corners attached to the same
                // vertex but divided by the seam edge.
                act_c = corner_table.next(act_c);
                act_sibling_c = corner_table.previous(act_sibling_c);
                let point_id = corner_table.vertex_idx(act_c);
                let sibling_point_id = corner_table.vertex_idx(act_sibling_c);
                if att.get_att_idx(point_id) != att.get_att_idx(sibling_point_id) {
                    is_edge_on_seam[c] = true;
                    is_edge_on_seam[opp_corner] = true;
                    // Mark seam vertices.
                    is_vertex_on_seam[corner_table.vertex_idx(corner_table.next(c))] = true;
                    is_vertex_on_seam[corner_table.vertex_idx(corner_table.previous(c))] = true;
                    is_vertex_on_seam[corner_table.vertex_idx(corner_table.next(opp_corner))] = true;
                    is_vertex_on_seam[corner_table.vertex_idx(corner_table.previous(opp_corner))] = true;
                    break;
                }
            }
        }

        let mut out = Self {
            corner_to_vertex: vec![0; corner_table.num_corners()],
            vertex_to_attribute_entry: Vec::new(),
            is_edge_on_seam,
            is_vertex_on_seam,
            left_most_corners: Vec::new(),
            num_vertices: corner_table.num_vertices(),
        };

        out.recompute_vertices(Some(att), corner_table);
        out
    }

    pub fn recompute_vertices(
        &mut self,
        att: Option<&Attribute>,
        corner_table: &CornerTable
    ) {
        if att.is_some() {
            self.recompute_vertices_impl::<true>(att, corner_table)
        } else {
            self.recompute_vertices_impl::<false>(att, corner_table)
        }
    }

    pub fn recompute_vertices_impl<const INIT_VERTEX_TO_ATTRIBUTE_ENTRIES: bool>(
        &mut self,
        att: Option<&Attribute>,
        corner_table: &CornerTable
    ) {
        self.vertex_to_attribute_entry.clear();
        self.left_most_corners.clear();
        let mut num_new_vertices = 0;

        for v in 0..corner_table.num_vertices() {
            let c = corner_table.left_most_corner(v);

            let first_vert_id = num_new_vertices;
            num_new_vertices += 1;
            if INIT_VERTEX_TO_ATTRIBUTE_ENTRIES {
                let att = att.unwrap(); // ToDo: This can even be unwrap_unchecked.
                let point_id = corner_table.vertex_idx(c);
                self.vertex_to_attribute_entry.push(att.get_att_idx(point_id));
            } else {
                // Identity mapping
                self.vertex_to_attribute_entry.push(first_vert_id);
            }
            let mut first_c = c;
            let mut maybe_act_c;
            // Check if the vertex is on a seam edge, if it is we need to find the first
            // attribute entry on the seam edge when traversing in the CCW direction.
            if self.is_vertex_on_seam[v] {
                // Try to swing left on the modified corner table. We need to get the
                // first corner that defines an attribute seam.
                maybe_act_c = self.swing_left(first_c, corner_table);
                while let Some(act_c) = maybe_act_c {
                    first_c = act_c;
                    if act_c == c {
                        // We have reached back to the same corner, which cannot happen when 'v' is on a seam edge.
                        unreachable!("Swinging left from the left most corner should never return the same corner.");
                    }
                    maybe_act_c = self.swing_left(act_c, corner_table);
                }
            }
            self.corner_to_vertex[first_c] = first_vert_id;
            self.left_most_corners.push(first_c);
            let mut maybe_act_c = corner_table.swing_right(first_c);
            while let Some(act_c) = maybe_act_c {
                if act_c == first_c {
                    break;
                }
                if self.is_corner_opposite_to_seam_edge(corner_table.next(act_c)) {
                    let first_vert_id = num_new_vertices;
                    num_new_vertices += 1;
                    if INIT_VERTEX_TO_ATTRIBUTE_ENTRIES {
                        let att = att.unwrap(); // ToDo: This can even be unwrap_unchecked.
                        let point_id = corner_table.vertex_idx(act_c);
                        self.vertex_to_attribute_entry.push(
                            att.get_att_idx(point_id));
                    } else {
                        // Identity mapping.
                        self.vertex_to_attribute_entry.push(first_vert_id);
                    }
                    self.left_most_corners.push(act_c);
                }
                self.corner_to_vertex[act_c] = first_vert_id;
                maybe_act_c = corner_table.swing_right(act_c);
            }
        }

        self.num_vertices = num_new_vertices;
    }

    pub(crate) fn vertex_idx(&self, c: usize) -> VertexIdx {
        self.corner_to_vertex[c]
    }

    pub(crate) fn num_vertices(&self) -> usize {
        self.num_vertices
    }

    pub(crate) fn next(&self, c: usize, corner_table: &CornerTable) -> usize {
        corner_table.next(c)
    }

    pub(crate) fn previous(&self, c: usize, corner_table: &CornerTable) -> usize {
        corner_table.previous(c)
    }

    pub(crate) fn opposite(&self, c: usize, corner_table: &CornerTable) -> Option<usize> {
        if self.is_corner_opposite_to_seam_edge(c) {
            None
        } else {
            corner_table.opposite(c)
        }
    }

    pub(crate) fn swing_right(&self, corner: usize, corner_table: &CornerTable) -> Option<usize> {
        if let Some(corner) = self.opposite(self.previous(corner, corner_table), corner_table) {
            Some(self.previous(corner, corner_table))
        } else {
            None
        }
    }

    pub(crate) fn swing_left(&self, corner: usize, corner_table: &CornerTable) -> Option<usize> {
        if let Some(corner) = self.opposite(self.next(corner, corner_table), corner_table) {
            Some(self.next(corner, corner_table))
        } else {
            None
        }
    }

    pub(crate) fn is_corner_opposite_to_seam_edge(&self, corner: usize) -> bool {
        self.is_edge_on_seam[corner]
    }

    pub(crate) fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        self.left_most_corners[vertex]
    }
}


#[cfg(test)]
#[test]
fn test_no_att_seam() {
    // read the test data from a corner table

    use crate::{io::obj::load_obj, prelude::AttributeType};
    let mesh = load_obj("tests/data/sphere.obj").unwrap();
    let faces = mesh.faces;

    let corner_table = CornerTable::new(&faces);
    let att = mesh.attributes.iter().find(|att| att.get_attribute_type()==AttributeType::Normal).unwrap();
    let attr_corner_table = AttributeCornerTable::new(&corner_table, att);
    assert_eq!(attr_corner_table.num_vertices(), corner_table.num_vertices());
    assert_eq!(attr_corner_table.corner_to_vertex.len(), corner_table.num_corners());
    assert_eq!(attr_corner_table.vertex_to_attribute_entry.len(), corner_table.num_vertices());
    assert_eq!(attr_corner_table.left_most_corners.len(), corner_table.num_vertices());
    assert_eq!(attr_corner_table.is_edge_on_seam.len(), corner_table.num_corners());
    assert_eq!(attr_corner_table.is_vertex_on_seam.len(), corner_table.num_vertices());
    assert!(attr_corner_table.is_edge_on_seam.iter().all(|&x| x == false));
    assert!(attr_corner_table.is_vertex_on_seam.iter().all(|&x| x == false));
    assert!(attr_corner_table.left_most_corners.iter().all(|&x| x < corner_table.num_corners()));
    assert!(attr_corner_table.corner_to_vertex.iter().all(|&x| x < corner_table.num_vertices()));

    // check the opprosite corners
    for c in 0..corner_table.num_corners() {
        assert_eq!(attr_corner_table.opposite(c, &corner_table), corner_table.opposite(c));
    }

    // check vertices
    for c in 0..corner_table.num_corners() {
        assert_eq!(attr_corner_table.vertex_idx(c), corner_table.vertex_idx(c), 
            "attr corner_to_vertex: {:?}\noriginal corner_to_vertex: {:?}", 
            attr_corner_table.corner_to_vertex, 
            corner_table.corner_to_vertex
        );
    }

    // no attribute seams, so all edges and vertices are not on a seam.
    attr_corner_table.is_edge_on_seam.iter().all(|&x| !x);
    attr_corner_table.is_vertex_on_seam.iter().all(|&x| !x);
}
    