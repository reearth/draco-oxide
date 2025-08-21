pub(crate) mod attribute_corner_table;
pub(crate) mod all_inclusive_corner_table;

use std::{collections::BTreeMap};

use crate::{core::shared::{AttributeValueIdx, CornerIdx, FaceIdx, PointIdx, VecCornerIdx, VecVertexIdx, VertexIdx}, prelude::Attribute};

pub(crate) trait GenericCornerTable {
    fn face_idx_containing(&self, corner: CornerIdx) -> FaceIdx;
    fn num_faces(&self) -> usize;
    fn num_corners(&self) -> usize;
    fn num_vertices(&self) -> usize;
    fn point_idx(&self, corner: CornerIdx) -> PointIdx;
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx;
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx>;
    fn previous(&self, corner: CornerIdx) -> CornerIdx;
    fn next(&self, corner: CornerIdx) -> CornerIdx;
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx;

    fn swing_right(&self, corner: CornerIdx) -> Option<CornerIdx> {
        if let Some(c) = self.opposite(self.previous(corner)){
            Some(self.previous(c))
        } else {
            None
        }
    }

    fn swing_left(&self, corner: CornerIdx) -> Option<CornerIdx> {
        if let Some(c) = self.opposite(self.next(corner)){
            Some(self.next(c))
        } else {
            None
        }
    }
    
    fn is_on_boundary(&self, v: VertexIdx) -> bool {
        self.swing_left(self.left_most_corner(v)).is_none()
    }
    
    fn get_left_corner(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(self.previous(corner))
    }
    
    fn get_right_corner(&self, corner: CornerIdx) -> Option<CornerIdx> {
        self.opposite(self.next(corner))
    }

    fn vertex_to_attribute_map(&self) -> Option<&VecVertexIdx<AttributeValueIdx>> {
        None
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CornerTable<'mesh> {
    /// Records the opposite corner for each corner.
    /// If a corner does not have an opposite corner, the value is 'CornerIdx::from(usize::MAX)'.
    opposite_corners: VecCornerIdx<CornerIdx>,

    /// faces of the mesh.
    mesh_faces: &'mesh [[PointIdx;3]],

    /// Faces of the POSITION ATTRIBUTE.
    /// This is different from the faces of the mesh.
    conn_faces: Vec<[VertexIdx;3]>,

    /// Number of corners in the mesh.
    num_corners: usize,

    // Number of vertices in the mesh.
    num_vertices: usize,

    /// Stores the left most corner for each vertex.
    left_most_corners: VecVertexIdx<CornerIdx>,

    /// corner to vertex map.
    corner_to_vertex: BTreeMap<CornerIdx, VertexIdx>,

    /// Stores the parents of non-manifold vertices.
    non_manifold_vertex_parents: Vec<VertexIdx>,
}

impl<'mesh> CornerTable<'mesh> {
    pub(crate) fn new(mesh_faces: &'mesh [[PointIdx;3]], pos_att: &Attribute) -> Self {
        let conn_faces = mesh_faces.iter()
            .map(|f| 
                [
                    usize::from(pos_att.get_unique_val_idx(f[0])).into(),
                    usize::from(pos_att.get_unique_val_idx(f[1])).into(),
                    usize::from(pos_att.get_unique_val_idx(f[2])).into()
                ]
            )
            .collect::<Vec<_>>();
        let mut out  = Self {
            opposite_corners: VecCornerIdx::new(), // will be computed later
            num_corners: mesh_faces.len() * 3,
            mesh_faces,
            conn_faces,
            num_vertices: 0, // will be computed later
            left_most_corners: VecVertexIdx::new(), // will be computed later
            corner_to_vertex: BTreeMap::new(), // will be computed later
            non_manifold_vertex_parents: Vec::new(), // will be computed later
        };

        let unused_vertices = Self::get_unused_vertices(&out.conn_faces);
        if !unused_vertices.is_empty() {
            panic!("Mesh contains unused vertices: {:?}. This is not supported by the corner table.", unused_vertices);
        }


        out.compute_table();
        if Self::contains_non_manifold_edges(&out.conn_faces) {
            out.handle_no_manifold_edges();
        }
        out.compute_left_most_corners();

        out
    }

