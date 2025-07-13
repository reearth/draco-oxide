use std::collections::HashMap;
use std::{cmp, vec, mem};
use std::io::Read;
use crate::core::bit_coder::ReaderErr;
use crate::core::corner_table::CornerTable;
use crate::decode::entropy::rans::{self, RabsDecoder, RansDecoder};
use crate::eval::ConnectivityEncoder;
use crate::{debug_expect, shared};
use crate::decode::connectivity::ConnectivityDecoder;
use crate::core::shared::VertexIdx;
use crate::shared::attribute::Portable;

use crate::prelude::{BitReader, ByteReader};
use crate::shared::connectivity::edgebreaker::symbol_encoder::{
    CrLight, Rans, Symbol, SymbolEncoder, SymbolEncodingConfig
};
use crate::utils::bit_coder::leb128_read;

use crate::shared::connectivity::edgebreaker::{
    edge_shared_by, orientation_of_next_face, Orientation, TopologySplit, Traversal
};

const MIN_VALENCE: usize = 2;
const MAX_VALENCE: usize = 7;
const NUM_UNIQUE_VALENCES: usize = 6;

#[derive(thiserror::Error, Debug)]
#[remain::sorted]
pub enum Err {
    #[error("Not enough data to decode connectivity")]
    NotEnoughData(#[from] ReaderErr),
    #[error("Rans decoder error")]
    RansDecoder(#[from] rans::Err),
    #[error("Shared Edgebreaker error")]
    SharedEdgebreaker(#[from] shared::connectivity::edgebreaker::Err),

}


pub(crate)struct SpiraleReversi {
    faces: Vec<[VertexIdx; 3]>,
    num_connected_components: usize,
    num_decoded_vertices: usize,
    // active edge is oriented from right to left.
    active_edge: [usize; 2],
    active_edge_stack: Vec<[usize; 2]>,
    boundary_edges: Vec<[usize; 2]>,
    prev_face: [usize;3],
    orientation: Vec<bool>,
    topology_splits: Vec<TopologySplit>,
    last_symbol: Option<Symbol>,
    active_context: Option<usize>,
    symbol_buffer: Vec<u8>, 
    standard_face_data: Vec<u8>,
    standard_attribute_connectivity_data: Vec<u8>,
    vertex_valences: Vec<usize>,
    is_vert_hole: Vec<bool>,
    corner_to_vertex_map: Vec<Vec<VertexIdx>>,
    vertex_corners: Vec<usize>,
    
    start_face_buffer_prob_zero: u8,

    corner_table: CornerTable,
    last_vert_added: isize,

    traversal_type: Traversal,
    num_vertices: usize,
    num_faces: usize,
    num_attribute_data: usize,
    num_encoded_symbols: usize,
    num_encoded_split_symbols: usize,

    curr_att_dec: usize,

    att_dec_types: Vec<shared::attribute::AttributeKind>,
    is_edge_on_seam: Vec<Vec<bool>>,
}

impl SpiraleReversi {
    pub(super) fn new() -> Self {
        Self {
            faces: vec![],
            num_connected_components: 0,
            num_decoded_vertices: 0,
            active_edge: [0,1],
            active_edge_stack: Vec::new(),
            boundary_edges: Vec::new(),
            prev_face: [0,1,2],
            orientation: Vec::new(),
            topology_splits: Vec::new(),
            last_symbol: None,
            active_context: None,
            symbol_buffer: Vec::new(),
            standard_face_data: Vec::new(),
            standard_attribute_connectivity_data: Vec::new(),
            vertex_valences: Vec::new(),
            is_vert_hole: Vec::new(),
            corner_to_vertex_map: Vec::new(),
            vertex_corners: Vec::new(),

            start_face_buffer_prob_zero: 0,
            
            corner_table: CornerTable::new(),
            last_vert_added: -1,
            
            traversal_type: Traversal::Standard,
            num_vertices: 0,
            num_faces: 0,
            num_attribute_data: 0,
            num_encoded_symbols: 0,
            num_encoded_split_symbols: 0,

            curr_att_dec: 0,
            att_dec_types: Vec::new(),
            is_edge_on_seam: Vec::new(),
        }
    }

    pub(super) fn init(&mut self) {
        self.faces.clear();
        self.num_connected_components = 0;
        self.num_decoded_vertices = 0;
        self.active_edge = [0,1];
        self.active_edge_stack.clear();
        self.boundary_edges.clear();
        self.prev_face = [0,1,2];
        self.topology_splits.clear();
        self.orientation.clear();
        self.active_context = None;
        self.last_symbol = None;
        self.symbol_buffer.clear();
        self.standard_face_data.clear();
        self.standard_attribute_connectivity_data.clear();
    }

    fn read_topology_splits<R: ByteReader>(&mut self, reader: &mut R) -> Result<(), Err> {
        let num_topology_splits = leb128_read(reader)? as u32;
        let mut last_idx = 0;
        for _ in 0..num_topology_splits {
            let source_symbol_idx = leb128_read(reader)? as usize + last_idx;
            let split_symbol_idx = source_symbol_idx - leb128_read(reader)? as usize;
            let topology_split = TopologySplit {
                source_symbol_idx,
                split_symbol_idx,
                source_edge_orientation: Orientation::Right, // this value is temporary
            };
            self.topology_splits.push(topology_split);
            last_idx = source_symbol_idx;
        }

        let mut reader: BitReader<_> = BitReader::spown_from(reader).unwrap();
        for split_mut in self.topology_splits.iter_mut() {
            // update the orientation of the topology split.
            split_mut.source_edge_orientation = match reader.read_bits(1)? {
                0 => Orientation::Left,
                1 => Orientation::Right, 
                _ => unreachable!(),
            };
        }
        
        Ok(())
    }

    fn start_traversal<R>(&mut self, reader: &mut R) -> Result<(), Err>
        where R: ByteReader
    {
        match self.traversal_type {
            Traversal::Standard => {
                let size = leb128_read(reader)? as usize;
                self.symbol_buffer.reserve(size);
                for _ in 0..size {
                    self.symbol_buffer.push(reader.read_u8()?);
                }

                self.start_face_buffer_prob_zero = reader.read_u8()?;
                let size = leb128_read(reader)? as usize;
                self.standard_face_data.reserve(size);
                for _ in 0..size {
                    self.standard_face_data.push(reader.read_u8()?);
                }

                let size = leb128_read(reader)? as usize;
                self.standard_attribute_connectivity_data.reserve(size);
                for _ in 0..size {
                    self.standard_attribute_connectivity_data.push(reader.read_u8()?);
                }
            },
            Traversal::Valence => {
                unimplemented!("Valence traversal is not implemented yet.");
            },
            _ => {} // Do nothing otherwise.
        }
        Ok(())
    }
        

    // fn spirale_reversi_standard(&mut self, num_symbols: usize) -> Result<(), Err> {
    //     let mut it = mem::take(&mut self.symbol_buffer).into_iter();
    //     let mut symbol_reader: BitReader<_> = BitReader::spown_from(&mut it).unwrap();
    //     let mut read_next_symbol = || -> Result<Symbol, Err> {
    //         let symbol = if symbol_reader.read_bits(1)? == 0 {
    //             Symbol::C
    //         } else {
    //             match symbol_reader.read_bits(2)? {
    //                 0 => Symbol::R,
    //                 1 => Symbol::L,
    //                 2 => Symbol::E,
    //                 3 => Symbol::S,
    //                 _ => unreachable!(), // it is safe to assume that the symbol is always one of these.
    //             }
    //         };
    //         Ok(symbol)
    //     };
    //     let mut active_corner_stack = Vec::new();
    //     let mut topology_split_active_corners = HashMap::new();

    //     self.is_vert_hole = vec![true; self.num_vertices+self.num_encoded_split_symbols];
    //     let mut num_faces = 0;
    //     for symbol_id in 0..num_symbols {
    //         let face_idx = num_faces;
    //         num_faces += 1;
    //         // Used to flag cases where we need to look for topology split events.
    //         let mut check_topology_split = false;
    //         let symbol = read_next_symbol()?;
    //         match symbol {
    //             Symbol::C => {
    //                 let corner_a = active_corner_stack.last().ok_or(Err::ConnectivityError("Active corner stack is empty."))?;
    //                 let vertex_x = self.corner_table.vertex(self.corner_table.next(corner_a));
    //                 let corner_b = self.corner_table.next(self.corner_table.left_most_corner(vertex_x));

    //                 if corner_a == corner_b {
    //                     Err::ConnectivityError("Matched corners must be different.")?;
    //                 }
    //                 if self.corner_table.opposite(corner_a).is_some() || self.corner_table.opposite(corner_b).is_some() {
    //                     // One of the corners is already opposite to an existing face, which
    //                     // should not happen unless the input was tampered with.
    //                     Err::ConnectivityError("One of the corners is already opposite to an existing face.")?;
    //                 }

    //                 // New tip corner.
    //                 let corner = 3 * face_idx;
    //                 // Update opposite corner mappings.
    //                 self.set_opposite_corners(corner_a, corner + 1);
    //                 self.set_opposite_corners(corner_b, corner + 2);

    //                 // Update vertex mapping.
    //                 let vert_a_prev = self.corner_table.vertex(self.corner_table.previous(corner_a));
    //                 let vert_b_next = self.corner_table.vertex(self.corner_table.next(corner_b));
    //                 if vertex_x == vert_a_prev || vertex_x == vert_b_next {
    //                     // Encoding is invalid, because face vertices are degenerate.
    //                     Err::ConnectivityError("Face vertices are degenerate.");
    //                 }
    //                 self.corner_table.map_corner_to_vertex(corner, vertex_x);
    //                 self.corner_table.map_corner_to_vertex(corner + 1, vert_b_next);
    //                 self.corner_table.map_corner_to_vertex(corner + 2, vert_a_prev);
    //                 self.corner_table.map_corner_to_vertex(vert_a_prev, corner + 2);
    //                 // Mark the vertex |x| as interior.
    //                 self.is_vert_hole[vertex_x] = false;
    //                 // Update the corner on the active stack.
    //                 *active_corner_stack.last_mut().unwrap() = corner;
    //             },
    //             Symbol::R => {
    //                 let corner_a = active_corner_stack.last().ok_or(Err::ConnectivityError("Active corner stack is empty."))?;
    //                 if self.corner_table.opposite(corner_a).is_some() {
    //                     // Active corner is already opposite to an existing face, which should happen.
    //                     return Err::ConnectivityError("Active corner is already opposite to an existing face.");
    //                 }

    //                 // First corner on the new face is either corner "l" or "r".
    //                 let corner = 3 * face_idx;
    //                 // "r" is the new first corner.
    //                 let opp_corner = corner + 2;
    //                 let corner_l = corner + 1;
    //                 let corner_r = corner;
    //                 self.set_opposite_corners(opp_corner, corner_a);
    //                 // Update vertex mapping.
    //                 let new_vert_index = self.corner_table.add_new_vertex();

    //                 if self.corner_table.num_vertices() > max_num_vertices {
    //                     return Err::ConnectivityError("Unexpected number of decoded vertices.");
    //                 }

    //                 corner_table_->MapCornerToVertex(opp_corner, new_vert_index);
    //                 corner_table_->SetLeftMostCorner(new_vert_index, opp_corner);

    //                 const VertexIndex vertex_r =
    //                     corner_table_->Vertex(corner_table_->Previous(corner_a));
    //                 corner_table_->MapCornerToVertex(corner_r, vertex_r);
    //                 // Update left-most corner on the vertex on the |corner_r|.
    //                 corner_table_->SetLeftMostCorner(vertex_r, corner_r);

    //                 corner_table_->MapCornerToVertex(
    //                     corner_l, corner_table_->Vertex(corner_table_->Next(corner_a)));
    //                 active_corner_stack.back() = corner;
    //                 check_topology_split = true;
    //             },
    //             Symbol::L => {
    //                 if (active_corner_stack.empty()) {
    //                     return -1;
    //                 }

    //                 let corner_a = active_corner_stack.back();
    //                 if (corner_table_->Opposite(corner_a) != kInvalidCornerIndex) {
    //                     // Active corner is already opposite to an existing face, which should
    //                     // not happen unless the input was tampered with.
    //                     return -1;
    //                 }

    //                 // First corner on the new face is either corner "l" or "r".
    //                 const CornerIndex corner(3 * face.value());
    //                 CornerIndex opp_corner, corner_l, corner_r;
                    
    //                 // "l" is the new first corner.
    //                 opp_corner = corner + 1;
    //                 corner_l = corner;
    //                 corner_r = corner + 2;

    //                 SetOppositeCorners(opp_corner, corner_a);
    //                 // Update vertex mapping.
    //                 const VertexIndex new_vert_index = corner_table_->AddNewVertex();

    //                 if (corner_table_->num_vertices() > max_num_vertices) {
    //                     return -1;  // Unexpected number of decoded vertices.
    //                 }

    //                 corner_table_->MapCornerToVertex(opp_corner, new_vert_index);
    //                 corner_table_->SetLeftMostCorner(new_vert_index, opp_corner);

    //                 const VertexIndex vertex_r =
    //                     corner_table_->Vertex(corner_table_->Previous(corner_a));
    //                 corner_table_->MapCornerToVertex(corner_r, vertex_r);
    //                 // Update left-most corner on the vertex on the |corner_r|.
    //                 corner_table_->SetLeftMostCorner(vertex_r, corner_r);

    //                 corner_table_->MapCornerToVertex(
    //                     corner_l, corner_table_->Vertex(corner_table_->Next(corner_a)));
    //                 active_corner_stack.back() = corner;
    //                 check_topology_split = true;
    //             },
    //             Symbol::S =>{
    //                 // Create a new face that merges two last active edges from the active
    //                 // stack. No new vertex is created, but two vertices at corners "p" and
    //                 // "n" need to be merged into a single vertex.
    //                 //
    //                 // *-------v-------*
    //                 //  \a   p/x\n   b/
    //                 //   \   /   \   /
    //                 //    \ /  S  \ /
    //                 //     *.......*
    //                 //
    //                 if (active_corner_stack.empty()) {
    //                     return -1;
    //                 }
    //                 const CornerIndex corner_b = active_corner_stack.back();
    //                 active_corner_stack.pop_back();

    //                 // Corner "a" can correspond either to a normal active edge, or to an edge
    //                 // created from the topology split event.
    //                 const auto it = topology_split_active_corners.find(symbol_id);
    //                 if (it != topology_split_active_corners.end()) {
    //                     // Topology split event. Move the retrieved edge to the stack.
    //                     active_corner_stack.push_back(it->second);
    //                 }
    //                 if (active_corner_stack.empty()) {
    //                     return -1;
    //                 }
    //                 const CornerIndex corner_a = active_corner_stack.back();

    //                 if (corner_a == corner_b) {
    //                     // All matched corners must be different.
    //                     return -1;
    //                 }
    //                 if (corner_table_->Opposite(corner_a) != kInvalidCornerIndex ||
    //                     corner_table_->Opposite(corner_b) != kInvalidCornerIndex) {
    //                     // One of the corners is already opposite to an existing face, which
    //                     // should not happen unless the input was tampered with.
    //                     return -1;
    //                 }

    //                 // First corner on the new face is corner "x" from the image above.
    //                 const CornerIndex corner(3 * face.value());
    //                 // Update the opposite corner mapping.
    //                 SetOppositeCorners(corner_a, corner + 2);
    //                 SetOppositeCorners(corner_b, corner + 1);
    //                 // Update vertices. For the vertex at corner "x", use the vertex id from
    //                 // the corner "p".
    //                 const VertexIndex vertex_p =
    //                     corner_table_->Vertex(corner_table_->Previous(corner_a));
    //                 corner_table_->MapCornerToVertex(corner, vertex_p);
    //                 corner_table_->MapCornerToVertex(
    //                     corner + 1, corner_table_->Vertex(corner_table_->Next(corner_a)));
    //                 const VertexIndex vert_b_prev =
    //                     corner_table_->Vertex(corner_table_->Previous(corner_b));
    //                 corner_table_->MapCornerToVertex(corner + 2, vert_b_prev);
    //                 corner_table_->SetLeftMostCorner(vert_b_prev, corner + 2);
    //                 CornerIndex corner_n = corner_table_->Next(corner_b);
    //                 const VertexIndex vertex_n = corner_table_->Vertex(corner_n);
    //                 traversal_decoder_.MergeVertices(vertex_p, vertex_n);
    //                 // Update the left most corner on the newly merged vertex.
    //                 corner_table_->SetLeftMostCorner(vertex_p,
    //                                                 corner_table_->LeftMostCorner(vertex_n));

    //                 // Also update the vertex id at corner "n" and all corners that are
    //                 // connected to it in the CCW direction.
    //                 const CornerIndex first_corner = corner_n;
    //                 while (corner_n != kInvalidCornerIndex) {
    //                     corner_table_->MapCornerToVertex(corner_n, vertex_p);
    //                     corner_n = corner_table_->SwingLeft(corner_n);
    //                     if (corner_n == first_corner) {
    //                     // We reached the start again which should not happen for split
    //                     // symbols.
    //                     return -1;
    //                     }
    //                 }
    //                 // Make sure the old vertex n is now mapped to an invalid corner (make it
    //                 // isolated).
    //                 corner_table_->MakeVertexIsolated(vertex_n);
    //                 if (remove_invalid_vertices) {
    //                     invalid_vertices.push_back(vertex_n);
    //                 }
    //                 active_corner_stack.back() = corner;
    //             },
    //             Symbol::E => {
    //                 const CornerIndex corner(3 * face.value());
    //                 const VertexIndex first_vert_index = corner_table_->AddNewVertex();
    //                 // Create three new vertices at the corners of the new face.
    //                 corner_table_->MapCornerToVertex(corner, first_vert_index);
    //                 corner_table_->MapCornerToVertex(corner + 1,
    //                                                 corner_table_->AddNewVertex());
    //                 corner_table_->MapCornerToVertex(corner + 2,
    //                                                 corner_table_->AddNewVertex());

    //                 if (corner_table_->num_vertices() > max_num_vertices) {
    //                     return -1;  // Unexpected number of decoded vertices.
    //                 }

    //                 corner_table_->SetLeftMostCorner(first_vert_index, corner);
    //                 corner_table_->SetLeftMostCorner(first_vert_index + 1, corner + 1);
    //                 corner_table_->SetLeftMostCorner(first_vert_index + 2, corner + 2);
    //                 // Add the tip corner to the active stack.
    //                 active_corner_stack.push_back(corner);
    //                 check_topology_split = true;
    //             }
    //         };

    //         // Inform the traversal decoder that a new corner has been reached.
    //         traversal_decoder_.NewActiveCornerReached(active_corner_stack.back());

    //         if (check_topology_split) {
    //         // Check for topology splits happens only for TOPOLOGY_L, TOPOLOGY_R and
    //         // TOPOLOGY_E symbols because those are the symbols that correspond to
    //         // faces that can be directly connected a TOPOLOGY_S face through the
    //         // topology split event.
    //         // If a topology split is detected, we need to add a new active edge
    //         // onto the active_corner_stack because it will be used later when the
    //         // corresponding TOPOLOGY_S event is decoded.

    //         // Symbol id used by the encoder (reverse).
    //         const int encoder_symbol_id = num_symbols - symbol_id - 1;
    //         EdgeFaceName split_edge;
    //         int encoder_split_symbol_id;
    //         while (IsTopologySplit(encoder_symbol_id, &split_edge,
    //                                 &encoder_split_symbol_id)) {
    //             if (encoder_split_symbol_id < 0) {
    //             return -1;  // Wrong split symbol id.
    //             }
    //             // Symbol was part of a topology split. Now we need to determine which
    //             // edge should be added to the active edges stack.
    //             const CornerIndex act_top_corner = active_corner_stack.back();
    //             // The current symbol has one active edge (stored in act_top_corner) and
    //             // two remaining inactive edges that are attached to it.
    //             //              *
    //             //             / \
    //             //  left_edge /   \ right_edge
    //             //           /     \
    //             //          *.......*
    //             //         active_edge

    //             CornerIndex new_active_corner;
    //             if (split_edge == RIGHT_FACE_EDGE) {
    //             new_active_corner = corner_table_->Next(act_top_corner);
    //             } else {
    //             new_active_corner = corner_table_->Previous(act_top_corner);
    //             }
    //             // Add the new active edge.
    //             // Convert the encoder split symbol id to decoder symbol id.
    //             const int decoder_split_symbol_id =
    //                 num_symbols - encoder_split_symbol_id - 1;
    //             topology_split_active_corners[decoder_split_symbol_id] =
    //                 new_active_corner;
    //         }
    //         }
    //     }
    //     if (corner_table_->num_vertices() > max_num_vertices) {
    //         return -1;  // Unexpected number of decoded vertices.
    //     }
    //     // Decode start faces and connect them to the faces from the active stack.
    //     while (!active_corner_stack.empty()) {
    //         const CornerIndex corner = active_corner_stack.back();
    //         active_corner_stack.pop_back();
    //         const bool interior_face =
    //             traversal_decoder_.DecodeStartFaceConfiguration();
    //         if (interior_face) {
    //         // The start face is interior, we need to find three corners that are
    //         // opposite to it. The first opposite corner "a" is the corner from the
    //         // top of the active corner stack and the remaining two corners "b" and
    //         // "c" are then the next corners from the left-most corners of vertices
    //         // "n" and "x" respectively.
    //         //
    //         //           *-------*
    //         //          / \     / \
    //         //         /   \   /   \
    //         //        /     \ /     \
    //         //       *-------p-------*
    //         //      / \a    . .    c/ \
    //         //     /   \   .   .   /   \
    //         //    /     \ .  I  . /     \
    //         //   *-------n.......x------*
    //         //    \     / \     / \     /
    //         //     \   /   \   /   \   /
    //         //      \ /     \b/     \ /
    //         //       *-------*-------*
    //         //

    //         if (num_faces >= corner_table_->num_faces()) {
    //             return -1;  // More faces than expected added to the mesh.
    //         }

    //         const CornerIndex corner_a = corner;
    //         const VertexIndex vert_n =
    //             corner_table_->Vertex(corner_table_->Next(corner_a));
    //         const CornerIndex corner_b =
    //             corner_table_->Next(corner_table_->LeftMostCorner(vert_n));

    //         const VertexIndex vert_x =
    //             corner_table_->Vertex(corner_table_->Next(corner_b));
    //         const CornerIndex corner_c =
    //             corner_table_->Next(corner_table_->LeftMostCorner(vert_x));

    //         if (corner == corner_b || corner == corner_c || corner_b == corner_c) {
    //             // All matched corners must be different.
    //             return -1;
    //         }
    //         if (corner_table_->Opposite(corner) != kInvalidCornerIndex ||
    //             corner_table_->Opposite(corner_b) != kInvalidCornerIndex ||
    //             corner_table_->Opposite(corner_c) != kInvalidCornerIndex) {
    //             // One of the corners is already opposite to an existing face, which
    //             // should not happen unless the input was tampered with.
    //             return -1;
    //         }

    //         const VertexIndex vert_p =
    //             corner_table_->Vertex(corner_table_->Next(corner_c));

    //         const FaceIndex face(num_faces++);
    //         // The first corner of the initial face is the corner opposite to "a".
    //         const CornerIndex new_corner(3 * face.value());
    //         SetOppositeCorners(new_corner, corner);
    //         SetOppositeCorners(new_corner + 1, corner_b);
    //         SetOppositeCorners(new_corner + 2, corner_c);

    //         // Map new corners to existing vertices.
    //         corner_table_->MapCornerToVertex(new_corner, vert_x);
    //         corner_table_->MapCornerToVertex(new_corner + 1, vert_p);
    //         corner_table_->MapCornerToVertex(new_corner + 2, vert_n);

    //         // Mark all three vertices as interior.
    //         for (int ci = 0; ci < 3; ++ci) {
    //             is_vert_hole_[corner_table_->Vertex(new_corner + ci).value()] = false;
    //         }

    //         init_face_configurations_.push_back(true);
    //         init_corners_.push_back(new_corner);
    //         } else {
    //         // The initial face wasn't interior and the traversal had to start from
    //         // an open boundary. In this case no new face is added, but we need to
    //         // keep record about the first opposite corner to this boundary.
    //         init_face_configurations_.push_back(false);
    //         init_corners_.push_back(corner);
    //         }
    //     }
    //     if (num_faces != corner_table_->num_faces()) {
    //         return -1;  // Unexpected number of decoded faces.
    //     }

    //     int num_vertices = corner_table_->num_vertices();
    //     // If any vertex was marked as isolated, we want to remove it from the corner
    //     // table to ensure that all vertices in range <0, num_vertices> are valid.
    //     for (const VertexIndex invalid_vert : invalid_vertices) {
    //         // Find the last valid vertex and swap it with the isolated vertex.
    //         VertexIndex src_vert(num_vertices - 1);
    //         while (corner_table_->LeftMostCorner(src_vert) == kInvalidCornerIndex) {
    //         // The last vertex is invalid, proceed to the previous one.
    //         src_vert = VertexIndex(--num_vertices - 1);
    //         }
    //         if (src_vert < invalid_vert) {
    //         continue;  // No need to swap anything.
    //         }

    //         // Remap all corners mapped to |src_vert| to |invalid_vert|.
    //         VertexCornersIterator<CornerTable> vcit(corner_table_.get(), src_vert);
    //         for (; !vcit.End(); ++vcit) {
    //         const CornerIndex cid = vcit.Corner();
    //         if (corner_table_->Vertex(cid) != src_vert) {
    //             // Vertex mapped to |cid| was not |src_vert|. This indicates corrupted
    //             // data and we should terminate the decoding.
    //             return -1;
    //         }
    //         corner_table_->MapCornerToVertex(cid, invalid_vert);
    //         }
    //         corner_table_->SetLeftMostCorner(invalid_vert,
    //                                         corner_table_->LeftMostCorner(src_vert));

    //         // Make the |src_vert| invalid.
    //         corner_table_->MakeVertexIsolated(src_vert);
    //         is_vert_hole_[invalid_vert.value()] = is_vert_hole_[src_vert.value()];
    //         is_vert_hole_[src_vert.value()] = false;

    //         // The last vertex is now invalid.
    //         num_vertices--;
    //     }
  
    //     self.process_interior_edges();
  
    //     num_vertices;
    //     Ok(())
    // }

    fn spirale_reversi_valence(&mut self) -> Result<(), Err> {
        unimplemented!("Valence traversal is not implemented yet.");
    }

    // fn process_interior_edges(&mut self) -> Result<(), Err> {
    //     let mut standard_face_data = mem::take(&mut self.standard_face_data).into_iter();
    //     let size = standard_face_data.len(); 
    //     let mut decoder: RabsDecoder<_> = RabsDecoder::new(
    //         &mut standard_face_data, 
    //         size,
    //         self.start_face_buffer_prob_zero as usize, 
    //         None
    //     )?;

    //     while let Some(corner_a) = self.active_corner_stack.pop() {
    //         let interior_face = decoder.read()?;
    //         if interior_face > 0 {
    //             let mut corner_b = self.corner_table.previous(corner_a);
    //             while let Some(b_opp) = self.corner_table.opposite_corners[corner_b] {
    //                 corner_b = self.corner_table.previous(b_opp);
    //             }

    //             let mut corner_c = self.corner_table.next(corner_a);
    //             while let Some(c_opp) = self.corner_table.opposite_corners[corner_c] {
    //                 corner_c = self.corner_table.next(c_opp);
    //             }
    //             let new_corner = self.faces.len() * 3;
    //             self.corner_table.set_opposite_corners(new_corner, corner_a);
    //             self.corner_table.set_opposite_corners(new_corner + 1, corner_b);
    //             self.corner_table.set_opposite_corners(new_corner + 2, corner_c);

    //             let [temp_v, next_a, temp_p] = self.corner_table.corner_to_verts(corner_a);
    //             let [temp_v, next_b, temp_p] = self.corner_table.corner_to_verts(corner_b);
    //             let [temp_v, next_c, temp_p] = self.corner_table.corner_to_verts(corner_c);
    //             self.map_corner_to_vertex(new_corner, next_b);
    //             self.map_corner_to_vertex(new_corner + 1, next_c);
    //             self.map_corner_to_vertex(new_corner + 2, next_a);
    //             self.faces.push([next_b, next_c, next_a]);

    //             // Mark all three vertices as interior.
    //             self.is_vert_hole[next_b] = false;
    //             self.is_vert_hole[next_c] = false;
    //             self.is_vert_hole[next_a] = false;
    //         }
    //     }
    //     Ok(())
    // }

    fn is_topology_split(&mut self, symbol_idx: usize) -> Option<(Orientation, usize)> {
        let split = if let Some(split) = self.topology_splits.last() {
            if split.source_symbol_idx == symbol_idx {
                self.topology_splits.pop().unwrap()
            } else {
                return None;
            }
        } else {
            return None;
        };
        let out_face_edge = split.source_edge_orientation;
        let out_encoder_split_symbol_id = split.split_symbol_idx;
        Some( (out_face_edge, out_encoder_split_symbol_id))
    }

    fn replace_verts(&mut self, from: usize, to: usize) {
        for i in 0..self.faces.len() {
            for v in self.faces[i].iter_mut() {
                if *v == from {
                    *v = to;
                }
            }
        }
    }

    // fn update_corners_after_merge(&mut self, c: usize, v: usize) {
    //     let opp_corner = self.corner_table.opposite_corners[c];
    //     if let Some(opp_corner) = opp_corner {
    //         let corner_n = self.corner_table.next(opp_corner);
    //         let mut corner_n = Some(corner_n);
    //         while let Some(corner_n_unwrapped) = corner_n {
    //             self.map_corner_to_vertex(corner_n_unwrapped, v);
    //             corner_n = self.swing_left(corner_n_unwrapped);
    //         }
    //     }
    // }
    
    #[inline]
    fn map_corner_to_vertex(&mut self, corner: usize, v: VertexIdx) {
        self.corner_to_vertex_map[0][corner] = v;
        self.vertex_corners[v] = corner;
    }



    fn spirale_reversi_recc(&mut self, symbol: Symbol) {
        match symbol {
            Symbol::C => {
                let right_vertex = self.active_edge[0];
                // ToDo: Optimize this
                let next_vertex = *self.boundary_edges.iter()
                    .find(|e| 
                        e.contains(&right_vertex) &&
                        !e.contains(&self.active_edge[1])
                    )
                    .unwrap()
                    .iter()
                    .find(|&v| *v != right_vertex)
                    .unwrap();
                
                let mut new_face = [
                    self.active_edge[0],
                    self.active_edge[1],
                    next_vertex
                ];
                // ToDo: This sort can be omitted by constructing a face in a proper order.
                new_face.sort();

                self.faces.push(new_face);

                // modify the boundary edges
                let removed_edge = [
                    cmp::min(self.active_edge[0], self.active_edge[1]),
                    cmp::max(self.active_edge[0], self.active_edge[1]),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap()
                );
                let removed_edge = [
                    cmp::min(next_vertex, self.active_edge[0]),
                    cmp::max(next_vertex, self.active_edge[0]),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap()
                );
                let new_edge = [
                    cmp::min(next_vertex, self.active_edge[1]),
                    cmp::max(next_vertex, self.active_edge[1]),
                ];
                if let Some(idx) = self.boundary_edges.binary_search(&new_edge).err() {
                    self.boundary_edges.insert(idx, new_edge);
                };
                debug_assert!(self.boundary_edges.is_sorted());

                // update the right vertex.
                self.active_edge[0] = next_vertex;
            },
            Symbol::R => {
                let mut new_face = [
                    *self.active_edge.iter().min().unwrap(),
                    *self.active_edge.iter().max().unwrap(),
                    self.num_decoded_vertices
                ];

                // ToDo: This sort can be omitted by constructing a face in a proper order.
                new_face.sort();

                self.faces.push(new_face);

                // modify the boundary edges
                let removed_edge = [
                    cmp::min(self.active_edge[0], self.active_edge[1]),
                    cmp::max(self.active_edge[0], self.active_edge[1]),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap()
                );
                let new_edge = [
                    cmp::min(self.active_edge[0], self.num_decoded_vertices),
                    cmp::max(self.active_edge[0], self.num_decoded_vertices),
                ];
                let idx = self.boundary_edges.binary_search(&new_edge).unwrap_err();
                self.boundary_edges.insert(idx, new_edge);
                let new_edge = [
                    cmp::min(self.active_edge[1], self.num_decoded_vertices),
                    cmp::max(self.active_edge[1], self.num_decoded_vertices),
                ];
                let idx = self.boundary_edges.binary_search(&new_edge).unwrap_err();
                self.boundary_edges.insert(idx, new_edge);
                debug_assert!(self.boundary_edges.is_sorted());

                self.active_edge[1] = self.num_decoded_vertices;
                self.num_decoded_vertices += 1;
            },
            Symbol::L => {
                let mut new_face = [
                    *self.active_edge.iter().min().unwrap(),
                    *self.active_edge.iter().max().unwrap(),
                    self.num_decoded_vertices
                ];
                // ToDo: This sort can be omitted by constructing a face in a proper order.
                new_face.sort();
                self.faces.push(new_face);
                
                // modify the boundary edges
                let removed_edge = [
                    cmp::min(self.active_edge[0], self.active_edge[1]),
                    cmp::max(self.active_edge[0], self.active_edge[1]),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap()
                );
                let new_edge = [
                    cmp::min(self.active_edge[0], self.num_decoded_vertices),
                    cmp::max(self.active_edge[0], self.num_decoded_vertices),
                ];
                let idx = self.boundary_edges.binary_search(&new_edge).unwrap_err();
                self.boundary_edges.insert(idx, new_edge);
                let new_edge = [
                    cmp::min(self.active_edge[1], self.num_decoded_vertices),
                    cmp::max(self.active_edge[1], self.num_decoded_vertices),
                ];
                let idx = self.boundary_edges.binary_search(&new_edge).unwrap_err();
                self.boundary_edges.insert(idx, new_edge);
                debug_assert!(self.boundary_edges.is_sorted());

                self.active_edge[0] = self.num_decoded_vertices;
                self.num_decoded_vertices += 1;
            },
            Symbol::E => {
                if self.num_decoded_vertices == 2 {
                    let mut new_face = [
                        self.active_edge[0],
                        self.active_edge[1],
                        self.num_decoded_vertices
                    ];
                    // ToDo: This sort can be omitted by constructing a face in a proper order.
                    new_face.sort();
                    self.faces.push(new_face);

                    // modify the boundary edges
                    debug_assert!(self.boundary_edges.is_empty());
                    self.boundary_edges.push([new_face[0], new_face[1]]);
                    self.boundary_edges.push([new_face[0], new_face[2]]);
                    self.boundary_edges.push([new_face[1], new_face[2]]);
                    
                    // choose any edge of the triangle
                    self.active_edge = [
                        new_face[0], 
                        new_face[1]
                    ];
                
                } else {
                    self.num_decoded_vertices += 2;
                    let new_face = [
                        self.num_decoded_vertices-2,
                        self.num_decoded_vertices-1,
                        self.num_decoded_vertices
                    ];
                    self.faces.push(new_face);
                    
                    // modify the boundary edges
                    self.boundary_edges.push([new_face[0], new_face[1]]);
                    self.boundary_edges.push([new_face[0], new_face[2]]);
                    self.boundary_edges.push([new_face[1], new_face[2]]);
                    debug_assert!(self.boundary_edges.is_sorted());

                    self.active_edge_stack.push(self.active_edge);
                    // choose any edge of the triangle
                    self.active_edge = [
                        new_face[0], 
                        new_face[1]
                    ];
                };
                self.num_decoded_vertices += 1;
            },
            Symbol::S => {
                let prev_active_edge = self.active_edge_stack.pop().unwrap();
                
                // merge the right vertex of the active edge and the left vertex of the previous active edge.
                let mut new_face = [
                    prev_active_edge[0], // right vertex of the previous active edge
                    prev_active_edge[1], // left vertex of the previous active edge (merged)
                    self.active_edge[1], // left vertex of the active edge
                ];
                new_face.sort();
                self.faces.push(new_face);

                // modify the boundary edges
                let removed_edge = [
                    cmp::min(self.active_edge[0], self.active_edge[1]),
                    cmp::max(self.active_edge[0], self.active_edge[1]),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap()
                );
                let removed_edge = [
                    cmp::min(prev_active_edge[0], prev_active_edge[1]),
                    cmp::max(prev_active_edge[0], prev_active_edge[1]),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap()
                );
                let new_edge = [
                    cmp::min(prev_active_edge[0], self.active_edge[1]),
                    cmp::max(prev_active_edge[0], self.active_edge[1]),
                ];
                let idx = self.boundary_edges.binary_search(&new_edge).unwrap_err();
                self.boundary_edges.insert(idx, new_edge);
                debug_assert!(self.boundary_edges.is_sorted());

                // now that the right vertex of the active edge is removed, we need to renumber
                // the vertices numbered after the vertex.
                {
                    for face in self.faces.iter_mut() {
                        for vertex in face.iter_mut() {
                            if *vertex > self.active_edge[0] {
                                *vertex -= 1;
                            } else if *vertex == self.active_edge[0] {
                                *vertex = prev_active_edge[1];
                            }
                        }
                        face.sort();
                    }
                    for edge in self.active_edge_stack.iter_mut() {
                        for vertex in edge.iter_mut() {
                            if *vertex > self.active_edge[0] {
                                *vertex -= 1;
                            } else if *vertex == self.active_edge[0] {
                                *vertex = prev_active_edge[1];
                            }
                        }
                    }
                    for edge in self.boundary_edges.iter_mut() {
                        for vertex in edge.iter_mut() {
                            if *vertex > self.active_edge[0] {
                                *vertex -= 1;
                            } else if *vertex == self.active_edge[0] {
                                *vertex = prev_active_edge[1];
                            }
                        }
                        edge.sort();
                    }
                    self.boundary_edges.sort();
                }

                
                
                let merged_vertex = self.active_edge[0];
                self.active_edge = [prev_active_edge[0], self.active_edge[1]];
                for vertex in self.active_edge.iter_mut() {
                    if *vertex > merged_vertex {
                        *vertex -= 1;
                    } else if *vertex == merged_vertex {
                        *vertex = prev_active_edge[1];
                    }
                }

                self.num_decoded_vertices -= 1;
                assert_ne!(self.active_edge[0], self.active_edge[1]);
                assert!( 
                    self.is_boundary_cyclic(),
                    "boundary_edges: {:?}",
                    self.boundary_edges
                );
            },
        }
    }


    #[allow(unused)]
    fn is_boundary_cyclic(&self) -> bool {
        let mut visited_edges = vec![false; self.boundary_edges.len()];
        while let Some(edge_idx) = visited_edges.iter()
            .position(|&x| x == false) 
        {
            let start = self.boundary_edges[edge_idx][0];
            let mut prev_vertex = start;
            let mut curr_vertex = self.boundary_edges[edge_idx][1];
            visited_edges[edge_idx] = true;
            while curr_vertex != start {
                let next_vertex = {
                    let edge =  if let Some(edge) = self.boundary_edges.iter()
                        .find(|e| 
                            e.contains(&curr_vertex) && 
                            !e.contains(&prev_vertex)
                        ) {
                            edge
                        } else {
                            return false;
                        };
                    let idx = self.boundary_edges.binary_search(&edge).unwrap();
                    if visited_edges[idx] {
                        return false;
                    } else {
                        visited_edges[idx] = true;
                    }

                    *edge.iter()
                        .find(|&&v| v != curr_vertex)
                        .unwrap()
                };
                prev_vertex = curr_vertex;
                curr_vertex = next_vertex;
            }
        }
        true
    }

    fn recover_orientation(&mut self, sign_of_first_face: bool) {
        if self.faces.is_empty() {
            return;
        }
        // records the sign of the faces. 'None' if the face is not visited.
        let mut sign_of_faces = vec![None; self.faces.len()];
        let mut face_stack = vec![self.faces.len()-1];
        sign_of_faces[self.faces.len()-1] = Some(sign_of_first_face);
        while let Some(face_idx) = face_stack.pop() {
            let face = self.faces[face_idx];

            // ToDo: Optimize this.
            let adjacent_faces = (0..self.faces.len())
                .rev()
                .filter(|i| sign_of_faces[*i].is_none())
                .filter_map(|i| edge_shared_by(&face, &self.faces[i]).map(|e| (e,i)))
                .take(2)
                .collect::<Vec<_>>();

            for (shared_edge, adj_face_idx) in adjacent_faces {
                sign_of_faces[adj_face_idx] = Some(
                    orientation_of_next_face(
                        face, 
                        sign_of_faces[face_idx].unwrap(), 
                        shared_edge, 
                        self.faces[adj_face_idx]
                    )
                );
                face_stack.push(adj_face_idx);
            }
        }

        for (i, s) in sign_of_faces.into_iter().enumerate() {
            let s = s.unwrap();
            if !s {
                self.faces[i].swap(1, 2);
            }
        }
    }
}

impl ConnectivityDecoder for SpiraleReversi {
    type Err = Err;
    fn decode_connectivity<R>(&mut self, reader: &mut R) -> Result<Vec<[VertexIdx; 3]>, Err> 
        where R: ByteReader
    {
        self.traversal_type = Traversal::read_from(reader)?;
        self.num_vertices = leb128_read(reader)? as usize;
        self.num_faces = leb128_read(reader)? as usize;
        self.num_attribute_data = reader.read_u8()? as usize;
        self.num_encoded_symbols = leb128_read(reader)? as usize;
        self.num_encoded_split_symbols = leb128_read(reader)? as usize;

        self.init();

        self.read_topology_splits(reader)?;

        self.start_traversal(reader)?;

        // unwrap the symbol encoding config here so that the spirale reversi does not 
        // need to unwrap config during each iteration.
        match self.traversal_type {
            Traversal::Standard => unimplemented!(), // self.spirale_reversi_standard(),
            Traversal::Valence => self.spirale_reversi_valence(),
            _ => unimplemented!()
        }?;


        let mut faces = Vec::new();
        mem::swap(&mut faces, &mut self.faces);
        Ok(faces)
    }
}

#[cfg(not(feature = "evaluation"))]
#[cfg(test)]
mod tests {
    use crate::core::attribute::AttributeId;
    use crate::encode::connectivity::edgebreaker::Config;
    use crate::encode::connectivity::{edgebreaker, ConnectivityEncoder};
    use crate::core::shared::{
        ConfigType, NdVector, Vector
    };
    use crate::prelude::{Attribute, AttributeType};
    use super::*;
    use crate::decode::connectivity::ConnectivityDecoder;


    fn manual_test(
        mut original_faces: Vec<[VertexIdx; 3]>, 
        points: Vec<NdVector<3,f32>>, 
        expected_faces: Vec<[VertexIdx; 3]>) 
    {
        let mut point_att = Attribute::from(AttributeId::new(0), points, AttributeType::Position, Vec::new());
        let mut edgebreaker = edgebreaker::Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut [&mut point_att], &mut original_faces).is_ok());
        let mut writer = Vec::new();
        assert!(edgebreaker.encode_connectivity(&mut original_faces, &mut [&mut point_att], &mut writer).is_ok());
        let mut reader = writer.into_iter();
        let mut spirale_reversi = SpiraleReversi::new();
        let decoded_faces = spirale_reversi.decode_connectivity(&mut reader);

        let decoded_faces = match decoded_faces {
            Ok(faces) => faces,
            Err(e) => panic!("Failed to decode faces: {:?}", e),
        };
        assert_eq!(&decoded_faces, &expected_faces);
        assert_eq!(original_faces, decoded_faces);
    }

    #[test]
    fn simplest() {
        let original_faces = vec![
            [0,1,2],
            [1,2,3]
        ];
        let points = vec![NdVector::<3,f32>::zero(); 4];

        let expected_faces = vec![
            [0,2,1],
            [0,1,3]
        ];

        manual_test(original_faces, points, expected_faces);
    }

    #[test]
    fn test_split() {
        let original_faces = vec![
            [0,1,2],
            [0,2,4],
            [0,4,5],
            [2,3,4]
        ];
        let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];
        let expected_faces = vec![
            [0,2,1], 
            [1,4,3], 
            [0,1,3], 
            [0,3,5]
        ];
        manual_test(original_faces, points, expected_faces);
    }

