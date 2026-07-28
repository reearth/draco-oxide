use std::collections::HashMap;

use draco_oxide_core::attribute::{Attribute, AttributeType};
use draco_oxide_core::mesh::ds::{
    AttributeCornerTable, AttributeDS, CornerTable, GenericCornerTable, DS,
};
use draco_oxide_core::safety_assert;
use draco_oxide_core::types::{
    AttributeValueIdx, CornerIdx, PointIdx, VecCornerIdx, VecPointIdx, VecVertexIdx, VertexIdx,
};

/// Marks every edge across which `att_val` disagrees as an attribute seam
/// (position boundaries are seams as well). `att_val(c)` must return the
/// attribute value stored at the point corner `c` points to.
fn compute_seam_edges<F>(
    pos_corner_table: &CornerTable,
    num_corners: usize,
    att_val: F,
) -> VecCornerIdx<bool>
where
    F: Fn(CornerIdx) -> AttributeValueIdx,
{
    let mut is_edge_on_seam: VecCornerIdx<bool> = vec![false; num_corners].into();

    for c in 0..num_corners {
        let c = CornerIdx::from(c);
        let Some(opp_corner) = pos_corner_table.opposite(c) else {
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

    is_edge_on_seam
}

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

    let is_edge_on_seam = compute_seam_edges(pos_corner_table, num_corners, |c| {
        att.get_unique_val_idx(ds.point_idx(c))
    });
    let corner_table = AttributeCornerTable::new(pos_corner_table, is_edge_on_seam);

    // Safety contract: a point-keyed map is a function only if every point's
    // corners lie in a single seam-separated sector. `sort_mesh` establishes
    // this invariant; the `safety_assert`s below enforce it.
    let mut point_to_vertex_map = vec![VertexIdx::INVALID; num_points];
    let mut vertex_to_left_most_corner: Vec<CornerIdx> = Vec::new();
    let mut visited = vec![false; num_corners];

    for start in 0..num_corners {
        if visited[start] {
            continue;
        }
        let start = CornerIdx::from(start);

        let mut pos_left_most = start;
        while let Some(l) = pos_corner_table.swing_left(pos_left_most) {
            if l == start {
                break;
            }
            pos_left_most = l;
        }

        let mut first_c = pos_left_most;
        while let Some(l) = corner_table.swing_left(first_c) {
            if l == pos_left_most {
                break;
            }
            first_c = l;
        }

        let mut cur_vert_id = VertexIdx::from(vertex_to_left_most_corner.len());
        vertex_to_left_most_corner.push(first_c);
        let p = usize::from(ds.point_idx(first_c));
        safety_assert!(
            point_to_vertex_map[p] == VertexIdx::INVALID || point_to_vertex_map[p] == cur_vert_id,
            "point {} spans multiple attribute sectors; sort_mesh must have split it",
            p
        );
        point_to_vertex_map[p] = cur_vert_id;
        visited[usize::from(first_c)] = true;

        let mut prev_c = first_c;
        while let Some(curr) = pos_corner_table.swing_right(prev_c) {
            if curr == first_c {
                break;
            }
            visited[usize::from(curr)] = true;
            if corner_table.is_corner_opposite_to_seam_edge(curr.next()) {
                cur_vert_id = VertexIdx::from(vertex_to_left_most_corner.len());
                vertex_to_left_most_corner.push(curr);
            }
            let p = usize::from(ds.point_idx(curr));
            safety_assert!(
                point_to_vertex_map[p] == VertexIdx::INVALID
                    || point_to_vertex_map[p] == cur_vert_id,
                "point {} spans multiple attribute sectors; sort_mesh must have split it",
                p
            );
            point_to_vertex_map[p] = cur_vert_id;
            prev_c = curr;
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

/// Builds the global data structure and position corner table. Non-manifold
/// points are split first (`sort_mesh`), extending `attributes` in place with
/// aliasing points, so every downstream `point -> vertex` map is a function.
/// `attributes` must contain a position attribute.
pub(crate) fn build_global_ds(
    mut mesh_faces: Vec<[PointIdx; 3]>,
    attributes: &mut [Attribute],
) -> (DS, CornerTable) {
    let pos_att = attributes
        .iter()
        .find(|att| att.get_attribute_type() == AttributeType::Position)
        .expect("position attribute must be present");
    let pos_faces = mesh_faces
        .iter()
        .map(|face| {
            [
                usize::from(pos_att.get_unique_val_idx(face[0])).into(),
                usize::from(pos_att.get_unique_val_idx(face[1])).into(),
                usize::from(pos_att.get_unique_val_idx(face[2])).into(),
            ]
        })
        .collect::<Vec<[VertexIdx; 3]>>();

    sort_mesh(&pos_faces, &mut mesh_faces, attributes);

    let corner_table = compute_corner_table(&pos_faces);

    let corner_to_point_map: VecCornerIdx<PointIdx> = mesh_faces
        .iter()
        .flat_map(|face| face.iter().copied())
        .collect::<Vec<_>>()
        .into();

    let ds = DS::new(corner_to_point_map);

    (ds, corner_table)
}

/// Splits every non-manifold point before the corner table is built, so that
/// each point's corners form a single seam-connected sector and every
/// per-attribute `point -> vertex` map downstream is a function.
///
/// Two corners are joined iff they sit at the same endpoint of an edge that
/// is shared by exactly two faces, traversed in opposite directions by them
/// (consistent winding), and point-exact at both endpoints (point equality
/// subsumes every attribute's seams). The connected components of this
/// relation are the finest common refinement of all attributes' vertex
/// sectors. For each point, the component of its first corner (in corner
/// order, which makes the result deterministic) keeps the point; every other
/// component gets a fresh point aliasing the same values (`Attribute::mint`
/// on every attribute in lockstep). Splitting only adds value aliases, so the
/// encoded bitstream is unaffected.
fn sort_mesh(
    pos_faces: &[[VertexIdx; 3]],
    mesh_faces: &mut [[PointIdx; 3]],
    attributes: &mut [Attribute],
) {
    let num_corners = pos_faces.len() * 3;

    let mut parent: Vec<usize> = (0..num_corners).collect();

    for corners in edge_coboundary(pos_faces).values() {
        if corners.len() != 2 {
            continue;
        }
        let c1 = CornerIdx::from(corners[0]);
        let c2 = CornerIdx::from(corners[1]);
        if !edge_orientation_consistent(pos_faces, c1, c2) {
            continue;
        }
        // Consistent winding pairs the endpoints as c1.next() <-> c2.previous()
        // and c1.previous() <-> c2.next().
        let point = |c: CornerIdx| mesh_faces[usize::from(c) / 3][usize::from(c) % 3];
        if point(c1.next()) != point(c2.previous()) || point(c1.previous()) != point(c2.next()) {
            continue;
        }
        uf_union(
            &mut parent,
            usize::from(c1.next()),
            usize::from(c2.previous()),
        );
        uf_union(
            &mut parent,
            usize::from(c1.previous()),
            usize::from(c2.next()),
        );
    }

    debug_assert!(
        attributes.windows(2).all(|w| w[0].len() == w[1].len()),
        "attributes must share one point space"
    );
    let mut first_root: HashMap<PointIdx, usize> = HashMap::new();
    let mut minted: HashMap<(PointIdx, usize), PointIdx> = HashMap::new();
    for c in 0..num_corners {
        let root = uf_find(&mut parent, c);
        let p = mesh_faces[c / 3][c % 3];
        let owner = *first_root.entry(p).or_insert(root);
        if owner == root {
            continue;
        }
        let np = *minted.entry((p, root)).or_insert_with(|| {
            let mut np: Option<PointIdx> = None;
            for att in attributes.iter_mut() {
                let m = att.mint(p);
                match np {
                    Some(prev) => debug_assert_eq!(
                        prev, m,
                        "attributes must mint in lockstep so point spaces stay equal"
                    ),
                    None => np = Some(m),
                }
            }
            np.expect("attributes is non-empty")
        });
        mesh_faces[c / 3][c % 3] = np;
    }
}

fn uf_find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn uf_union(parent: &mut [usize], a: usize, b: usize) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra != rb {
        parent[ra] = rb;
    }
}

/// Groups the corners by the undirected position edge they are opposite to:
/// for each edge (a sorted vertex pair), the corners facing it. An edge's
/// group size is its face-adjacency count; exactly 2 means a manifold edge.
fn edge_coboundary(pos_faces: &[[VertexIdx; 3]]) -> HashMap<(usize, usize), Vec<usize>> {
    let pos_vertex = |c: CornerIdx| usize::from(pos_faces[usize::from(c) / 3][usize::from(c) % 3]);

    let mut coboundary: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for c in 0..pos_faces.len() * 3 {
        let c = CornerIdx::from(c);
        let next = pos_vertex(c.next());
        let prev = pos_vertex(c.previous());
        let entry = if next < prev {
            (next, prev)
        } else {
            (prev, next)
        };
        coboundary.entry(entry).or_default().push(c.into());
    }
    coboundary
}

/// Whether the two faces at corners `c` / `opp` (each opposite the same shared
/// edge) traverse that edge in opposite directions, i.e. their windings agree:
/// `c.next()` pairs with `opp.previous()` and `c.previous()` with `opp.next()`.
fn edge_orientation_consistent(pos_faces: &[[VertexIdx; 3]], c: CornerIdx, opp: CornerIdx) -> bool {
    let pos_vertex = |c: CornerIdx| pos_faces[usize::from(c) / 3][usize::from(c) % 3];
    pos_vertex(c.next()) == pos_vertex(opp.previous())
        && pos_vertex(c.previous()) == pos_vertex(opp.next())
}

/// Builds the corner table for the given position faces: opposite corners are
/// linked across every edge shared by exactly two consistently-wound faces;
/// every other edge is a boundary. Non-manifold points must have been split
/// beforehand (`sort_mesh`).
fn compute_corner_table(pos_faces: &[[VertexIdx; 3]]) -> CornerTable {
    let num_corners = pos_faces.len() * 3;

    let mut opposite: Vec<Option<CornerIdx>> = vec![None; num_corners];

    for corners in edge_coboundary(pos_faces).values() {
        if corners.len() != 2 {
            continue;
        }
        let c1 = CornerIdx::from(corners[0]);
        let c2 = CornerIdx::from(corners[1]);
        opposite[corners[0]] = Some(c2);
        opposite[corners[1]] = Some(c1);
    }

    cut_orientation_seams(&mut opposite, pos_faces);

    CornerTable::from_opposites(opposite)
}

/// Cuts the opposite relation at every edge whose two incident faces disagree
/// on winding, so each connected component becomes a consistently-oriented
/// manifold-with-boundary without any triangle being rewound. Rewinding would
/// flip a face's geometric normal away from its stored normal attribute,
/// which normal prediction relies on; a non-orientable input instead falls
/// apart into oriented patches.
fn cut_orientation_seams(corner_table: &mut [Option<CornerIdx>], pos_faces: &[[VertexIdx; 3]]) {
    for c in 0..pos_faces.len() * 3 {
        let Some(opp_c) = corner_table[c] else {
            continue;
        };
        if usize::from(opp_c) < c {
            continue;
        }
        let c = CornerIdx::from(c);
        if !edge_orientation_consistent(pos_faces, c, opp_c) {
            corner_table[usize::from(c)] = None;
            corner_table[usize::from(opp_c)] = None;
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

    /// A textbook tetrahedron: 4 distinct positions, 4 consistently-oriented
    /// faces, fully manifold, no duplicate points. `build_global_ds` must
    /// produce exactly 4 points (an identity point map) and mint nothing.
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

        assert_eq!(
            ds.num_points(),
            4,
            "closed manifold tetrahedron must not mint points"
        );

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
        assert_eq!(ct.opposite(CornerIdx::from(0)), Some(CornerIdx::from(5)));
        assert_eq!(ct.opposite(CornerIdx::from(5)), Some(CornerIdx::from(0)));
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
    /// shared position must be split into two vertices, one per fan, so the
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

    /// A genuinely NON-ORIENTABLE surface. Of the 5 core edges only the twist
    /// edge has its two faces wound the same way, so `cut_orientation_seams`
    /// cuts exactly that edge and `build_global_ds` completes: 4 core edges stay
    /// linked (8 corners) and the twist edge is a boundary.
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

    /// A genuinely NON-ORIENTABLE surface builds its attribute DS. The twist
    /// edge is a winding disagreement, so `sort_mesh` treats it as a cut: each
    /// endpoint's corners fall into two components, and the second component
    /// gets a fresh point aliasing the value it was split from. The 5 original
    /// points grow to 7 (one alias per cut endpoint), the attribute is
    /// extended to match, and the `corner -> point -> vertex -> value` chain
    /// is total.
    #[test]
    fn build_attribute_ds_handles_non_orientable_surface() {
        let mut attributes = mobius_positions();
        let (ds, ct) = build_global_ds(mobius_band(), &mut attributes);

        assert_eq!(ds.num_points(), 7, "two aliasing points from the cut");

        let adss = build_attribute_ds(&ds, &ct, attributes);
        let pos = &adss[0];

        // The attribute was extended to cover the minted points, so the value
        // lookup is total over every point.
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
    /// triangles form a quad sharing edge {1,2}, but face 1 ([1,2,3]) traverses
    /// that edge in the SAME direction as face 0, an orientation disagreement.
    /// The edge is cut (never any face rewound), so the quad splits at the two
    /// shared vertices: 4 points grow to 6 and no interior edge stays linked.
    #[test]
    fn build_global_ds_cuts_inconsistent_orientable_quad() {
        let mut attributes = vec![pos_attribute_2d(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
        ])];
        let (ds, ct) = build_global_ds(faces(vec![[0, 1, 2], [1, 2, 3]]), &mut attributes);

        // The single interior edge disagrees, so it is cut; its two endpoints
        // become non-manifold and are split, growing 4 points to 6.
        assert_eq!(ds.num_points(), 6);
        // No interior edge remains linked.
        let linked = (0..ds.num_corners())
            .filter(|&c| ct.opposite(CornerIdx::from(c)).is_some())
            .count();
        assert_eq!(linked, 0);

        let adss = build_attribute_ds(&ds, &ct, attributes);
        assert_eq!(adss[0].num_vertices(), 6);
    }

    /// An ORIENTABLE 3-triangle strip with its END triangle wound the wrong way
    /// ([3,2,4] disagrees with its neighbour across edge {2,3}, while edge {1,2}
    /// agrees). Only the disagreeing edge is cut, so the strip splits at that
    /// edge's two endpoints: 5 points grow to 7, and only the one consistent
    /// interior edge ({1,2}) stays linked.
    #[test]
    fn build_global_ds_cuts_inconsistent_orientable_strip() {
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

        assert_eq!(ds.num_points(), 7);
        // Only the consistent interior edge ({1,2}) stays linked = two corners.
        let linked = (0..ds.num_corners())
            .filter(|&c| ct.opposite(CornerIdx::from(c)).is_some())
            .count();
        assert_eq!(linked, 2);
        let adss = build_attribute_ds(&ds, &ct, attributes);
        assert_eq!(adss[0].num_vertices(), 7);
    }
}