    /// checks if the mesh has non-manifold edges.
    fn contains_non_manifold_edges(faces: &[[VertexIdx;3]]) -> bool {
        let mut edges = faces.iter()
            .flat_map(|f| {
                let v0 = f[0];
                let v1 = f[1];
                let v2 = f[2];
                vec![[v0, v1], [v1, v2], [v2, v0]]
            })
            .collect::<Vec<_>>();
        for e in &mut edges { e.sort(); }
        edges.sort();
        // count duplicates. If there is a triple of the same edge, it is non-manifold.
        let mut count = 1;
        for i in 1..edges.len() {
            if edges[i] == edges[i-1] {
                count += 1;
                if count > 2 {
                    return true; // found a non-manifold edge
                }
            } else {
                count = 1; // reset count
            }
        }
        false // no non-manifold edges found
    }

    /// Handles non-manifold edges by breaking the connectivity at them.
    /// Follows the draco's implementation.
    fn handle_no_manifold_edges(&mut self) {
        let mut visited_corners = vec![false; self.num_corners()];
        let mut sink_vertices: Vec<(VertexIdx, CornerIdx)> = Vec::new();
        let mut connectivity_updated;
        let default_opposite = CornerIdx::from(usize::MAX);
        loop {
            connectivity_updated = false;
            for c in 0..self.num_corners() {
                if visited_corners[c] {
                    continue;
                }
                let c = CornerIdx::from(c);
                sink_vertices.clear();

                // Swing all the way to find the lefft most corner, if any.
                let mut first_c = c;
                let mut curr_c = c;
                while let Some(next_c) = self.swing_left(curr_c) {
                    if next_c == first_c || visited_corners[usize::from(next_c)] {
                        break;
                    }
                    curr_c = next_c;
                }

                first_c = curr_c;

                // Check for the uniqueness by swinging right.
                loop {
                    visited_corners[usize::from(curr_c)] = true;
                    let sink_c = self.next(curr_c);
                    let sink_v = self.corner_to_vert(sink_c);

                    let edge_c = self.previous(curr_c);
                    let mut vertex_connectivity_updated = false;

                    for &attached_sink_vertex in &sink_vertices {
                        if attached_sink_vertex.0 == sink_v {
                            let other_edge_c = attached_sink_vertex.1;
                            let opp_edge_c = self.opposite(edge_c);

                            if let Some(opp_edge_c) = opp_edge_c {
                                if opp_edge_c == other_edge_c {
                                    continue;
                                }
                            }

                            let opp_other_edge_c = self.opposite(other_edge_c);
                            if let Some(opp_edge_c) = opp_edge_c {
                                self.opposite_corners[opp_edge_c] = default_opposite; // None
                            }
                            if let Some(opp_other_edge_c) = opp_other_edge_c {
                                self.opposite_corners[opp_other_edge_c] = default_opposite; // None
                            }

                            self.opposite_corners[edge_c] = default_opposite;
                            self.opposite_corners[other_edge_c] = default_opposite;

                            vertex_connectivity_updated = true;
                            break;
                        }
                    }
                    if vertex_connectivity_updated {
                        connectivity_updated = true;
                        break;
                    }
                    let new_sink_vert: (VertexIdx, CornerIdx) = (
                        self.corner_to_vert(self.previous(curr_c)),
                        sink_c
                    );
                    sink_vertices.push(new_sink_vert);

                    curr_c = if let Some(c) = self.swing_right(curr_c) {
                        c
                    } else {
                        break; // reached the end of the corner loop
                    };
                    if curr_c == first_c {
                        break; // reached back to the first corner
                    }
                }
            }
            if !connectivity_updated {
                break; // no more connectivity updates
            }
        }
    }

