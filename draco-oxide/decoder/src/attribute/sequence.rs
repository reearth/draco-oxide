//! Builds the per-attribute `AttributeDS` over the decoded connectivity from the
//! precomputed fan vertex maps, and produces the traversal seeds that reproduce
//! the encoder's visit order via core's `Traverser`.

use crate::connectivity::points::FanVertices;
use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::mesh::ds::{AttributeCornerTable, AttributeDS, DS};
use draco_oxide_core::types::{CornerIdx, VecPointIdx, VecVertexIdx, VertexIdx};

/// Builds the attribute data structure for one attribute from the decoded
/// connectivity and its fan vertex map, mirroring the encoder's
/// `build_single_attribute_ds`. `att` is the (typically still empty) attribute
/// the traversal and prediction stages will fill.
pub(crate) fn build_ads<'a>(
    ds: &'a DS,
    act: AttributeCornerTable<'a>,
    fan_vertices: FanVertices,
    att: Attribute,
) -> AttributeDS<'a> {
    let num_corners = ds.num_corners();

    // Points were assigned as the finest refinement of all attributes' sectors,
    // so every point lies in exactly one sector of this attribute and the
    // point-to-vertex map is a function.
    let mut point_to_vertex = vec![VertexIdx::from(usize::MAX); ds.num_points()];
    for c in 0..num_corners {
        let c = CornerIdx::from(c);
        point_to_vertex[usize::from(ds.point_idx(c))] =
            VertexIdx::from(fan_vertices.corner_to_vertex[usize::from(c)]);
    }

    AttributeDS::new(
        ds,
        act,
        VecVertexIdx::from(fan_vertices.vertex_to_left_most_corner),
        VecPointIdx::from(point_to_vertex),
        att,
    )
}

/// The traversal seed stack replicating the reference decoder's sequencing: every
/// face in decode order, seeded at its first corner. The `Traverser` pops from
/// the back, so the corners are stacked in reverse.
pub(crate) fn traversal_seeds(num_faces: usize) -> Vec<CornerIdx> {
    (0..num_faces)
        .rev()
        .map(|f| CornerIdx::from(3 * f))
        .collect()
}
