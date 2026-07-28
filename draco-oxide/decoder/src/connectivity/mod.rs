//! Connectivity decoding, dispatched on the encoder method. Only edgebreaker is
//! handled; sequential connectivity is not implemented.

mod edgebreaker;

use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::header::EncoderMethod;
use draco_oxide_core::mesh::ds::{AttributeCornerTable, CornerTable};
use draco_oxide_core::types::{CornerIdx, VecCornerIdx, VertexIdx};
use std::collections::HashMap;

/// The decoded position connectivity plus the data the attribute stages need.
pub struct Connectivity {
    /// Position corner table (opposite relation over `num_faces * 3` corners).
    pub corner_table: CornerTable,
    /// Per-corner position vertex.
    pub corner_to_vertex: Vec<VertexIdx>,
    /// Left-most corner per position vertex (`CornerIdx::INVALID` for isolated
    /// vertices); a per-fan seed reused by point assignment. Boundary-left-most
    /// for hole vertices, an arbitrary incident corner otherwise.
    pub vertex_corners: Vec<CornerIdx>,
    /// Number of position vertices (compacted when there is no attribute data).
    pub num_vertices: usize,
    /// Number of faces.
    pub num_faces: usize,
    /// Number of non-position attribute connectivities carried by the stream.
    pub num_attribute_data: usize,
    /// Per-vertex boundary/hole flag (pre-compaction vertex ids).
    pub is_vert_hole: Vec<bool>,
    /// Seed corners of each component, in start-face resolution order.
    pub init_corners: Vec<CornerIdx>,
    /// Per non-position attribute, the decoded `is_edge_on_seam` flag per corner
    /// (true when the edge opposite the corner is an attribute seam). Indexed
    /// `[attribute][corner]`, in the stream's attribute order.
    pub attribute_seams: Vec<Vec<bool>>,
}

impl Connectivity {
    /// Faces as position-vertex triples, densely relabeled so referenced vertices
    /// occupy `0..k`. Returns the faces and `k`.
    pub fn position_faces(&self) -> (Vec<[usize; 3]>, usize) {
        let mut faces = Vec::with_capacity(self.num_faces);
        for f in 0..self.num_faces {
            faces.push([
                usize::from(self.corner_to_vertex[3 * f]),
                usize::from(self.corner_to_vertex[3 * f + 1]),
                usize::from(self.corner_to_vertex[3 * f + 2]),
            ]);
        }

        let mut remap: HashMap<usize, usize> = HashMap::new();
        for face in &mut faces {
            for v in face.iter_mut() {
                let next = remap.len();
                *v = *remap.entry(*v).or_insert(next);
            }
        }
        (faces, remap.len())
    }

    /// The attribute corner table for non-position attribute `i`, built from the
    /// decoded seam edges over the position corner table.
    pub fn attribute_corner_table(&self, i: usize) -> AttributeCornerTable<'_> {
        AttributeCornerTable::new(
            &self.corner_table,
            VecCornerIdx::from(self.attribute_seams[i].clone()),
        )
    }
}

/// Decodes the connectivity section, dispatching on the header's encoder method.
pub fn decode_connectivity(
    reader: &mut Reader<'_>,
    encoder_method: EncoderMethod,
) -> Result<Connectivity, Err> {
    match encoder_method {
        EncoderMethod::Edgebreaker => edgebreaker::decode(reader),
        // Google falls back to sequential for tiny meshes; decode arrives in B.
        EncoderMethod::Sequential => Err(Err::Unimplemented),
    }
}