    fn get_unused_vertices(faces: &[[VertexIdx;3]]) -> Vec<usize> {
        let num_vertices = {
            let num_vertices_minus_1 = *faces.iter().flatten().max().unwrap_or(&VertexIdx::from(0));
            usize::from(num_vertices_minus_1) + 1
        };
        let mut used_vertices = vec![false; num_vertices];
        for f in faces {
            for &v in f {
                used_vertices[usize::from(v)] = true;
            }
        }
        used_vertices.iter().enumerate()
            .filter_map(|(idx, &used)| if !used { Some(idx) } else { None })
            .collect()
    }

    fn compute_table(&mut self) {
        let default_opposite = CornerIdx::from(usize::MAX);
        let default_vertex = VertexIdx::from(usize::MAX);
        self.opposite_corners.resize(self.num_corners(), default_opposite);
        let mut num_corners_on_vertices = VecVertexIdx::with_capacity(self.num_corners());

        // Compute the number of corners on each vertex.
        // The vertices are sorted by their earliest corner.
        for c in 0..self.num_corners() {
            let c = CornerIdx::from(c);
            let v1 = self.vertex_idx(c);
            if usize::from(v1) >= num_corners_on_vertices.len() {
                num_corners_on_vertices.resize(usize::from(v1) + 1, 0);
            }
            num_corners_on_vertices[v1]+=1;
        }

        // Array for storing half edges. (sink vertex, edge corner)
        let mut vertex_edges: Vec<(VertexIdx, CornerIdx)> = vec![ (default_vertex, default_opposite); self.num_corners() ];

        let mut offset = 0;
        // Compute the offset of the the earliest corner for each vertex.
        let vertex_offset: VecVertexIdx<_> = (0..num_corners_on_vertices.len()).map(|i| {
                let i = VertexIdx::from(i);
                let out = offset;
                offset += num_corners_on_vertices[i];
                out
            })
            .collect::<Vec<_>>()
            .into();

        for c in 0..self.num_corners() {
            let c = CornerIdx::from(c);
            let tip_v = self.vertex_idx(c);
            let source_v = self.vertex_idx(self.next(c));
            let sink_v = self.vertex_idx(self.previous(c));

            let f_idx = self.face_idx_containing(c);
            if c == Self::first_corner(f_idx) {
                let v0 = self.vertex_idx(c);
                if v0 == source_v || v0 == sink_v || source_v == sink_v {
                    continue; // skip degenerate corners
                }
            }

            let mut opposite_c = default_opposite;
            let num_corners_on_vert = num_corners_on_vertices[sink_v];
            offset = vertex_offset[sink_v];
            for i in 0..num_corners_on_vert {
                let other_v = vertex_edges[offset].0;
                if other_v == default_vertex {
                    break;
                }
                if other_v == source_v {
                    // opposite corner found.
                    // We need to remove the half edge from the vertex_edges.
                    if tip_v == self.vertex_idx(vertex_edges[offset].1) {
                        continue; 
                    }
                    opposite_c = vertex_edges[offset].1;
                    for _ in i+1..num_corners_on_vert {
                        vertex_edges[offset] = vertex_edges[offset + 1];
                        if vertex_edges[offset].0 == default_vertex {
                            break;
                        }
                        offset += 1;
                    }
                    vertex_edges[offset].0 = default_vertex;
                    break;
                }
                offset+=1;
            }
            if opposite_c == default_opposite {
                let num_corners_on_source_vert = num_corners_on_vertices[source_v];
                let first_c = vertex_offset[source_v];
                for corner in first_c..num_corners_on_source_vert+first_c {
                    if vertex_edges[corner].0 == default_vertex {
                        vertex_edges[corner].0 = sink_v;
                        vertex_edges[corner].1 = c;
                        break;
                    }
                }
            } else {
                self.opposite_corners[c] = opposite_c;
                self.opposite_corners[opposite_c] = c;
            }
        }
        self.num_vertices = num_corners_on_vertices.len();
    }

