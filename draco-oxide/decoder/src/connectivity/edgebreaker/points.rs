//! Point-id assignment across the per-attribute corner tables, and the shared
//! position-fan walker used to rebuild the per-attribute vertex maps.

use draco_oxide_core::mesh::ds::{AttributeCornerTable, CornerTable, GenericCornerTable};
use draco_oxide_core::types::{CornerIdx, PointIdx, VecCornerIdx};

/// The seam-separated corner groups of an attribute corner table: per corner its
/// group id, and per group its left-most corner. Groups are discovered by walking
/// each position fan from its left-most corner and starting a new group at every
/// seam crossing, mirroring the encoder's vertex construction so both sides agree
/// on which corners share an attribute value.
pub(crate) struct FanGroups {
    pub corner_to_group: Vec<usize>,
    pub group_to_left_most_corner: Vec<CornerIdx>,
}

/// Walks every position fan of `pos_ct`, splitting it into the sectors separated
/// by the seams of `act`.
pub(crate) fn fan_groups(
    pos_ct: &CornerTable,
    act: &AttributeCornerTable,
    num_corners: usize,
) -> FanGroups {
    let mut visited = vec![false; num_corners];
    let mut corner_to_group = vec![usize::MAX; num_corners];
    let mut group_to_left_most_corner: Vec<CornerIdx> = Vec::new();

    for start in 0..num_corners {
        if visited[start] {
            continue;
        }
        let start = CornerIdx::from(start);

        // Walk to the position-fan left-most corner (boundary or full circle).
        let mut pos_left_most = start;
        loop {
            let l = pos_ct.swing_left(pos_left_most);
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

        // Walk further to the attribute-sector left-most corner.
        let mut first_c = pos_left_most;
        loop {
            let l = act.swing_left(first_c);
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

        let mut cur_group = group_to_left_most_corner.len();
        group_to_left_most_corner.push(first_c);
        corner_to_group[usize::from(first_c)] = cur_group;
        visited[usize::from(first_c)] = true;

        // Sweep right across the whole position fan, starting a new group at
        // every seam crossing.
        let mut maybe_curr = pos_ct.swing_right(first_c);
        while maybe_curr.is_some() {
            let curr = maybe_curr;
            if curr == first_c {
                break;
            }
            visited[usize::from(curr)] = true;
            if act.is_corner_opposite_to_seam_edge(curr.next()) {
                cur_group = group_to_left_most_corner.len();
                group_to_left_most_corner.push(curr);
            }
            corner_to_group[usize::from(curr)] = cur_group;
            maybe_curr = pos_ct.swing_right(curr);
        }
    }

    FanGroups {
        corner_to_group,
        group_to_left_most_corner,
    }
}

/// Assigns a point id to every corner. Points are the finest common refinement of
/// all attributes' seam sectors over each position fan (the decoder-side
/// equivalent of Google's `AssignPointsToCorners`): corners of one fan share a
/// point exactly when no attribute separates them by a seam.
pub(crate) fn assign_points(
    pos_ct: &CornerTable,
    num_corners: usize,
    attribute_seams: &[Vec<bool>],
) -> VecCornerIdx<PointIdx> {
    // The union of every attribute's seams; boundary edges are seams for every
    // attribute already, so an empty attribute list leaves plain position fans.
    let mut union_seams = vec![false; num_corners];
    for seams in attribute_seams {
        for (u, &s) in union_seams.iter_mut().zip(seams.iter()) {
            *u |= s;
        }
    }

    let act = AttributeCornerTable::new(pos_ct, VecCornerIdx::from(union_seams));
    let groups = fan_groups(pos_ct, &act, num_corners);
    groups
        .corner_to_group
        .into_iter()
        .map(PointIdx::from)
        .collect::<Vec<_>>()
        .into()
}
