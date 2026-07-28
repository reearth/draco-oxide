//! The traversal seeds that reproduce the encoder's visit order via core's
//! `Traverser`.

use draco_oxide_core::types::CornerIdx;

/// The traversal seed stack replicating the reference decoder's sequencing: every
/// face in decode order, seeded at its first corner. The `Traverser` pops from
/// the back, so the corners are stacked in reverse.
pub(crate) fn traversal_seeds(num_faces: usize) -> Vec<CornerIdx> {
    (0..num_faces)
        .rev()
        .map(|f| CornerIdx::from(3 * f))
        .collect()
}