    fn compute_left_most_corners(&mut self) {
        let default_corner = CornerIdx::from(usize::MAX);
        self.left_most_corners.resize(self.num_vertices(), default_corner);
        let mut visited_vertices = VecVertexIdx::from(vec![false; self.num_vertices()]);
        let mut visited_corners = VecCornerIdx::from(vec![false; self.num_corners()]);

        {
            let mut vertices = VecVertexIdx::from( vec![false; self.num_vertices()] );
            for f_idx in 0..self.get_mesh_faces().len() {
                for i in 0..3 {
                    let c = CornerIdx::from(3*f_idx + i);
                    let v = self.vertex_idx(c);
                    if !vertices[v] {
                        vertices[v] = true;
                    }
                }
            }
        }

        for f_idx in 0..self.get_mesh_faces().len() {
            for i in 0..3 {
                let c = CornerIdx::from(3*f_idx + i);
                if visited_corners[c] { continue; }

                let mut v = self.vertex_idx(c);
                let mut is_non_manifold_vertex = false;
                if visited_vertices[v] {
                    // Coming here means the vertex has a neighborhood that is not connected when the vertex is removed,
                    // i.e. it is violating the manifold condition.
                    // We need to create a new vertex here to avoid this case.
                    self.left_most_corners.push(default_corner);
                    self.non_manifold_vertex_parents.push(v);
                    visited_vertices.push(false);
                    v = VertexIdx::from(self.num_vertices);
                    self.num_vertices += 1;
                    is_non_manifold_vertex = true;
                }
                visited_vertices[v] = true;
                visited_corners[c] = true;
                self.left_most_corners[v] = c;
                if is_non_manifold_vertex {
                    // Update vertex index in the corresponding face.
                    self.corner_to_vertex.insert(c, v);
                }

                // Swing all the way to the left
                let mut maybe_act_c= self.swing_left(c);
                while let Some(act_c) = maybe_act_c {
                    if act_c == c {
                        // Reached back to the initial corner.
                        break;  
                    }
                    visited_corners[act_c] = true;
                    self.left_most_corners[v] = act_c;
                    if is_non_manifold_vertex {
                        // Update vertex index in the corresponding face.
                        self.corner_to_vertex.insert(act_c, v);
                    }
                    maybe_act_c = self.swing_left(act_c);
                }
                
                if maybe_act_c.is_none() {
                    // if we have reached open boundary, we need to swing right to mark all corners
                    maybe_act_c = Some(c);
                    while let Some(act_c) = maybe_act_c {
                        visited_corners[act_c] = true;
                        if is_non_manifold_vertex {
                            self.corner_to_vertex.insert(act_c, v);
                        }
                        maybe_act_c = self.swing_right(act_c);
                    }
                }
            }
        }
    }

    #[inline]
    pub(crate) fn vertex_valence(&self, v: VertexIdx) -> usize {
        let c = self.left_most_corner(v);
        let mut count = 2;
        while let Some(next_c) = self.swing_right(c) {
            if next_c == c {
                count -= 1;
                break; // we have reached back to the initial corner
            }
            count += 1;
        }
        count
    }

    #[inline]
    pub(crate) fn first_corner(face_idx: FaceIdx) -> CornerIdx {
        CornerIdx::from(usize::from(face_idx) * 3)
    }

    #[inline]
    pub(crate) fn get_mesh_faces(&self) -> &[[PointIdx; 3]] {
        self.mesh_faces
    }

    #[inline]
    pub(crate) fn corner_to_vert(&self, corner: CornerIdx) -> VertexIdx {
        if let Some(v) = self.corner_to_vertex.get(&corner) {
            return *v
        };

        let corner = usize::from(corner);

        let local = corner % 3;
        let face_idx = corner / 3;

        match local {
            0 => self.conn_faces[face_idx][0],
            1 => self.conn_faces[face_idx][1],
            2 => self.conn_faces[face_idx][2],
            _ => unreachable!(), // it is safe to assume this as 'local' is the remainder of a number divided by 3.
        }
    }
}

impl<'mesh> GenericCornerTable for CornerTable<'mesh> {
    #[inline]
    fn face_idx_containing(&self, corner: CornerIdx) -> FaceIdx {
        (usize::from(corner) / 3).into()
    }

