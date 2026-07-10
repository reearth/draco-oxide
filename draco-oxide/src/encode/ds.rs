use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::mesh::ds::{
    AttributeCornerTable, AttributeDS, CornerTable, GenericCornerTable, DS,
};
use draco_oxide_core::types::{
    CornerIdx, PointIdx, VecCornerIdx, VecPointIdx, VecVertexIdx, VertexIdx,
};

pub(crate) fn build_attribute_ds<'a>(
    ds: &'a DS,
    pos_corner_table: &'a CornerTable,
    attributes: Vec<Attribute>,
) -> Vec<AttributeDS<'a>> {
    attributes
        .into_iter()
        .map(|att| build_single_attribute_ds(ds, pos_corner_table, att))
        .collect()
}

/// Builds a single attribute data structure for the given attribute and the provided global data structure and
/// the position corner table.
/// The input manifold (ds and pos_corner_table) is assumed to be a disjoint union of manifolds with boundary.
fn build_single_attribute_ds<'a>(
    ds: &'a DS,
    pos_corner_table: &'a CornerTable,
    att: Attribute,
) -> AttributeDS<'a> {
    let num_corners = ds.num_corners();
    let num_points = ds.num_points();

    // Attribute value stored at the point that `c` points to.
    let att_val = |c: CornerIdx| att.get_unique_val_idx(ds.point_idx(c));

    let mut is_edge_on_seam: VecCornerIdx<bool> = vec![false; num_corners].into();

    for c in 0..num_corners {
        let c = CornerIdx::from(c);
        let opp_corner = pos_corner_table.opposite(c);
        if opp_corner.is_none() {
            is_edge_on_seam[c] = true;
            continue;
        };

        if usize::from(opp_corner) < usize::from(c) {
            continue;
        }

        let mut c1 = c;
        let mut c2 = opp_corner;
        for _ in 0..2 {
            c1 = c1.next();
            c2 = c2.previous();
            if att_val(c1) != att_val(c2) {
                is_edge_on_seam[c] = true;
                is_edge_on_seam[opp_corner] = true;
                break;
            }
        }
    }

    let is_edge_on_seam: VecCornerIdx<bool> = is_edge_on_seam.into();
    let corner_table = AttributeCornerTable::new(pos_corner_table, is_edge_on_seam);

    let mut point_to_vertex_map = vec![VertexIdx::from(usize::MAX); num_points];
    let mut vertex_to_left_most_corner: Vec<CornerIdx> = Vec::new();
    let mut visited = vec![false; num_corners];

    for start in 0..num_corners {
        if visited[start] {
            continue;
        }
        let start = CornerIdx::from(start);

        let mut pos_left_most = start;
        loop {
            let l = pos_corner_table.swing_left(pos_left_most);
            if l.is_some() {
                if l == start {
                    break;
                } else {
                    pos_left_most = l;
                }
            } else {
                break;
            }
        }

        let mut first_c = pos_left_most;
        loop {
            let l = corner_table.swing_left(first_c);
            if l.is_some() {
                if l == pos_left_most {
                    break;
                } else {
                    first_c = l;
                }
            } else {
                break;
            }
        }

        let mut cur_vert_id = VertexIdx::from(vertex_to_left_most_corner.len());
        vertex_to_left_most_corner.push(first_c);
        point_to_vertex_map[usize::from(ds.point_idx(first_c))] = cur_vert_id;
        visited[usize::from(first_c)] = true;

        let mut maybe_curr = pos_corner_table.swing_right(first_c);
        while maybe_curr.is_some() {
            let curr = maybe_curr;
            if curr == first_c {
                break;
            }
            visited[usize::from(curr)] = true;
            if corner_table.is_corner_opposite_to_seam_edge(curr.next()) {
                cur_vert_id = VertexIdx::from(vertex_to_left_most_corner.len());
                vertex_to_left_most_corner.push(curr);
            }
            point_to_vertex_map[usize::from(ds.point_idx(curr))] = cur_vert_id;
            maybe_curr = pos_corner_table.swing_right(curr);
        }
    }

    let vertex_to_left_most_corner_map: VecVertexIdx<CornerIdx> = vertex_to_left_most_corner.into();
    let point_to_vertex_map: VecPointIdx<VertexIdx> = point_to_vertex_map.into();

    AttributeDS::new(
        ds,
        corner_table,
        vertex_to_left_most_corner_map,
        point_to_vertex_map,
        att,
    )
}

