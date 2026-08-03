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
    Edgebreaker(EdgebreakerConnectivity),
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
    pub faces: Vec<[PointIdx; 3]>,
    pub num_points: usize,
}

/// The decoded position connectivity plus what the attribute stages need.
pub struct EdgebreakerConnectivity {
    pub corner_table: CornerTable,
    pub corner_to_vertex: Vec<VertexIdx>,
    /// Left-most corner per vertex; boundary-left-most for hole vertices.
    pub vertex_corners: Vec<CornerIdx>,
    pub num_vertices: usize,
    pub num_faces: usize,
    pub num_attribute_data: usize,
    pub is_vert_hole: Vec<bool>,
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