    #[inline]
    fn num_faces(&self) -> usize {
        self.get_mesh_faces().len()
    }

    #[inline]
    fn num_corners(&self) -> usize {
        self.num_corners
    }

    #[inline]
    fn num_vertices(&self) -> usize {
        self.num_vertices
    }

    #[inline]
    fn point_idx(&self, corner: CornerIdx) -> PointIdx {
        let corner = usize::from(corner);
        self.get_mesh_faces()[corner / 3][corner % 3]
    }
    
    #[inline]
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx {
        self.corner_to_vert(corner)
    }

    #[inline]
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx> {
        if self.opposite_corners[corner] == CornerIdx::from(usize::MAX) {
            None
        } else {
            Some(self.opposite_corners[corner])
        }
    }

    #[inline]
    fn previous(&self, corner: CornerIdx) -> CornerIdx {
        let corner = usize::from(corner);
        let out = if corner%3 == 0 {
            corner+2
        } else {
            corner-1
        };
        CornerIdx::from(out)
    }

    #[inline]
    fn next(&self, corner: CornerIdx) -> CornerIdx {
        let corner = usize::from(corner);
        let out = if corner%3 == 2 {
            corner-2
        } else {
            corner+1
        };
        CornerIdx::from(out)
    }

    #[inline]
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx {
        self.left_most_corners[vertex]
    }
}


#[cfg(test)]
mod tests {
    use crate::{core::attribute::AttributeDomain, prelude::{AttributeType, NdVector}};

    use super::*;

    #[test]
    fn test_corner_table() {
        let faces = vec![
            [PointIdx::from(0), PointIdx::from(1), PointIdx::from(2)], 
            [PointIdx::from(2), PointIdx::from(1), PointIdx::from(3)]
        ];
        let att= Attribute::new(
            vec![
                NdVector::from([0_f32, 0.0]), 
                NdVector::from([1_f32, 0.0]), 
                NdVector::from([0_f32, 1.0]), 
                NdVector::from([1_f32, 1.0])
            ],
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );

        let corner_table = CornerTable::new(&faces, &att);
        assert_eq!(corner_table.num_faces(), 2);
        assert_eq!(corner_table.num_corners(), 6);
        assert_eq!(corner_table.num_vertices(), 4);
        assert_eq!(corner_table.face_idx_containing(CornerIdx::from(0)), FaceIdx::from(0));
        assert_eq!(corner_table.face_idx_containing(CornerIdx::from(1)), FaceIdx::from(0));
        assert_eq!(corner_table.face_idx_containing(CornerIdx::from(2)), FaceIdx::from(0));
        assert_eq!(corner_table.face_idx_containing(CornerIdx::from(3)), FaceIdx::from(1));
        assert_eq!(corner_table.face_idx_containing(CornerIdx::from(4)), FaceIdx::from(1));
        assert_eq!(corner_table.face_idx_containing(CornerIdx::from(5)), FaceIdx::from(1));
        assert!(corner_table.corner_to_vertex.is_empty());
        assert_eq!(corner_table.opposite(CornerIdx::from(0)), Some(CornerIdx::from(5)));
        assert_eq!(corner_table.opposite(CornerIdx::from(1)), None);
        assert_eq!(corner_table.opposite(CornerIdx::from(2)), None);
        assert_eq!(corner_table.opposite(CornerIdx::from(3)), None);
        assert_eq!(corner_table.opposite(CornerIdx::from(4)), None);
        assert_eq!(corner_table.opposite(CornerIdx::from(5)), Some(CornerIdx::from(0)));
        assert_eq!(corner_table.previous(CornerIdx::from(0)), CornerIdx::from(2));
        assert_eq!(corner_table.previous(CornerIdx::from(1)), CornerIdx::from(0));
        assert_eq!(corner_table.previous(CornerIdx::from(2)), CornerIdx::from(1));
        assert_eq!(corner_table.next(CornerIdx::from(0)), CornerIdx::from(1));
        assert_eq!(corner_table.next(CornerIdx::from(1)), CornerIdx::from(2));
        assert_eq!(corner_table.next(CornerIdx::from(2)), CornerIdx::from(0));
    }

