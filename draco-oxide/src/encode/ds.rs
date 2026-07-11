use draco_oxide_core::attribute::{Attribute, AttributeType};
use draco_oxide_core::mesh::ds::{
    AttributeCornerTable, AttributeDS, CornerTable, GenericCornerTable, DS,
};
use draco_oxide_core::safety_assert;
use draco_oxide_core::types::{
    CornerIdx, PointIdx, VecCornerIdx, VecFaceIdx, VecPointIdx, VecVertexIdx, VertexIdx,
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

/// Builds a single attribute data structure for the given attribute, the global data structure and
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

/// Builds the global data structure and position corner table, and extends
/// `attributes` in place to cover any phantom points minted while splitting the
/// mesh into orientable manifolds-with-boundary. `attributes` must contain a
/// position attribute.
pub(crate) fn build_global_ds(
    mut mesh_faces: Vec<[PointIdx; 3]>,
    attributes: &mut [Attribute],
) -> (DS, CornerTable) {
    let pos_att = attributes
        .iter()
        .find(|att| att.get_attribute_type() == AttributeType::Position)
        .expect("position attribute must be present");
    let mut pos_faces = mesh_faces
        .iter()
        .map(|face| {
            [
                usize::from(pos_att.get_unique_val_idx(face[0])).into(),
                usize::from(pos_att.get_unique_val_idx(face[1])).into(),
                usize::from(pos_att.get_unique_val_idx(face[2])).into(),
            ]
        })
        .collect::<Vec<[VertexIdx; 3]>>();
    let (corner_table, new_to_old_point_map) =
        compute_corner_table(&mut pos_faces, &mut mesh_faces);

    let corner_to_point_map: VecCornerIdx<PointIdx> = mesh_faces
        .iter()
        .flat_map(|face| face.iter().copied())
        .collect::<Vec<_>>()
        .into();

    let ds = DS::new(corner_to_point_map);

    let num_points = ds.num_points();
    for att in attributes.iter_mut() {
        for p in att.len()..num_points {
            att.mint(new_to_old_point_map[p]);
        }
    }

    (ds, corner_table)
}

/// Builds the corner table for the given position faces, and updates the mesh faces so that the mesh
/// is a disjoint union of manifolds with boundary. The corner table is returned.
/// It traverses over the position connectivity to find the opposite corners for each corner in the mesh.
/// The traversal assumes that edges with face-adjacency not equal to 2 are boundary edges, and it will
/// not traverse across them. When a traversal finds a corner that has already been visited in the previous
/// connected component but not in the current component, it means that the vertex is non-manifold and a new
/// vertex is created for the current component.
fn compute_corner_table(
    pos_faces: &mut [[VertexIdx; 3]],
    mesh_faces: &mut [[PointIdx; 3]],
) -> (CornerTable, Vec<PointIdx>) {
    use std::collections::HashMap;

    let num_corners = pos_faces.len() * 3;

    let pos_vertex = |c: CornerIdx| usize::from(pos_faces[usize::from(c) / 3][usize::from(c) % 3]);

    let mut opposite: VecCornerIdx<CornerIdx> = vec![CornerIdx::none(); num_corners].into();

    let mut edge_coboundary: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for c in 0..num_corners {
        let c = CornerIdx::from(c);
        let next = pos_vertex(c.next());
        let prev = pos_vertex(c.previous());
        let entry = if next < prev {
            (next, prev)
        } else {
            (prev, next)
        };
        edge_coboundary.entry(entry).or_default().push(c.into());
    }
    for corners in edge_coboundary.values() {
        if corners.len() != 2 {
            continue; // non-manifold in this direction -> boundary.
        }
        let c1 = CornerIdx::from(corners[0]);
        let c2 = CornerIdx::from(corners[1]);
        opposite[c1] = c2;
        opposite[c2] = c1;
    }

    orient_faces(&mut opposite, pos_faces, mesh_faces);

    let pos_vertex = |c: CornerIdx| usize::from(pos_faces[usize::from(c) / 3][usize::from(c) % 3]);

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
            continue;
        }

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

    (
        CornerTable::from_raw_data(opposite_corners),
        new_to_old_point_map,
    )
}