pub(crate) fn build_global_ds(
    mut mesh_faces: Vec<[PointIdx; 3]>,
    pos_att: &Attribute,
) -> (DS, CornerTable) {
    let pos_faces = &mesh_faces
        .iter()
        .map(|face| {
            [
                usize::from(pos_att.get_unique_val_idx(face[0])).into(),
                usize::from(pos_att.get_unique_val_idx(face[1])).into(),
                usize::from(pos_att.get_unique_val_idx(face[2])).into(),
            ]
        })
        .collect::<Vec<[VertexIdx; 3]>>();
    let corner_table = compute_corner_table(pos_faces, &mut mesh_faces);

    let corner_to_point_map: VecCornerIdx<PointIdx> = mesh_faces
        .iter()
        .flat_map(|face| face.iter().copied())
        .collect::<Vec<_>>()
        .into();

    (DS::new(corner_to_point_map), corner_table)
}

/// Builds the corner table for the given position faces, and updates the mesh faces so that the mesh
/// is a disjoint union of manifolds with boundary. The corner table is returned.
/// It traverses over the position connectivity to find the opposite corners for each corner in the mesh.
/// The traversal assumes that edges with face-adjacency not equal to 2 are boundary edges, and it will
/// not traverse across them. When a traversal finds a corner that has already been visited in the previous
/// connected component but not in the current component, it means that the vertex is non-manifold and a new
/// vertex is created for the current component.
fn compute_corner_table(
    pos_faces: &[[VertexIdx; 3]],
    mesh_faces: &mut [[PointIdx; 3]],
) -> CornerTable {
    use std::collections::HashMap;

    let num_corners = pos_faces.len() * 3;

    let pos_vertex = |c: CornerIdx| usize::from(pos_faces[usize::from(c) / 3][usize::from(c) % 3]);

    let mut opposite: VecCornerIdx<CornerIdx> = vec![CornerIdx::none(); num_corners].into();

    let mut half_edges: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for c in 0..num_corners {
        let c = CornerIdx::from(c);
        half_edges
            .entry((pos_vertex(c.next()), pos_vertex(c.previous())))
            .or_default()
            .push(c.into());
    }
    for c in 0..num_corners {
        let c = CornerIdx::from(c);
        let forward = (pos_vertex(c.next()), pos_vertex(c.previous()));
        if half_edges[&forward].len() != 1 {
            continue; // non-manifold in this direction -> boundary.
        }
        if let Some(reverse) = half_edges.get(&(forward.1, forward.0)) {
            if reverse.len() == 1 {
                opposite[c] = reverse[0].into();
            }
        }
    }

    let swing_left = |c: CornerIdx| opposite[c.next()].next();
    let swing_right = |c: CornerIdx| opposite[c.previous()].previous();

    let num_vertices = pos_faces
        .iter()
        .flatten()
        .max()
        .map(|v| usize::from(*v) + 1)
        .unwrap_or(0);

    let num_points = mesh_faces
        .iter()
        .flatten()
        .max()
        .map(|p| usize::from(*p) + 1)
        .unwrap_or(0);
    let mut new_to_old_point_map: Vec<PointIdx> = (0..num_points).map(PointIdx::from).collect();

    let mut left_most_corners: Vec<CornerIdx> = vec![CornerIdx::none(); num_vertices];

    let mut visited_vertices = vec![false; num_vertices];
    let mut visited_corners: VecCornerIdx<bool> = vec![false; num_corners].into();

    for start in 0..num_corners {
        let start = CornerIdx::from(start);
        if visited_corners[start] {
            continue;
        }
        let v = pos_vertex(start);
        let is_non_manifold = visited_vertices[v];
        visited_vertices[v] = true;

        let mut fan: VecCornerIdx<CornerIdx> = vec![start].into();
        visited_corners[start] = true;
        let mut closed = false;
        let mut c = start;
        c = swing_left(c);
        while c.is_some() {
            if c == start {
                closed = true;
                break;
            }
            visited_corners[c] = true;
            fan.push(c);
            c = swing_left(c);
        }
        let left_most = CornerIdx::from(c);
        if !closed {
            let mut c = start;
            c = swing_right(c);
            while c.is_some() {
                visited_corners[c] = true;
                fan.push(c);
                c = swing_right(c);
            }
        }

        if !is_non_manifold {
            left_most_corners[v] = left_most;
            continue;
        }

        left_most_corners.push(left_most);
        let mut remap: HashMap<PointIdx, PointIdx> = HashMap::new();
        for c in fan {
            let c = usize::from(c);
            let old_point = mesh_faces[c / 3][c % 3];
            let new_point = *remap.entry(old_point).or_insert_with(|| {
                let np = PointIdx::from(new_to_old_point_map.len());
                new_to_old_point_map.push(old_point);
                np
            });
            mesh_faces[c / 3][c % 3] = new_point;
        }
    }

    let opposite_corners: VecCornerIdx<CornerIdx> = opposite
        .iter()
        .map(|&o| CornerIdx::from(o))
        .collect::<Vec<_>>()
        .into();

    for (new, old) in new_to_old_point_map.iter().enumerate() {
        if usize::from(*old) != new {
            left_most_corners.push(left_most_corners[usize::from(*old)]);
        }
    }

    CornerTable::from_raw_data(opposite_corners)
}