    #[test]
    fn test_no_att_seam() {
        let faces = vec![
            [PointIdx::from(0), PointIdx::from(1), PointIdx::from(2)], 
            [PointIdx::from(1), PointIdx::from(3), PointIdx::from(2)], 
            [PointIdx::from(2), PointIdx::from(3), PointIdx::from(4)], 
            [PointIdx::from(2), PointIdx::from(4), PointIdx::from(5)]
        ];
        let att= Attribute::new(
            // Some non-duplicated positions
            vec![
                NdVector::from([0_f32, 0.0, 0.0]), 
                NdVector::from([1_f32, 0.0, 0.0]), 
                NdVector::from([0_f32, 1.0, 0.0]), 
                NdVector::from([1_f32, 1.0, 0.0]),
                NdVector::from([0_f32, 0.5, 0.0]), 
                NdVector::from([1_f32, 0.5, 0.0])
            ],
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );

        let corner_table = CornerTable::new(&faces, &att);
        assert_eq!(corner_table.num_faces(), 4);
        assert_eq!(corner_table.num_corners(), 12);
        assert_eq!(corner_table.num_vertices(), 6);
        assert!(corner_table.corner_to_vertex.is_empty());
    }

    #[test]
    fn test_triangle() {
        let faces = vec![[PointIdx::from(0), PointIdx::from(1), PointIdx::from(2)]];
        let att= Attribute::new(
            vec![
                NdVector::from([0_f32, 0.0]), 
                NdVector::from([1_f32, 0.0]), 
                NdVector::from([0_f32, 1.0])
            ],
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );

        let corner_table = CornerTable::new(&faces, &att);
        assert_eq!(corner_table.num_faces(), 1);
        assert_eq!(corner_table.num_corners(), 3);
        assert_eq!(corner_table.num_vertices(), 3);
        assert_eq!(corner_table.left_most_corners, VecVertexIdx::from( vec![CornerIdx::from(0), CornerIdx::from(1), CornerIdx::from(2)]));
    }

    #[test]
    fn test_non_manifold() {
        let faces = vec![
            [PointIdx::from(0), PointIdx::from(1), PointIdx::from(2)], 
            [PointIdx::from(0), PointIdx::from(3), PointIdx::from(4)]];
        let att= Attribute::new(
            vec![
                NdVector::from([0_f32, 0.0]), 
                NdVector::from([1_f32, 0.0]), 
                NdVector::from([0_f32, 1.0]),
                NdVector::from([-1_f32, 1.0]), 
                NdVector::from([0_f32, -1.0]),
            ],
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );

        let corner_table = CornerTable::new(&faces, &att);
        assert_eq!(corner_table.num_faces(), 2);
        assert_eq!(corner_table.num_corners(), 6);
        assert_eq!(corner_table.num_vertices(), 6); // Vertex 0 is non-manifold, so it is duplicated.
        let answer: VecVertexIdx<CornerIdx> = vec![0, 1, 2, 4, 5, 3].into_iter()
            .map(|x| CornerIdx::from(x))
            .collect::<Vec<_>>()
            .into();
        assert_eq!(corner_table.left_most_corners, answer); 
    }

    #[test]
    fn test_non_manifold_with_seam() {
        let faces = vec![
            [VertexIdx::from(0), VertexIdx::from(1), VertexIdx::from(2)], 
            [VertexIdx::from(1), VertexIdx::from(3), VertexIdx::from(2)], 
            [VertexIdx::from(2), VertexIdx::from(1), VertexIdx::from(4)]
        ];
        assert!(CornerTable::contains_non_manifold_edges(&faces), 
            "The mesh should contain non-manifold edges, but it does not."
        );
    }

    // ToDo: Add tests for non-manifold vertices cases.
}