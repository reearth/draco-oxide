pub(crate) mod attribute_corner_table;
pub(crate) mod all_inclusive_corner_table;

use std::collections::BTreeMap;

use crate::core::shared::{CornerIdx, FaceIdx, VertexIdx};

#[derive(Debug, Clone)]
enum Faces<'faces> {
    Taken(Vec<[VertexIdx;3]>),
    Borrowed(&'faces [[VertexIdx;3]])
}

pub(crate) trait GenericCornerTable {
    fn face_idx_containing(&self, corner: CornerIdx) -> FaceIdx;
    fn num_faces(&self) -> usize;
    fn num_corners(&self) -> usize;
    fn num_vertices(&self) -> usize;
    fn vertex_idx(&self, corner: CornerIdx) -> VertexIdx;
    fn opposite(&self, corner: CornerIdx) -> Option<CornerIdx>;
    fn previous(&self, corner: CornerIdx) -> CornerIdx;
    fn next(&self, corner: CornerIdx) -> CornerIdx;
    fn left_most_corner(&self, vertex: VertexIdx) -> CornerIdx;

    fn swing_right(&self, corner: usize) -> Option<usize> {
        if let Some(c) = self.opposite(self.previous(corner)){
            Some(self.previous(c))
        } else {
            None
        }
    }

    fn swing_left(&self, corner: usize) -> Option<usize> {
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
}

#[derive(Debug, Clone)]
pub(crate) struct CornerTable<'faces> {
    /// Records the opposite corner for each corner.
    /// If a corner does not have an opposite corner, the value is 'usize::MAX'.
    opposite_corners: Vec<usize>,

    /// corner to vertex map
    faces: Faces<'faces>,

    /// Number of corners in the mesh.
    num_corners: usize,

    // Number of vertices in the mesh.
    num_vertices: usize,

    /// Stores the left most corner for each vertex.
    left_most_corners: Vec<usize>,

    /// corner to vertex map.
    corner_to_vertex: Vec<VertexIdx>,

    /// Stores the parents of non-manifold vertices.
    non_manifold_vertex_parents: Vec<VertexIdx>,
}

impl<'faces> CornerTable<'faces> {
    pub(crate) fn new(faces: &'faces [[VertexIdx;3]]) -> Self {
        let mut out  = Self {
            opposite_corners: Vec::new(), // will be computed later
            faces: Faces::Borrowed(faces),
            num_corners: faces.len() * 3,
            num_vertices: 0, // will be computed later
            left_most_corners: Vec::new(), // will be computed later
            corner_to_vertex: Vec::new(), // will be computed later
            non_manifold_vertex_parents: Vec::new(), // will be computed later
        };

        for f in 0..faces.len() {
            for i in 0..3 {
                out.corner_to_vertex.push(faces[f][i]);
            }
        }
        out.compute_table();
        out.compute_left_most_corners();

        out
    }

    pub(crate) fn new_with_taken_faces(faces: Vec<[VertexIdx;3]>) -> Self {
        let mut out  = Self {
            opposite_corners: Vec::new(), // will be computed later
            num_corners: faces.len() * 3,
            corner_to_vertex: Vec::new(), // will be computed later
            faces: Faces::Taken(faces),
            num_vertices: 0, // will be computed later
            left_most_corners: Vec::new(), // will be computed later
            non_manifold_vertex_parents: Vec::new(), // will be computed later
        };

        for f in 0..out.get_faces().len() {
            for i in 0..3 {
                out.corner_to_vertex.push(out.get_faces()[f][i]);
            }
        }

        out.compute_table();
        out.compute_left_most_corners();

        out
    }

