//! Edgebreaker connectivity decode: counts, topology splits, and the driver that
//! ties traversal, corner-table reconstruction, and point assignment together.

mod reconstruct;
mod traversal;

use super::Connectivity;
use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::mesh::ds::CornerTable;
use draco_oxide_core::utils::bit_coder::leb128_read;

use reconstruct::reconstruct;
use traversal::{
    decode_topology_splits, StandardTraversalDecoder, TopologySplit, TraversalDecoder,
    ValenceTraversalDecoder,
};

/// The edgebreaker traversal type ids on the wire.
const TRAVERSAL_STANDARD: u8 = 0;
const TRAVERSAL_VALENCE: u8 = 2;

/// The edgebreaker connectivity header counts, read before the traversal payload.
struct Counts {
    num_encoded_vertices: usize,
    num_faces: usize,
    num_attribute_data: usize,
    num_encoded_symbols: usize,
    num_encoded_split_symbols: usize,
}

/// Decodes edgebreaker connectivity from `reader`, positioned just after the header
/// (and metadata). Leaves the reader at the start of the attribute section. Handles
/// the standard and valence traversals; any other traversal id is unimplemented.
pub fn decode(reader: &mut Reader<'_>) -> Result<Connectivity, Err> {
    let traversal_type = reader.read_u8()?;

    let counts = Counts {
        num_encoded_vertices: leb128_read(reader)? as usize,
        num_faces: leb128_read(reader)? as usize,
        num_attribute_data: reader.read_u8()? as usize,
        num_encoded_symbols: leb128_read(reader)? as usize,
        num_encoded_split_symbols: leb128_read(reader)? as usize,
    };

    let splits = decode_topology_splits(reader)?;
    let max_num_vertices = counts.num_encoded_vertices + counts.num_encoded_split_symbols;

    // Each arm monomorphizes reconstruction over its concrete traversal.
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

/// Reconstructs the corner table and attribute seams for an already-started
/// traversal, producing the connectivity. Generic over the traversal variant so
/// its per-symbol decode is monomorphized.
fn decode_with<T: TraversalDecoder>(
    mut traversal: T,
    counts: &Counts,
    splits: Vec<TopologySplit>,
) -> Result<Connectivity, Err> {
    let recon = reconstruct(
        &mut traversal,
        counts.num_encoded_symbols,
        counts.num_encoded_vertices,
        counts.num_encoded_split_symbols,
        counts.num_faces,
        counts.num_attribute_data,
        splits,
    )?;

    let attribute_seams = traversal.decode_attribute_seams(&recon.opposite, counts.num_faces);

    Ok(Connectivity {
        corner_table: CornerTable::from_opposite_sentinels(recon.opposite),
        corner_to_vertex: recon.corner_to_vertex,
        vertex_corners: recon.vertex_corners,
        num_vertices: recon.num_vertices,
        num_faces: counts.num_faces,
        num_attribute_data: counts.num_attribute_data,
        is_vert_hole: recon.is_vert_hole,
        init_corners: recon.init_corners,
        attribute_seams,
    })
}