#[cfg(test)]
mod tests {
    use super::*;
    use draco_oxide_core::attribute::{Attribute, AttributeDomain, AttributeType};
    use draco_oxide_core::types::{FaceIdx, NdVector};

    fn pos_attribute(vals: Vec<[f32; 3]>) -> Attribute {
        Attribute::new(
            vals.into_iter().map(NdVector::<3, f32>::from).collect(),
            AttributeType::Position,
            AttributeDomain::Position,
            Vec::new(),
        )
    }

    fn pos_attribute_2d(vals: Vec<[f32; 2]>) -> Attribute {
        Attribute::new(
            vals.into_iter().map(NdVector::<2, f32>::from).collect(),
            AttributeType::Position,
            AttributeDomain::Position,
            Vec::new(),
        )
    }

    fn faces(raw: Vec<[usize; 3]>) -> Vec<[PointIdx; 3]> {
        raw.into_iter()
            .map(|f| {
                [
                    PointIdx::from(f[0]),
                    PointIdx::from(f[1]),
                    PointIdx::from(f[2]),
                ]
            })
            .collect()
    }

    /// Minimal reproduction of the closed-manifold point-inflation bug.
    ///
    /// A textbook tetrahedron: 4 distinct positions, 4 consistently-oriented
    /// faces, fully manifold, no duplicate points. `build_global_ds` must
    /// produce exactly 4 points (an identity point map). Instead
    /// `compute_corner_table` spuriously treats the closed vertex fans as
    /// non-manifold and mints phantom points (`num_points()` == 7), after which
    /// `build_attribute_ds` panics indexing the position attribute with a
    /// phantom point index.
    #[test]
    fn build_global_ds_does_not_inflate_points_on_closed_tetrahedron() {
        let faces: Vec<[PointIdx; 3]> = vec![[0, 1, 2], [0, 2, 3], [0, 3, 1], [1, 3, 2]]
            .into_iter()
            .map(|f| {
                [
                    PointIdx::from(f[0]),
                    PointIdx::from(f[1]),
                    PointIdx::from(f[2]),
                ]
            })
            .collect();

        let attributes = vec![pos_attribute(vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ])];

        let (ds, pos_corner_table) = build_global_ds(faces, &attributes[0]);

        // The mesh has no duplicate points, so no point should be created.
        assert_eq!(
            ds.num_points(),
            4,
            "closed manifold tetrahedron must not mint phantom points"
        );

