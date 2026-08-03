//! Edgebreaker connectivity decode.

mod reconstruct;
mod traversal;

use super::EdgebreakerConnectivity;
use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::mesh::ds::CornerTable;
use draco_oxide_core::utils::bit_coder::leb128_read;

use reconstruct::reconstruct;
pub use traversal::SeamStats;
use traversal::{
    decode_topology_splits, StandardTraversalDecoder, TopologySplit, TraversalDecoder,
    ValenceTraversalDecoder,
};

/// The traversal type ids on the wire.
const TRAVERSAL_STANDARD: u8 = 0;
const TRAVERSAL_VALENCE: u8 = 2;

/// The connectivity header counts.
struct Counts {
    num_encoded_vertices: usize,
    num_faces: usize,
    num_attribute_data: usize,
    num_encoded_symbols: usize,
    num_encoded_split_symbols: usize,
}

/// Decodes edgebreaker connectivity, leaving the reader at the attribute
/// section.
pub fn decode(reader: &mut Reader<'_>) -> Result<EdgebreakerConnectivity, Err> {
    let traversal_type = reader.read_u8()?;

    let counts = Counts {
        num_encoded_vertices: leb128_read(reader)? as usize,
        num_faces: leb128_read(reader)? as usize,
        num_attribute_data: reader.read_u8()? as usize,
        num_encoded_symbols: leb128_read(reader)? as usize,
        num_encoded_split_symbols: leb128_read(reader)? as usize,
    };
    // The seam flags of every stream pack into one byte per corner.
    if counts.num_attribute_data > 8 {
        return Err(Err::Unimplemented);
    }

    let splits = decode_topology_splits(reader)?;
    let max_num_vertices = counts.num_encoded_vertices + counts.num_encoded_split_symbols;

    match traversal_type {
        TRAVERSAL_STANDARD => {
            let traversal = StandardTraversalDecoder::start(reader, counts.num_attribute_data)?;
            decode_with(traversal, &counts, splits)
        }
        TRAVERSAL_VALENCE => {
            let traversal = ValenceTraversalDecoder::start(
                reader,
                counts.num_attribute_data,
                max_num_vertices,
                counts.num_faces,
            )?;
            decode_with(traversal, &counts, splits)
        }
        _ => Err(Err::Unimplemented),
    }
}

/// Reconstructs the corner table and seams, monomorphized per traversal.
fn decode_with<T: TraversalDecoder>(
    mut traversal: T,
    counts: &Counts,
    splits: Vec<TopologySplit>,
) -> Result<EdgebreakerConnectivity, Err> {
    let recon = reconstruct(
        &mut traversal,
        counts.num_encoded_symbols,
        counts.num_encoded_vertices,
        counts.num_encoded_split_symbols,
        counts.num_faces,
        counts.num_attribute_data,
        splits,
    )?;

    let (seam_bits, seam_stats) = traversal.decode_attribute_seams(
        &recon.opposite,
        counts.num_faces,
        &recon.corner_to_vertex,
        recon.vertex_corners.len(),
    );

    Ok(EdgebreakerConnectivity {
        corner_table: CornerTable::from_opposite_sentinels(recon.opposite),
        corner_to_vertex: recon.corner_to_vertex,
        vertex_corners: recon.vertex_corners,
        num_vertices: recon.num_vertices,
        num_faces: counts.num_faces,
        num_attribute_data: counts.num_attribute_data,
        is_vert_hole: recon.is_vert_hole,
        init_corners: recon.init_corners,
        seam_bits,
        seam_stats,
    })
}
