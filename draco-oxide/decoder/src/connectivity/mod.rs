//! Connectivity decoding, dispatched on the encoder method.

pub mod edgebreaker;
mod sequential;

use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::header::EncoderMethod;
use draco_oxide_core::mesh::ds::{AttributeCornerTable, CornerTable};
use draco_oxide_core::types::{CornerIdx, PointIdx, VecCornerIdx, VertexIdx};
use std::collections::HashMap;

/// The decoded connectivity.
pub enum Connectivity {
    /// Connectivity decoded from an edgebreaker-encoded stream.
    Edgebreaker(EdgebreakerConnectivity),
    /// Connectivity decoded from a sequentially encoded stream.
    Sequential(SequentialConnectivity),
}

impl Connectivity {
    /// The corner-table connectivity, if edgebreaker-encoded.
    pub fn edgebreaker(&self) -> Option<&EdgebreakerConnectivity> {
        match self {
            Connectivity::Edgebreaker(conn) => Some(conn),
            Connectivity::Sequential(_) => None,
        }
    }
}

/// Face indices over a flat point space.
pub struct SequentialConnectivity {
    /// The faces as point-index triples.
    pub faces: Vec<[PointIdx; 3]>,
    /// The number of points the faces index into.
    pub num_points: usize,
}

/// The decoded position connectivity plus what the attribute stages need.
pub struct EdgebreakerConnectivity {
    /// The opposite-corner table over the reconstructed faces.
    pub corner_table: CornerTable,
    /// The position vertex of each corner, indexed by corner.
    pub corner_to_vertex: Vec<VertexIdx>,
    /// Left-most corner per vertex; boundary-left-most for hole vertices.
    pub vertex_corners: Vec<CornerIdx>,
    /// The number of position vertices.
    pub num_vertices: usize,
    /// The number of faces.
    pub num_faces: usize,
    /// The number of attribute connectivity streams (attribute corner tables).
    pub num_attribute_data: usize,
    /// Per vertex, whether the vertex lies on a boundary hole.
    pub is_vert_hole: Vec<bool>,
    /// The initial corner of each edgebreaker traversal component.
    pub init_corners: Vec<CornerIdx>,
    /// Packed seams, one byte per corner: bit `i` set when the edge opposite
    /// the corner is a seam of stream `i`.
    pub seam_bits: Vec<u8>,
    /// Seam statistics per stream, parallel to `seam_bits` bits.
    pub seam_stats: Vec<edgebreaker::SeamStats>,
}

impl EdgebreakerConnectivity {
    /// Faces as densely relabeled position-vertex triples.
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

    /// The attribute corner table for stream `i`.
    pub fn attribute_corner_table(&self, i: usize) -> AttributeCornerTable<'_> {
        let seams: Vec<bool> = self.seam_bits.iter().map(|&b| b & (1 << i) != 0).collect();
        AttributeCornerTable::new(&self.corner_table, VecCornerIdx::from(seams))
    }
}

/// Decodes the connectivity section.
pub fn decode_connectivity(
    reader: &mut Reader<'_>,
    encoder_method: EncoderMethod,
) -> Result<Connectivity, Err> {
    match encoder_method {
        EncoderMethod::Edgebreaker => Ok(Connectivity::Edgebreaker(edgebreaker::decode(reader)?)),
        EncoderMethod::Sequential => Ok(Connectivity::Sequential(sequential::decode(reader)?)),
    }
}