        // Building the attribute DS must not panic (it does today, indexing the
        // 4-value position attribute with a phantom point index).
        let _adss = build_attribute_ds(&ds, &pos_corner_table, attributes);
    }

    // The following tests were carried over (as the corner-table construction
    // tests) from the pre-refactor `corner_table` module. Construction now lives
    // in `build_global_ds`/`build_single_attribute_ds`, so they are expressed
    // against that API here: `DS` owns the face/corner/point counts and the
    // opposite table, and the per-attribute `AttributeDS` owns the vertices.

    /// Two triangles sharing the edge {1,2}. Only that pair of corners is
    /// interior; every other edge is a boundary.
    #[test]
    fn corner_table_two_triangles() {
        let attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
        ])];
        let (ds, ct) = build_global_ds(faces(vec![[0, 1, 2], [2, 1, 3]]), &attributes[0]);
        let adss = build_attribute_ds(&ds, &ct, attributes);
        let pos = &adss[0];

        assert_eq!(ds.num_faces(), 2);
        assert_eq!(ds.num_corners(), 6);
        assert_eq!(ds.num_points(), 4);
        assert_eq!(pos.num_vertices(), 4);

        // face containment is now derived from the corner index directly.
        for c in 0..6 {
            assert_eq!(CornerIdx::from(c).face_idx(), FaceIdx::from(c / 3));
        }

        // Only corners 0 and 5 (across the shared edge {1,2}) are opposite.
        assert_eq!(ct.opposite(CornerIdx::from(0)), CornerIdx::from(5));
        assert_eq!(ct.opposite(CornerIdx::from(5)), CornerIdx::from(0));
        for c in [1, 2, 3, 4] {
            assert!(ct.opposite(CornerIdx::from(c)).is_none());
        }
    }

    /// A four-triangle strip with all-distinct positions: no shared vertices are
    /// merged beyond the ones the connectivity implies, so there are 6 vertices
    /// and no point duplication.
    #[test]
    fn corner_table_no_attribute_seam() {
        let attributes = vec![pos_attribute(vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.5, 0.0],
            [1.0, 0.5, 0.0],
        ])];
        let (ds, ct) = build_global_ds(
            faces(vec![[0, 1, 2], [1, 3, 2], [2, 3, 4], [2, 4, 5]]),
            &attributes[0],
        );
        let adss = build_attribute_ds(&ds, &ct, attributes);

        assert_eq!(ds.num_faces(), 4);
        assert_eq!(ds.num_corners(), 12);
        assert_eq!(ds.num_points(), 6);
        assert_eq!(adss[0].num_vertices(), 6);
    }

    /// A single triangle: three boundary vertices, each with its own corner.
    #[test]
    fn corner_table_triangle() {
        let attributes = vec![pos_attribute_2d(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])];
        let (ds, ct) = build_global_ds(faces(vec![[0, 1, 2]]), &attributes[0]);
        let adss = build_attribute_ds(&ds, &ct, attributes);
        let pos = &adss[0];

        assert_eq!(ds.num_faces(), 1);
        assert_eq!(ds.num_corners(), 3);
        assert_eq!(ds.num_points(), 3);
        assert_eq!(pos.num_vertices(), 3);

        // Each vertex's left-most corner belongs to that vertex.
        for v in 0..pos.num_vertices() {
            let v = VertexIdx::from(v);
            assert_eq!(pos.vertex_idx(pos.left_most_corner(v)), v);
        }
    }

    /// Two triangles meeting only at position 0 (a non-manifold vertex). The
    /// shared position must be split into two vertices — one per fan — so the
    /// point count grows from 5 to 6 and there are 6 vertices.
    #[test]
    fn corner_table_non_manifold_vertex() {
        let attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [-1.0, 1.0],
            [0.0, -1.0],
        ])];
        let (ds, ct) = build_global_ds(faces(vec![[0, 1, 2], [0, 3, 4]]), &attributes[0]);
        let adss = build_attribute_ds(&ds, &ct, attributes);
        let pos = &adss[0];

        assert_eq!(ds.num_faces(), 2);
        assert_eq!(ds.num_corners(), 6);
        // Position 0 is non-manifold, so it is duplicated into its own point.
        assert_eq!(ds.num_points(), 6);
        assert_eq!(pos.num_vertices(), 6);

        // Each corner maps to a vertex, and each vertex's left-most corner maps
        // back to it.
        for v in 0..pos.num_vertices() {
            let v = VertexIdx::from(v);
            assert_eq!(pos.vertex_idx(pos.left_most_corner(v)), v);
        }
    }

    /// The edge {1,2} is shared by three faces (a non-manifold edge). Such edges
    /// carry face-adjacency != 2, so `build_global_ds` treats them as boundaries
    /// and leaves the corners across them without an opposite.
    #[test]
    fn corner_table_non_manifold_edge_is_boundary() {
        let attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
            [-1.0, 0.0],
        ])];
        let (ds, ct) =
            build_global_ds(faces(vec![[0, 1, 2], [1, 3, 2], [2, 1, 4]]), &attributes[0]);
        let _adss = build_attribute_ds(&ds, &ct, attributes);

        // Corners 0, 4, 8 are opposite the non-manifold edge {1,2}; none is linked.
        for c in [0, 4, 8] {
            assert!(
                ct.opposite(CornerIdx::from(c)).is_none(),
                "corner {c} across the non-manifold edge must be a boundary"
            );
        }
    }
}