    fn compute_table(&mut self) {
        self.opposite_corners.resize(self.num_corners(), usize::MAX);
        let mut num_corners_on_vertices = Vec::with_capacity(self.num_corners());

        // Compute the number of corners on each vertex.
        // The vertices are sorted by their earliest corner.
        for c in 0..self.num_corners() {
            let v1 = self.vertex_idx(c);
            if v1 >= num_corners_on_vertices.len() {
                num_corners_on_vertices.resize(v1 + 1, 0);
            }
            num_corners_on_vertices[v1]+=1;
        }

        // Array for storing half edges. (sink vertex, edge corner)
        let mut vertex_edges: Vec<(VertexIdx, CornerIdx)> = vec![ (usize::MAX, usize::MAX); self.num_corners() ];

        let mut offset = 0;
        // Compute the offset of the the earliest corner for each vertex.
        let vertex_offset = (0..num_corners_on_vertices.len()).map(|i| {
                let out = offset;
                offset += num_corners_on_vertices[i];
                out
            })
            .collect::<Vec<_>>();

        for c in 0..self.num_corners() {
            let tip_v = self.vertex_idx(c);
            let source_v = self.vertex_idx(self.next(c));
            let sink_v = self.vertex_idx(self.previous(c));

            let f_idx = self.face_idx_containing(c);
            if c == Self::first_corner(f_idx) {
                let v0 = self.vertex_idx(c);
                if v0 == source_v || v0 == sink_v || source_v == sink_v {
                    unimplemented!("Degenerated face found in corner table computation.");
                }
            }

            let mut opposite_c = usize::MAX;
            let num_corners_on_vert = num_corners_on_vertices[sink_v];
            offset = vertex_offset[sink_v];
            for i in 0..num_corners_on_vert {
                let other_v = vertex_edges[offset].0;
                if other_v == usize::MAX {
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
                        if vertex_edges[offset].0 == usize::MAX {
                            break;
                        }
                        offset += 1;
                    }
                    vertex_edges[offset].0 = usize::MAX;
                    break;
                }
                offset+=1;
            }
            if opposite_c == usize::MAX {
                let num_corners_on_source_vert = num_corners_on_vertices[source_v];
                let first_c = vertex_offset[source_v];
                for corner in first_c..num_corners_on_source_vert+first_c {
                    if vertex_edges[corner].0 == usize::MAX {
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
        self.left_most_corners.resize(self.num_vertices(), usize::MAX);
        let mut visited_vertices = vec![false; self.num_vertices()];
        let mut visited_corners = vec![false; self.num_corners()];

        for f_idx in 0..self.get_faces().len() {
            for i in 0..3 {
                let c = 3*f_idx + i;
                if visited_corners[c] { continue; }

                let mut v = self.corner_to_vertex[c];
                let mut is_non_manifold_vertex = false;
                if visited_vertices[v] {
                    // Coming here means the vertex has a neighborhood that is not connected when the vertex is removed,
                    // i.e. it is violating the manifold condition.
                    // We need to create a new vertex here to avoid this case.
                    self.left_most_corners.push(usize::MAX);
                    self.non_manifold_vertex_parents.push(v);
                    visited_vertices.push(false);
                    v = self.num_vertices;
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
                    visited_corners[act_c] = true;
                    self.left_most_corners[v] = act_c;
                    if is_non_manifold_vertex {
                        // Update vertex index in the corresponding face.
                        self.corner_to_vertex.insert(act_c, v);
                    }
                    maybe_act_c = self.swing_left(act_c);
                    if act_c == c {
                        // Reached back to the initial corner.
                        break;  
                    }
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
    pub(crate) fn first_corner(face_idx: usize) -> usize {
        face_idx * 3
    }

    #[inline]
    pub(crate) fn get_faces(&self) -> &[[VertexIdx; 3]] {
        match &self.faces {
            Faces::Taken(faces) => faces,
            Faces::Borrowed(faces) => faces,
        }
    }

    #[inline]
    pub(crate) fn corner_to_vert(&self, corner: usize) -> VertexIdx {
        if let Some(v) = self.corner_to_vertex.get(corner) {
            return *v
        };

        let local = corner % 3;
        let face_idx = corner / 3;

        match local {
            0 => self.get_faces()[face_idx][0],
            1 => self.get_faces()[face_idx][1],
            2 => self.get_faces()[face_idx][2],
            _ => unreachable!(), // it is safe to assume this as 'local' is the remainder of a number divided by 3.
        }
    }
}

impl<'mesh> GenericCornerTable for CornerTable<'mesh> {
    #[inline]
    fn face_idx_containing(&self, corner: usize) -> usize {
        corner / 3
    }

    #[inline]
    fn num_faces(&self) -> usize {
        self.get_faces().len()
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
    fn vertex_idx(&self, corner: usize) -> VertexIdx {
        self.corner_to_vertex[corner]
    }

    #[inline]
    fn opposite(&self, corner: usize) -> Option<usize> {
        if self.opposite_corners[corner] == usize::MAX {
            None
        } else {
            Some(self.opposite_corners[corner])
        }
    }

    #[inline]
    fn previous(&self, corner: usize) -> usize {
        if corner%3 == 0 {
            corner+2
        } else {
            corner-1
        }
    }

    #[inline]
    fn next(&self, corner: usize) -> usize {
        if corner%3 == 2 {
            corner-2
        } else {
            corner+1
        }
    }

    #[inline]
    fn left_most_corner(&self, vertex: usize) -> usize {
        self.left_most_corners[vertex]
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_corner_table() {
        let faces = vec![[0, 1, 2], [2, 1, 3]];
        let corner_table = CornerTable::new_with_taken_faces(faces);
        assert_eq!(corner_table.num_faces(), 2);
        assert_eq!(corner_table.num_corners(), 6);
        assert_eq!(corner_table.num_vertices(), 4);
        assert_eq!(corner_table.vertex_idx(0), 0);
        assert_eq!(corner_table.vertex_idx(1), 1);
        assert_eq!(corner_table.vertex_idx(2), 2);
        assert_eq!(corner_table.vertex_idx(3), 2);
        assert_eq!(corner_table.vertex_idx(4), 1);
        assert_eq!(corner_table.vertex_idx(5), 3);
        assert_eq!(corner_table.face_idx_containing(0), 0);
        assert_eq!(corner_table.face_idx_containing(1), 0);
        assert_eq!(corner_table.face_idx_containing(2), 0);
        assert_eq!(corner_table.face_idx_containing(3), 1);
        assert_eq!(corner_table.face_idx_containing(4), 1);
        assert_eq!(corner_table.face_idx_containing(5), 1);
        assert_eq!(corner_table.opposite(0), Some(5));
        assert_eq!(corner_table.opposite(1), None);
        assert_eq!(corner_table.opposite(2), None);
        assert_eq!(corner_table.opposite(3), None);
        assert_eq!(corner_table.opposite(4), None);
        assert_eq!(corner_table.opposite(5), Some(0));
        assert_eq!(corner_table.previous(0), 2);
        assert_eq!(corner_table.previous(1), 0);
        assert_eq!(corner_table.previous(2), 1);
        assert_eq!(corner_table.next(0), 1);
        assert_eq!(corner_table.next(1), 2);
        assert_eq!(corner_table.next(2), 0);
    }

    #[test]
    fn test_corner_table_disk() {
        let faces = vec![
            [0, 1, 2], [0, 2, 3], [0, 3, 4], 
            [0, 4, 5],[0, 5, 6], [0, 6, 1]
        ];

        let corner_table = CornerTable::new_with_taken_faces(faces);
        assert_eq!(corner_table.num_faces(), 6);
        assert_eq!(corner_table.num_corners(), 18);
        assert_eq!(corner_table.num_vertices(), 7);
        assert_eq!(corner_table.opposite(1), Some(5));
        assert_eq!(corner_table.opposite(4), Some(8));
        assert_eq!(corner_table.opposite(2), Some(16));
    }

    #[test]
    fn test_corner_table_sphere() {
        // read the sphere.obj file and create a corner table
        let sphere = tobj::load_obj(
            "tests/data/sphere.obj",
            &tobj::GPU_LOAD_OPTIONS
        ).unwrap();
        let sphere = &sphere.0[0];
        let mesh = &sphere.mesh;
        let faces = mesh.indices.chunks(3)
            .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
            .collect::<Vec<_>>();
        let corner_table = CornerTable::new_with_taken_faces(faces);

        for c in 0..corner_table.num_corners() {
            assert!(corner_table.opposite(c).is_some());
        }
    }

    // ToDo: Add tests for non-manifold vertices cases.
}