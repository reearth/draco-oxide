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

pub(crate) const SYMBOL_ENCODING_CONFIG_SLOT: u8 = 4;
pub(crate) const NUM_CONNECTED_COMPONENTS_SLOT: u8 = 8;
pub(crate) const NUM_FACES_SLOT: u8 = 32;
pub(crate) const HOLE_SLOT_SIZE: u8 = 2;
pub(crate) const HANDLE_SLOT_SIZE: u8 = 2;
pub(crate) const NUM_VERTICES_IN_HOLE_SLOTS: [u8;4] = [8,12,16,20];
pub(crate) const HANDLE_METADATA_SLOTS: [u8;4] = [8,12,16,20];