/// Orient faces, in the sense that faces connected by an edge will have consistent orientation.
/// When the input mesh has a non-orientable surface, then the opposite relation will be removed at
/// some edges.
fn orient_faces(
    corner_table: &mut VecCornerIdx<CornerIdx>,
    pos_faces: &mut [[VertexIdx; 3]],
    mesh_faces: &mut [[PointIdx; 3]],
) {
    let mut face_orientation: VecFaceIdx<Option<bool>> = vec![None; pos_faces.len()].into();
    for c in 0..pos_faces.len() * 3 {
        let c = CornerIdx::from(c);
        if face_orientation[c.face_idx()].is_some() {
            continue;
        }
        let mut stack = vec![c, c.next(), c.previous()];
        face_orientation[c.face_idx()] = Some(true);
        while let Some(c) = stack.pop() {
            let next_c = c.next();
            let prev_c = c.previous();
            let opp_c = corner_table[c];
            if opp_c.is_none() {
                continue;
            }
            let next_opp_c = opp_c.next();
            let prev_opp_c = opp_c.previous();
            if pos_faces[usize::from(c.face_idx())][usize::from(next_c) % 3]
                == pos_faces[usize::from(opp_c.face_idx())][usize::from(prev_opp_c) % 3]
                && pos_faces[usize::from(c.face_idx())][usize::from(prev_c) % 3]
                    == pos_faces[usize::from(opp_c.face_idx())][usize::from(next_opp_c) % 3]
            {
                if face_orientation[opp_c.face_idx()].is_none() {
                    face_orientation[opp_c.face_idx()] = face_orientation[c.face_idx()];
                    // Only recurse into a face the first time it is oriented;
                    // re-pushing already-oriented faces loops forever on any mesh
                    // with cycles (e.g. a closed manifold).
                    stack.push(next_opp_c);
                    stack.push(prev_opp_c);
                } else if face_orientation[opp_c.face_idx()] != face_orientation[c.face_idx()] {
                    // The two faces have inconsistent orientation, so we remove the opposite relation.
                    corner_table[c] = CornerIdx::none();
                    corner_table[opp_c] = CornerIdx::none();
                }
            } else {
                if face_orientation[opp_c.face_idx()].is_none() {
                    face_orientation[opp_c.face_idx()] =
                        Some(!face_orientation[c.face_idx()].unwrap());
                    stack.push(next_opp_c);
                    stack.push(prev_opp_c);
                } else if face_orientation[opp_c.face_idx()] == face_orientation[c.face_idx()] {
                    // The two faces have inconsistent orientation, so we remove the opposite relation.
                    corner_table[c] = CornerIdx::none();
                    corner_table[opp_c] = CornerIdx::none();
                }
            }
        }
    }
    safety_assert!(face_orientation.iter().all(|o| o.is_some()));
    for (f, o) in face_orientation.into_iter().enumerate() {
        // Safety: face_orientation is guaranteed to be all Some, so unwrap_unchecked is safe here.
        if unsafe { o.unwrap_unchecked() } {
            continue;
        }

        mesh_faces[f].swap(0, 1);
        pos_faces[f].swap(0, 1);
        corner_table.swap((f * 3).into(), (f * 3 + 1).into());
        let opp1 = corner_table[(f * 3).into()];
        let opp2 = corner_table[(f * 3 + 1).into()];
        if opp1.is_some() {
            corner_table[opp1] = (f * 3).into();
        }
        if opp2.is_some() {
            corner_table[opp2] = (f * 3 + 1).into();
        }
    }
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

        let mut attributes = vec![pos_attribute(vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ])];

        let (ds, pos_corner_table) = build_global_ds(faces, &mut attributes);

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
        let mut attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
        ])];
        let (ds, ct) = build_global_ds(faces(vec![[0, 1, 2], [2, 1, 3]]), &mut attributes);
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
        let mut attributes = vec![pos_attribute(vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.5, 0.0],
            [1.0, 0.5, 0.0],
        ])];
        let (ds, ct) = build_global_ds(
            faces(vec![[0, 1, 2], [1, 3, 2], [2, 3, 4], [2, 4, 5]]),
            &mut attributes,
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
        let mut attributes = vec![pos_attribute_2d(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])];
        let (ds, ct) = build_global_ds(faces(vec![[0, 1, 2]]), &mut attributes);
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
        let mut attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [-1.0, 1.0],
            [0.0, -1.0],
        ])];
        let (ds, ct) = build_global_ds(faces(vec![[0, 1, 2], [0, 3, 4]]), &mut attributes);
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
        let mut attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
            [-1.0, 0.0],
        ])];
        let (ds, ct) = build_global_ds(
            faces(vec![[0, 1, 2], [1, 3, 2], [2, 1, 4]]),
            &mut attributes,
        );
        let _adss = build_attribute_ds(&ds, &ct, attributes);

        // Corners 0, 4, 8 are opposite the non-manifold edge {1,2}; none is linked.
        for c in [0, 4, 8] {
            assert!(
                ct.opposite(CornerIdx::from(c)).is_none(),
                "corner {c} across the non-manifold edge must be a boundary"
            );
        }
    }

    /// Minimal 5-triangle Möbius band, used to probe how `build_global_ds`
    /// behaves on a non-orientable input.
    ///
    /// The interior edges form the core {0,1},{1,2},{2,3},{3,4},{0,4}; the five
    /// remaining edges form a single boundary cycle 0-2-4-1-3-0 (one boundary
    /// component ⇒ Möbius, not an annulus). The closing edge {0,1} is the twist:
    /// both incident faces traverse it in the same direction (0→1).
    fn mobius_band() -> Vec<[PointIdx; 3]> {
        faces(vec![[0, 1, 2], [2, 1, 3], [2, 3, 4], [4, 3, 0], [4, 0, 1]])
    }

    fn mobius_positions() -> Vec<Attribute> {
        // Positions are arbitrary and distinct; only connectivity matters.
        vec![pos_attribute(vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
        ])]
    }

    /// A genuinely NON-ORIENTABLE surface. `orient_faces` propagates a
    /// consistent orientation as far as it can, then hits a contradiction at the
    /// twist and cuts that one edge, so `build_global_ds` itself completes: of
    /// the 5 core edges, 4 stay linked (8 corners) and the twist edge is cut.
    #[test]
    fn build_global_ds_cuts_twist_edge_of_non_orientable_surface() {
        let mut attributes = mobius_positions();
        let (ds, ct) = build_global_ds(mobius_band(), &mut attributes);

        assert_eq!(ds.num_faces(), 5);
        assert_eq!(ds.num_corners(), 15);

        // Exactly one of the five core edges (the twist) is cut, leaving four
        // interior edges = eight linked corners.
        let linked = (0..ds.num_corners())
            .filter(|&c| ct.opposite(CornerIdx::from(c)).is_some())
            .count();
        assert_eq!(linked, 8, "one core (twist) edge should be cut");
    }

    /// A genuinely NON-ORIENTABLE surface now builds its attribute DS without
    /// panicking. Cutting the twist edge turns its two endpoints into
    /// non-manifold vertices, which `compute_corner_table` splits by minting new
    /// point indices; `build_single_attribute_ds` then extends the position
    /// attribute so each phantom point aliases the value of the point it was
    /// split from (`Attribute::mint`). The result: 5 original points grow to 7
    /// (one phantom per cut endpoint), the attribute is extended to match, and
    /// the whole `corner -> point -> vertex -> value` chain is total.
    #[test]
    fn build_attribute_ds_handles_non_orientable_surface() {
        let mut attributes = mobius_positions();
        let (ds, ct) = build_global_ds(mobius_band(), &mut attributes);

        // Two phantom points minted (the twist edge's two endpoints).
        assert_eq!(ds.num_points(), 7, "two phantom points from the cut");

        let adss = build_attribute_ds(&ds, &ct, attributes);
        let pos = &adss[0];

        // The attribute was extended to cover the phantom points, so the value
        // lookup is total over every point (no more out-of-bounds panic).
        assert_eq!(pos.att_data().len(), ds.num_points());

        // Position carries no seams, so each point is its own vertex, and every
        // vertex's left-most corner maps back to it.
        assert_eq!(pos.num_vertices(), ds.num_points());
        for v in 0..pos.num_vertices() {
            let v = VertexIdx::from(v);
            assert_eq!(pos.vertex_idx(pos.left_most_corner(v)), v);
        }
    }

    /// An ORIENTABLE surface whose input faces are wound inconsistently. Two
    /// triangles form a quad sharing edge {1,2}; face 1 is wound the "wrong" way
    /// ([1,2,3]). `orient_faces` now reorients it, so the shared edge links and
    /// the quad stays a single connected component — 4 points, 4 vertices, no
    /// duplication (contrast the old behavior, which shattered it into 6 points).
    #[test]
    fn build_global_ds_reorients_inconsistent_orientable_quad() {
        let mut attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
        ])];
        let (ds, ct) = build_global_ds(faces(vec![[0, 1, 2], [1, 2, 3]]), &mut attributes);

        // No duplication: the quad is preserved as a connected component.
        assert_eq!(ds.num_points(), 4);
        // The shared edge is linked (exactly one interior edge = two corners).
        let linked = (0..ds.num_corners())
            .filter(|&c| ct.opposite(CornerIdx::from(c)).is_some())
            .count();
        assert_eq!(linked, 2);

        let adss = build_attribute_ds(&ds, &ct, attributes);
        assert_eq!(adss[0].num_vertices(), 4);
    }

    /// An ORIENTABLE 3-triangle strip with its END triangle wound the wrong way
    /// ([3,2,4] instead of [2,3,4]). `orient_faces` reorients it, so both
    /// interior edges link and no vertex is duplicated — 5 points, 5 vertices
    /// (contrast the old behavior: 7 points / 7 vertices).
    #[test]
    fn build_global_ds_reorients_inconsistent_orientable_strip() {
        let mut attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
            [2.0, 0.0],
        ])];
        let (ds, ct) = build_global_ds(
            faces(vec![[0, 1, 2], [2, 1, 3], [3, 2, 4]]),
            &mut attributes,
        );

        assert_eq!(ds.num_points(), 5);
        let adss = build_attribute_ds(&ds, &ct, attributes);
        assert_eq!(adss[0].num_vertices(), 5);
    }
}
