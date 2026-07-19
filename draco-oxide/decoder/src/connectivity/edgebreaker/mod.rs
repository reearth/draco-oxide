//! Edgebreaker connectivity decode: counts, topology splits, and the driver that
//! ties traversal, corner-table reconstruction, and point assignment together.

mod points;
mod reconstruct;
mod traversal;

use super::Connectivity;
use crate::Err;
use draco_oxide_core::bit_coder::ByteReader;
use draco_oxide_core::mesh::ds::CornerTable;
use draco_oxide_core::types::{CornerIdx, VecCornerIdx};
use draco_oxide_core::utils::bit_coder::leb128_read;

use reconstruct::reconstruct;
use traversal::{decode_topology_splits, TraversalDecoder};

/// The standard edgebreaker traversal type id.
const TRAVERSAL_STANDARD: u8 = 0;

/// Decodes edgebreaker connectivity from `reader`, positioned just after the header
/// (and metadata). Leaves the reader at the start of the attribute section.
pub fn decode<R: ByteReader>(reader: &mut R) -> Result<Connectivity, Err> {
    let traversal_type = reader.read_u8()?;
    if traversal_type != TRAVERSAL_STANDARD {
        // Valence/predictive traversal decode arrives with Google interop.
        return Err(Err::Unimplemented);
    }

    let num_encoded_vertices = leb128_read(reader)? as usize;
    let num_faces = leb128_read(reader)? as usize;
    let num_attribute_data = reader.read_u8()? as usize;
    let num_encoded_symbols = leb128_read(reader)? as usize;
    let num_encoded_split_symbols = leb128_read(reader)? as usize;

    let splits = decode_topology_splits(reader)?;

    let mut traversal = TraversalDecoder::start(reader, num_attribute_data)?;

    let recon = reconstruct(
        &mut traversal,
        num_encoded_symbols,
        num_encoded_vertices,
        num_encoded_split_symbols,
        num_faces,
        num_attribute_data,
        splits,
    )?;

    let attribute_seams = decode_attribute_seams(
        &recon.opposite,
        num_faces,
        num_attribute_data,
        &mut traversal,
    );

    Ok(Connectivity {
        corner_table: CornerTable::from_raw_data(VecCornerIdx::from(recon.opposite)),
        corner_to_vertex: recon.corner_to_vertex,
        num_vertices: recon.num_vertices,
        num_faces,
        num_attribute_data,
        is_vert_hole: recon.is_vert_hole,
        init_corners: recon.init_corners,
        attribute_seams,
    })
}

/// Decodes the per-attribute seam edges (port of Google's
/// `DecodeAttributeConnectivitiesOnFace`, run over every face in order). Returns,
/// per attribute, the `is_edge_on_seam` flag for each corner: a boundary edge is an
/// automatic seam for every attribute and reads no bit; an interior edge decodes one
/// seam bit per attribute, processed once from its lower-id face. Both corners of a
/// seam edge are marked, matching `AddSeamEdge`.
fn decode_attribute_seams(
    opposite: &[CornerIdx],
    num_faces: usize,
    num_attribute_data: usize,
    traversal: &mut TraversalDecoder,
) -> Vec<Vec<bool>> {
    let num_corners = num_faces * 3;
    let mut is_seam = vec![vec![false; num_corners]; num_attribute_data];
    if num_attribute_data == 0 {
        return is_seam;
    }

    let mark = |seam: &mut [bool], c: CornerIdx| {
        seam[usize::from(c)] = true;
        let opp = opposite[usize::from(c)];
        if opp.is_some() {
            seam[usize::from(opp)] = true;
        }
    };

    for f in 0..num_faces {
        let corner = CornerIdx::from(3 * f);
        for c in [corner, corner.next(), corner.previous()] {
            let opp = opposite[usize::from(c)];
            if opp.is_none() {
                // Boundary edge: an automatic seam for every attribute, no bit.
                for seam in is_seam.iter_mut() {
                    mark(seam, c);
                }
                continue;
            }
            // Each shared edge is decoded once, from its lower-id face.
            if usize::from(opp.face_idx()) < f {
                continue;
            }
            for (i, seam) in is_seam.iter_mut().enumerate() {
                if traversal.decode_attribute_seam(i) {
                    mark(seam, c);
                }
            }
        }
    }

    is_seam
}