    #[test]
    fn test_disc() {
        let original_faces = vec![
            [0,1,4],
            [0,3,4],
            [1,2,5],
            [1,4,5],
            [2,5,6],
            [3,4,7],
            [3,7,10],
            [4,5,7],
            [5,6,8],
            [5,7,8],
            [7,8,9],
            [7,9,10],
            [8,9,11],
            [9,10,11]
        ];
        // positions do not matter
        let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];
        let exptected_faces = vec![
            [0,1,2], 
            [1,3,4], 
            [0,3,1], 
            [0,5,3], 
            [0,6,5], 
            [5,6,7], 
            [6,8,7], 
            [0,8,6], 
            [0,2,8], 
            [2,9,8], 
            [2,10,9], 
            [2,11,10], 
            [1,11,2], 
            [1,4,11]
        ];
        manual_test(original_faces, points, exptected_faces);
    }

    #[test]
    fn test_long_split() {
        let original_faces = vec![
            [0,1,2],
            [0,2,3],
            [0,3,4],
            [1,2,6],
            [1,5,6]
        ];
        // positions do not matter
        let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

        let expected_faces = vec![
            [0,2,1], 
            [0,1,3], 
            [4,6,5], 
            [3,4,5], 
            [0,3,5]
        ];

        manual_test(original_faces, points, expected_faces);
    }

    #[test]
    fn test_hole() {
        let original_faces = vec![
            [9,23,24], [8,9,23], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,22,23], [7,8,22], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,21,22], [6,7,21], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,20,21], [6,19,20], [5,6,19], [5,18,19], [4,5,18], [4,17,18], [4,15,17], [15,16,17]
        ];

        // positions do not matter
        let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

        let expected_faces = vec![
            [0,1,2], [3,4,5], [4,6,7], [3,6,4], [3,8,6], [3,9,8], [8,9,10], [9,11,10], 
            [10,11,12], [11,13,12], [1,13,11], [1,14,13], [0,14,1], [0,15,14], [15,16,17], [0,16,15],
            [0,18,16], [0,2,18], [2,19,18], [20,21,22], [19,21,20], [2,21,19], [2,23,21], [1,23,2], 
            [1,11,23], [9,23,11], [9,24,23], [3,24,9], [3,5,24], [5,22,24], [21,24,22], [21,23,24]
        ];

        manual_test(original_faces, points, expected_faces);
    }

    // #[test]
    fn test_handle() {
        // create torus in order to test the handle symbol.
        let original_faces = vec![
            [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
        ];

        // positions do not matter
        let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

        let expected_faces = vec![
            [0,1,2], [1,3,4], [0,1,3], [0,3,5], [2,6,7], [4,7,8], [6,7,8], [5,6,8], 
            [5,8,9], [0,5,9], [0,9,10], [0,2,10], [2,7,10], [7,10,11], [4,7,11], [3,4,11], 
            [3,11,12], [3,5,12], [5,6,12], [6,12,13], [2,6,13], [1,2,13], [1,13,14], [1,4,14], 
            [4,8,14], [8,9,14], [9,14,15], [9,10,15], [10,11,15], [11,12,15], [12,13,15], [13,14,15]
        ];

        manual_test(original_faces, points, expected_faces);
    }

    // This test is disabled because it takes too long to run.
    // #[test]
    #[allow(unused)]
    fn test_with_large_mesh() {
        let (bunny,_) = tobj::load_obj(
            format!("tests/data/bunny.obj"), 
            &tobj::GPU_LOAD_OPTIONS
        ).unwrap();
        let bunny = &bunny[0];
        let mesh = &bunny.mesh;

        let mut faces = mesh.indices.chunks(3)
            .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
            .collect::<Vec<_>>();

        let points = mesh.positions.chunks(3)
            .map(|x| NdVector::<3,f32>::from([x[0], x[1], x[2]]))
            .collect::<Vec<_>>();

        let mut point_att = Attribute::from(AttributeId::new(0), points, AttributeType::Position, Vec::new());
        let mut edgebreaker = edgebreaker::Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut [&mut point_att], &mut faces).is_ok());
        let mut writer = Vec::new();
        assert!(edgebreaker.encode_connectivity(&mut faces, &mut [&mut point_att], &mut writer).is_ok());
        let mut reader = writer.into_iter();
        let mut spirale_reversi = SpiraleReversi::new();
        let decoded_faces = spirale_reversi.decode_connectivity(&mut reader);

        let decoded_faces = match decoded_faces {
            Ok(faces) => faces,
            Err(e) => panic!("Failed to decode faces: {:?}", e),
        };

        assert_eq!(faces, decoded_faces);
    }
}
