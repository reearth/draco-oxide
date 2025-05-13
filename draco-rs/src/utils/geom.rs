use crate::core::shared::{
    NdVector,
    Float,
    Cross,
    Dot,
};

/// Calculates the distance from a point to a triangle in 3D space.
pub fn point_to_face_distance_3d<F: Float>(p: NdVector<3, F>, face: [NdVector<3,F>; 3]) -> F {
    let x = face[1] - face[0];
    let y = face[2] - face[0];
    let n = x.cross(y).normalize();
    let distance_to_plane = n.dot(p-face[0]).abs();

    let p_onto_plane = p - n * distance_to_plane;
    let p_onto_plane_inside_face = 
        (p_onto_plane - face[0]).dot(face[1] - face[0]) * (face[2] - face[0]).dot(face[1] - face[0]) > F::zero() &&
        (p_onto_plane - face[1]).dot(face[2] - face[1]) * (face[0] - face[1]).dot(face[2] - face[1]) > F::zero() &&
        (p_onto_plane - face[2]).dot(face[0] - face[2]) * (face[1] - face[2]).dot(face[0] - face[2]) > F::zero();
 
    if p_onto_plane_inside_face {
        distance_to_plane
    } else {
        [
            point_to_line_distance_3d(p, [face[0], face[1]]),
            point_to_line_distance_3d(p, [face[1], face[2]]),
            point_to_line_distance_3d(p, [face[2], face[0]]),
            (face[1]-face[0]).norm(),
            (face[2]-face[1]).norm(),
            (face[0]-face[2]).norm()
        ].into_iter().min_by(|a,b| a.partial_cmp(b).unwrap()).unwrap()
    }
}

/// Calculates the distance from a point to a line in 3D space.
/// The line should be expressed as two points in 3D space.
pub fn point_to_line_distance_3d<F: Float>(p: NdVector<3, F>, line: [NdVector<3,F>; 2]) -> F {
    let dir = (line[1] - line[0]).normalize();
    let p_line0 = p - line[0];
    let n = (p_line0 - dir * p_line0.dot(dir)).normalize();
    debug_assert!(n.dot(dir).abs() < F::from_f64(1e-6));
    n.dot(p_line0).abs()
}