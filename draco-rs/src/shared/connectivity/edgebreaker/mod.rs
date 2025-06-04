use crate::core::shared::VertexIdx;

pub mod symbol_encoder;
pub mod prediction;

pub(crate) fn orientation_of_next_face(prev_face: [usize;3], prev_orientation: bool, common_edge: [usize;2], next_face: [usize; 3]) -> bool {
    let face_orientation_on_edge = |edge: [usize;2], face: [usize;3]| -> bool {
        debug_assert!(edge.iter().all(|v| face.contains(v)), "edge: {:?}, face: {:?}", edge, face);
        !(face[0]==edge[0] && face[2]==edge[1])
    };

    if face_orientation_on_edge(common_edge, prev_face) ^ face_orientation_on_edge(common_edge, next_face) {
        prev_orientation
    } else {
        !prev_orientation
    }
}


/// returns the sign of 'face' as boolean ('true' if + and 'false' if -), where the sign of 'face'
/// is defined as the sign of the permutation of the vertices of 'face' with respect to the
/// sorted 'face'.
pub(crate) fn sign_of(face: [VertexIdx; 3]) -> bool {
    // check that the indices are distinct.
    debug_assert!(face[0] != face[1] && face[0] != face[2] && face[1] != face[2]);
    let min_idx = (0..3).min_by_key(|&i| face[i]).unwrap();
    let second = (min_idx+1)%3;
    let third = (min_idx+2)%3;
    face[second] < face[third]
}

// Tells if two faces share an edge, and return the edge if any.
pub(crate) fn edge_shared_by(f1: &[usize; 3], f2: &[usize; 3]) -> Option<[usize; 2]> {
    // ToDo: This can be optimized as faces are sorted.
    let maybe_edge = f1.iter().filter(|v| f2.contains(v)).collect::<Vec<_>>();
    if maybe_edge.len() == 2 {
        Some([*maybe_edge[0], *maybe_edge[1]])
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TopologySplit {
    pub source_symbol_idx: usize,
    pub split_symbol_idx: usize,
    pub source_edge_orientation: Orientation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Orientation {
    Left,
    Right,
}

pub(crate) enum Traversal {
    
}

pub(crate) const SYMBOL_ENCODING_CONFIG_SLOT: u8 = 4;
pub(crate) const NUM_CONNECTED_COMPONENTS_SLOT: u8 = 8;
pub(crate) const NUM_FACES_SLOT: u8 = 32;
pub(crate) const HOLE_SLOT_SIZE: u8 = 2;
pub(crate) const HANDLE_SLOT_SIZE: u8 = 2;
pub(crate) const NUM_VERTICES_IN_HOLE_SLOTS: [u8;4] = [8,12,16,20];
pub(crate) const HANDLE_METADATA_SLOTS: [u8;4] = [8,12,16,20];