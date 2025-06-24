use std::{
    fmt,
    cmp,
};

use crate::core::buffer::LsbFirst;
use crate::core::corner_table::all_inclusive_corner_table::AllInclusiveCornerTable;
use crate::core::corner_table::attribute_corner_table::AttributeCornerTable;
use crate::core::corner_table::GenericCornerTable;
use crate::core::bit_coder::ByteWriter;
use crate::core::corner_table::CornerTable;
use crate::encode::entropy::rans::{self, RabsCoder};
use crate::encode::entropy::symbol_coding::encode_symbols;
use crate::debug_write;
use crate::prelude::{Attribute, AttributeType, BitWriter};
use crate::shared::connectivity::edgebreaker::symbol_encoder::{CrLight, Symbol, SymbolEncoder};

use crate::core::shared::{ConfigType, CornerIdx, EdgeIdx, FaceIdx, VertexIdx};

use crate::shared::connectivity::edgebreaker::{self, edge_shared_by, orientation_of_next_face, sign_of, EdgebreakerKind, Orientation, TopologySplit, MAX_VALENCE, MIN_VALENCE};
use crate::shared::connectivity::EdgebreakerDecoder;
use crate::shared::entropy::SymbolEncodingMethod;
use crate::utils::bit_coder::leb128_write;
use std::collections::{BTreeMap, VecDeque};
use std::vec;



use crate::encode::connectivity::ConnectivityEncoder;

#[cfg(feature = "evaluation")]
use crate::eval;

pub(crate) struct Edgebreaker<'faces, T> 
    where T: Traversal
{
	/// 'edges' is a set of edges of the input mesh, each of which is a two-element 
	/// non-multiset sorted in the increasing order. 'edges' itself is also sorted 
	/// by the initial vertex of its edges in the increasing order.
	edges: Vec<[VertexIdx;2]>,
	
	/// 'coboundary_map_one' records the coboundary information of edges, i.e. the i'th 
	/// entry of this array stores the indexes of the faces that have 'edge[i]'
	/// as the boundary. 
	coboundary_map_one: Vec<Vec<FaceIdx>>,

    /// 'face_connectivity' records the connectivity information of faces.
    /// Each entry of this array is an array of three face indexes.
    face_connectivity: Vec<[FaceIdx; 3]>,

	/// 'coboundary_map_zero' records the coboundary information of vertices, i.e. the i'th 
	/// entry of this array stores the indexes of the edges that have the 'i'th vertex
	/// as the boundary. This value is lazily computed, and it is 'None' if it is not computed yet.
	/// It is designed to be so because the coboundary information of vertices is necessary only
	/// when the Edgebreaker is not homeomorphic to a sphere.
	coboundary_map_zero: Option<Vec<Vec<EdgeIdx>>>,

    lies_on_boundary_or_cutting_path: Vec<bool>,
	
	/// The 'i'th entry of 'visited_vertices' is true if the Edgebreaker has
	/// already visited the 'i' th vertex.
	visited_vertices: Vec<bool>,

	/// The 'i'th entry of 'visited_edges' is true if the Edgebreaker has
	/// already visited the 'i' th face.
	visited_faces: Vec<bool>,
    
    /// Corner table: a fast-lookup structure for the mesh connectivity.
    corner_table: CornerTable<'faces>,

    /// The visited holes. i th entry of this array records whether the i th hole is visited or not.
    visited_holes: Vec<bool>,

    // A map from vertices to the hole id if the vertex is on a hole or void if the vertex is not on a hole.
    vertex_hole_id: Vec<Option<usize>>,
	
	/// The edge index stack to remember the split information given by the 'S' symbol. 
    /// Since 'S' symbol might later transform into 'H' symbol, we need to remember by index
    /// which 'S' symbol caused the active edge. If the active edge is not caused by 'S' symbol,
    /// then the second element of the tuple is 'None'.
	active_edge_face_idx_stack: Vec<(OrientedEdge, FaceIdx)>,

	/// This stores the information of the decomposition.
	/// Each element of the vector is a list of vertex indexes that forms a path along a cut.
	cutting_paths: Vec<Vec<VertexIdx>>,

	symbols: Vec<Symbol>,

	/// The orientation of the faces. The 'i'th entry of this array stores the orientation of the 'i'th face.
	face_orientation: Vec<bool>,

    /// Stores the face corresponding to each symbol in the resulting string.
    /// This will be used in the case of reverse decoding.
    symbol_idx_to_face_idx: Vec<usize>,

    vertex_decode_order: Vec<usize>,

    face_decode_order: Vec<usize>,

    num_decoded_vertices: usize,

    corner_traversal_stack: Vec<CornerIdx>,

    last_encoded_symbol_idx: usize,

    processed_connectivity_corners: Vec<CornerIdx>,

    face_to_split_symbol_map: BTreeMap<usize, usize>,

    num_split_symbols: usize,

    vertex_traversal_length: Vec<usize>,
    
    init_face_connectivity_corners: Vec<CornerIdx>,

    traversal: T,

    /// records the signs of the permutations when faces are sorted.
    signs_of_faces: Vec<bool>,

    /// The previous face index that was deoded.
    prev_face_idx: FaceIdx,

    /// Records the topology splits detected during the edgebreaker encoding.
    topology_splits: Vec<TopologySplit>,

    /// The map from the face index to the split symbol index.
    map_face_idx_to_split_symbol_idx: BTreeMap<FaceIdx, usize>,

    /// The last corners encoded for each connected component. This is used to traverse the connected components in the attribute encoder.
    corners_for_connected_components: Vec<CornerIdx>,

    attribute_encoding_data: Vec<AttributeCornerTable>,
	
	/// configurations for the encoder
	config: Config
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OrientedEdge {
    left_vertex: VertexIdx,
    right_vertex: VertexIdx,
}


#[derive(Clone, fmt::Debug, cmp::PartialEq)]
pub struct Config {
    pub traversal: EdgebreakerKind,
    pub decoder: EdgebreakerDecoder,
    pub use_single_connectivity: bool,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self{
            traversal: EdgebreakerKind::Standard,
            decoder: EdgebreakerDecoder::SpiraleReversi,
            use_single_connectivity: false,
		}
    }
}


pub(crate) struct Output<'faces> {
    pub(crate) corner_table: AllInclusiveCornerTable<'faces, >,
    pub(crate) corners_for_connected_components: Vec<CornerIdx>,
}


#[derive(Debug, PartialEq)]
#[remain::sorted]
#[derive(thiserror::Error)]
pub enum Err {
    #[error("Edgebreaker error: {0}")]
    EdgebreakerError(#[from] edgebreaker::Err),
    #[error("Entropy encoding error: {0}")]
    EntropyEncodingError(#[from] crate::encode::entropy::symbol_coding::Err ),
    #[error("Too many handles.")]
    HandleSizeTooLarge,
    #[error("Too many holes.")]
    HoleSizeTooLarge,
    #[error("The input mesh is non-orientable.")]
    NonOrientable,
    #[error("Rabs coder error: {0}")]
    RabsCoderError(#[from] rans::Err),
    #[error("The input mesh has too many connected components: {0}")]
    TooManyConnectedComponents(usize),
}

impl<'faces, T> Edgebreaker<'faces, T>
    where T: Traversal
{
	// Build the object with empty arrays.
	pub fn new(config: Config, atts: &[Attribute], faces: &'faces [[FaceIdx; 3]]) -> Result<Self, Err> {
        let corner_table = if config.use_single_connectivity {
            CornerTable::new(faces)
        } else {
            let pos_att = atts.iter()
                .find(|att| att.get_attribute_type() == AttributeType::Position)
                .unwrap();
            let new_faces = faces.iter()
                .map(|f| [pos_att.get_att_idx(f[0]), pos_att.get_att_idx(f[1]), pos_att.get_att_idx(f[2])])
                .collect::<Vec<_>>();
            CornerTable::new_with_taken_faces(new_faces)
        };

        let traversal = T::new(&corner_table);

        let attribute_encoding_data = Self::init_attribute_data(atts, &corner_table, &config)?;

        let mut out = Self {
            edges: Vec::new(),
            coboundary_map_one: Vec::new(),
            face_connectivity: Vec::new(),
            coboundary_map_zero: None,
            lies_on_boundary_or_cutting_path: Vec::new(),
            visited_vertices: Vec::new(),
            visited_faces: Vec::new(),
            corner_table,
            visited_holes: Vec::new(),
            vertex_hole_id: Vec::new(),
            face_orientation: Vec::new(),
            active_edge_face_idx_stack: Vec::new(),
            cutting_paths: Vec::new(),
            symbols: Vec::new(),
            symbol_idx_to_face_idx: Vec::new(),
            vertex_decode_order: Vec::new(),
            face_decode_order: Vec::new(),
            num_decoded_vertices: 0,
            corner_traversal_stack: Vec::new(),
            last_encoded_symbol_idx: usize::MAX,
            processed_connectivity_corners: Vec::new(),
            face_to_split_symbol_map: BTreeMap::new(),
            num_split_symbols: 0,
            vertex_traversal_length: Vec::new(),
            init_face_connectivity_corners: Vec::new(),
            traversal,
            prev_face_idx: usize::MAX,
            signs_of_faces: Vec::new(),
            topology_splits: Vec::new(),
            map_face_idx_to_split_symbol_idx: BTreeMap::new(),
            corners_for_connected_components: Vec::new(),
            attribute_encoding_data,
            config,
        };

        let num_vertices = out.corner_table.num_vertices();
        out.visited_vertices = vec!(false; num_vertices);
        out.visited_faces = vec!(false; faces.len());
        out.face_orientation = vec!(false; faces.len());

        out.coboundary_map_zero = None;
        out.lies_on_boundary_or_cutting_path = vec![false; num_vertices];

        out.vertex_decode_order = vec![usize::MAX; num_vertices];
        out.face_decode_order = vec![usize::MAX; faces.len()];
        out.num_decoded_vertices = 0;

        Ok(out)
	}

    fn init_attribute_data(atts: &[Attribute], corner_table: &CornerTable, config: &Config) -> Result<Vec<AttributeCornerTable>, Err> {
        let num_attributes = atts.len();
        if config.use_single_connectivity && num_attributes==1 {
            // Each attribute refers to the same connectivity attribute, so no need to create attriibute encoding data.
            return Ok(Vec::new());
        }

        
        // Ignore the position attribute as it is decoded separately.
        let mut attribute_encoding_data = Vec::with_capacity(num_attributes - 1);

        for i in 0..num_attributes {
            // skip the position attribute
            let att = &atts[i];
            if att.get_attribute_type() == AttributeType::Position {
                continue;
            }
            let att_connectivity = AttributeCornerTable::new(&corner_table, att);
            attribute_encoding_data.push(att_connectivity);
        }

        Ok(attribute_encoding_data)
    }

    /// computes all the edges of the mesh and returns the raw coboundary map.
    fn compute_edges(&mut self, faces: &[[VertexIdx; 3]]) {
        // input faces must be sorted.
        debug_assert!(faces.is_sorted(), "Faces are not sorted");

        // initialize the face connectivity
        self.face_connectivity = (0..faces.len()).map(|i|[i,i,i]).collect();

        let mut edges = Vec::new();
        edges.reserve(faces.len()*3);
        for (face_idx, face) in faces.iter().enumerate() {
            edges.push([face[0], face[1], face_idx]);
            edges.push([face[0], face[2], face_idx]);
            edges.push([face[1], face[2], face_idx]);
        }
        edges.sort();

        let mut i = 0;
        self.coboundary_map_one.clear();
        self.coboundary_map_one.reserve(edges.len());
        while i < edges.len() {
            self.edges.push([edges[i][0],edges[i][1]]);
            self.coboundary_map_one.push(vec!(edges[i][2]));
            let mut j = i+1;
            let coboundary = self.coboundary_map_one.last_mut().unwrap();
            while j < edges.len() && edges[i][..2] == edges[j][..2] {
                coboundary.push(edges[j][2]);
                j += 1;
            }
            coboundary.sort();
            coboundary.dedup();

            if coboundary.len() > 2 || coboundary.len() == 0 {
                unimplemented!("the mesh is not homeomorphic to a manifold with boundary, and out current Edgebreaker cannot handle this case yet.");
            }

            if coboundary.len()==2 {
                let idx = self.face_connectivity[coboundary[0]].iter().position(|f_idx| f_idx == &coboundary[0]).unwrap();
                self.face_connectivity[coboundary[0]][idx] = coboundary[1];
                let idx = self.face_connectivity[coboundary[1]].iter().position(|f_idx| f_idx == &coboundary[1]).unwrap();
                self.face_connectivity[coboundary[1]][idx] = coboundary[0];
            }
            
            // update edge valency
            let edge = edges[i];
            if coboundary.len() != 2 {
                self.lies_on_boundary_or_cutting_path[edge[0]]=true;
                self.lies_on_boundary_or_cutting_path[edge[1]]=true;
            }
            i = j;
        }

        debug_assert!(self.edges.is_sorted(), "Edges are not sorted: edges: {:?}", self.edges);
    }


    fn compute_coboundary_map_zero(&mut self) {
        self.coboundary_map_zero = {
            let mut out = vec![Vec::new(); self.edges.iter().flatten().max().unwrap()+1];
            for (edges_idx, e) in self.edges.iter().enumerate() {
                out[e[0]].push(edges_idx);
                out[e[1]].push(edges_idx);
            }
            Some(out)
        };
    }

    /// Change the numbering of the vertices so that the earliest face in each connected component
    /// has the order such that the permutation from the face to the sorted face if of positive sign.
    fn swap_vertices_and_reserve_orientation(&mut self, faces: &mut [[VertexIdx; 3]], children: &mut [&mut Attribute]) {
        let mut visited_vertices = vec![false; faces.iter().flatten().max().unwrap()+1];
        for f_idx in 0..faces.len() {
            let f = faces[f_idx];
            if f.iter().any(|&v| visited_vertices[v]) {
                f.iter().for_each(|v| 
                    visited_vertices[*v] = true
                );
                continue;
            }

            // coming here means that the face is the initial face of a connected component.
            // in this case, we need to swap a pair of vertices of the face if the sign of the permutation
            // from the face to the sorted face is negative.
            if !sign_of(f) {
                // swap the first two vertices of the face.
                // this change affects the whole mesh.
                let v1 = f[0];
                let v2 = f[1];
                for f in faces.iter_mut() {
                    for v in f.iter_mut() {
                        if *v == v1 {
                            *v = v2;
                        } else if *v == v2 {
                            *v = v1;
                        }
                    }
                }
                
                // also swap the vertices in the attributes.
                for att in children.iter_mut() {
                    att.swap(v1, v2);
                }
            }
            
            f.iter().for_each(|&v| 
                visited_vertices[v] = true
            );
        }
    }


    fn check_orientability(&mut self, faces: &[[VertexIdx; 3]]) -> Result<(), Err> {
        // we use 'visited_faces' to store the orientation of the faces.
        // since we use this for Edgebreaker as well, this must be cleared at the end of the function.
        debug_assert!(self.visited_faces==vec!(false; faces.len()));

        // loop over the connected components.
        for start in 0..faces.len() {
            // safety: 'start' is always in bounds since 'faces.len()==self.visited_faces.len()'.
            if unsafe{ *self.visited_faces.get_unchecked(start) } { continue }

            let mut face_queue = VecDeque::new();
            face_queue.push_back(start);
            self.face_orientation[start] = true;

            // loop over the faces in the connected component.
            while let Some(face_idx) = face_queue.pop_front() {
                // safety: 'face_idx' is always in bounds since 'faces.len()==self.visited_faces.len()'.
                if unsafe{ *self.visited_faces.get_unchecked(face_idx) } { continue }
                self.visited_faces[face_idx] = true;
                
                // For each adjacent face, toggle orientation if needed and check for conflicts.
                // If a conflict is found, return Err(Err::NonOrientable).
                // Otherwise, push unchecked neighbors to the queue.
                let face = faces[face_idx];
                for edge in [[face[0], face[1]], [face[0], face[2]], [face[1], face[2]]] {
                    // ToDo: this binary search can be optimized.
                    let edge_idx = self.edges.binary_search(&edge).unwrap();

                    if self.coboundary_map_one[edge_idx].len() != 2 {
                        continue;
                    }
                    
                    for &adjacent_face_idx in &self.coboundary_map_one[edge_idx] {
                        if adjacent_face_idx == face_idx {
                            continue;
                        }

                        use crate::shared::connectivity::edgebreaker::orientation_of_next_face;
                        let face = faces[face_idx];
                        let adjacent_face = faces[adjacent_face_idx];
                        
                        let orientation_of_adjacent_face = orientation_of_next_face(face, self.face_orientation[face_idx], edge, adjacent_face);

                        unsafe {
                            if *self.visited_faces.get_unchecked(adjacent_face_idx) {
                                if *self.face_orientation.get_unchecked(adjacent_face_idx)^orientation_of_adjacent_face {
                                    // If we detect a mismatch in orientation here, return an error.
                                    return Err(Err::NonOrientable);
                                }
                            } else {
                                *self.face_orientation.get_unchecked_mut(adjacent_face_idx) = orientation_of_adjacent_face;
                                face_queue.push_back(adjacent_face_idx);
                            }
                        }
                    }
                }
            }
        }

        self.visited_faces = vec!(false; faces.len());

        Ok(())
    }


    fn compute_boundaries(&mut self) -> Result<(), Err> {
        self.vertex_hole_id = vec![None; self.corner_table.num_vertices()];
        for c in 0..self.corner_table.num_corners() {
            if self.corner_table.opposite(c).is_none() {
                // 'c' is on a boundary.
                let mut v = self.corner_table.vertex_idx(self.corner_table.next(c));
                if self.vertex_hole_id[v].is_some() {
                    // The hole is already processed.
                    continue;
                }
                // Now we have found a new boundary containing the vertex 'v'.
                let boundary_idx = self.visited_holes.len();
                self.visited_holes.push(false);

                let mut c = c;
                while self.vertex_hole_id[v].is_none() {
                    self.vertex_hole_id[v] = Some(boundary_idx);
                    c = self.corner_table.next(c);

                    while self.corner_table.opposite(c).is_some() {
                        c = self.corner_table.next(c);
                    }
                    // Id of the next vertex in the vertex on the hole.
                    v = self.corner_table.vertex_idx(self.corner_table.next(c));
                }
            }
        }
        Ok(())
    }

    fn process_boundary(
        &mut self,
        start_corner: CornerIdx,
        encode_first_vertex: bool,
    ) -> usize {
        let mut corner = self.corner_table.previous(start_corner);
        while let Some(opp) = self.corner_table.opposite(corner) {
            corner = self.corner_table.next(opp);
        } // 'corner' now faces the hole

        let start_v = self.corner_table.vertex_idx(start_corner);

        let mut num_encoded_hole_verts = 0;
        if encode_first_vertex {
            self.visited_vertices[start_v] = true;
            num_encoded_hole_verts += 1;
        }

        self.visited_holes[self.vertex_hole_id[start_v].unwrap()] = true; // it is safe to unwrap here as start_v is on a hole.
        let mut curr_v = self.corner_table.vertex_idx(self.corner_table.previous(corner));
        while curr_v != start_v {
            self.visited_vertices[curr_v] = true;
            num_encoded_hole_verts += 1;
            corner = self.corner_table.next(corner);
            while let Some(opp) = self.corner_table.opposite(corner) {
                corner = self.corner_table.next(opp);
            }
            curr_v = self.corner_table.vertex_idx(self.corner_table.previous(corner));
        }
        num_encoded_hole_verts
    }

	
	
	/// A function implementing a step of the Edgebreaker algorithm.
	/// When this function returns, all the CLERS symbols are written to the
	/// buffer. Since the time complexity of 'find_vertices_pinching()' 
	/// is O(1), the complexity of this function (single recursive step) is also O(1).
	fn edgebreaker_from(&mut self, mut c: CornerIdx) -> Result<(), Err> {
        println!("edgebreaker_from: c={}", c);
        self.corner_traversal_stack.clear();
        self.corner_traversal_stack.push(c);
        let num_faces = self.corner_table.num_faces();
        let mut most_recent_processed_corner = c;
        while let Some(&start) = self.corner_traversal_stack.last() {
            c = start;
            // Make sure the face hasn't been visited yet.
            if self.visited_faces[self.corner_table.face_idx_containing(c)] {
                self.corner_traversal_stack.pop();
                continue;
            }

            let mut num_visited_faces = 0;
            while num_visited_faces < num_faces {
                num_visited_faces += 1;
                self.last_encoded_symbol_idx = self.last_encoded_symbol_idx.wrapping_add(1); // since the initial value of 'last_encoded_symbol_idx' is usize::MAX, we do wrapping add.

                let face_idx = self.corner_table.face_idx_containing(c);
                self.visited_faces[face_idx] = true;
                self.processed_connectivity_corners.push(c);
                self.traversal.new_corner_reached(c);
                let v = self.corner_table.vertex_idx(c);
                if !self.visited_vertices[v] {
                    self.visited_vertices[v] = true;
                    if self.vertex_hole_id[v].is_none() {
                        print!("C");
                        self.traversal.record_symbol(Symbol::C, &self.visited_faces,  &self.corner_table);
                        most_recent_processed_corner = c;
                        c = self.corner_table.get_right_corner(c).unwrap(); // unwrap is safe here; we checked that the right edge is not on a boundary, and this implies that the right face exists.
                        continue;
                    }
                }
                let maybe_right_c = self.corner_table.get_right_corner(c);
                let maybe_left_c = self.corner_table.get_left_corner(c);
                let maybe_right_face = maybe_right_c.map(|c| self.corner_table.face_idx_containing(c));
                let maybe_left_face = maybe_left_c.map(|c| self.corner_table.face_idx_containing(c));
                if self.is_right_face_visited(c) {
                    if let Some(right_face) = maybe_right_face {
                        self.check_and_store_topology_split_event(
                            self.last_encoded_symbol_idx,
                            Orientation::Right,
                            right_face
                        );
                    }
                    if self.is_left_face_visited(c) {
                        // 'E' symbol
                        if let Some(left_face) = maybe_left_face {
                            self.check_and_store_topology_split_event(
                                self.last_encoded_symbol_idx,
                                Orientation::Left,
                                left_face
                            );
                        }
                        print!("E");
                        self.traversal.record_symbol(Symbol::E, &self.visited_faces, &self.corner_table);
                        most_recent_processed_corner = c;
                        self.corner_traversal_stack.pop();
                        // End of a branch of the traversal.
                        break;
                    } else {
                        // 'R' symbol
                        print!("R");
                        self.traversal.record_symbol(Symbol::R, &self.visited_faces, &self.corner_table);
                        most_recent_processed_corner = c;
                        c = maybe_left_c.unwrap(); // unwrap is safe here; we checked that the left face is not visited, which implies that the left face exist.
                    }
                } else {
                    if self.is_left_face_visited(c) {
                        // 'L' symbol
                        if let Some(left_face) = maybe_left_face {
                            self.check_and_store_topology_split_event(
                                self.last_encoded_symbol_idx,
                                Orientation::Left,
                                left_face
                            );
                        }
                        print!("L");
                        self.traversal.record_symbol(Symbol::L, &self.visited_faces, &self.corner_table);
                        most_recent_processed_corner = c;
                        c = maybe_right_c.unwrap(); // unwrap is safe here; we checked that the right face is not visited, which implies that the right face exist.
                    } else {
                        print!("S");
                        self.traversal.record_symbol(Symbol::S, &self.visited_faces, &self.corner_table);
                        most_recent_processed_corner = c;
                        self.num_split_symbols += 1;
                        if let Some(hole_idx) = self.vertex_hole_id[v] {
                            if !self.visited_holes[hole_idx] {
                                self.process_boundary(c, false);
                            }
                        }
                        self.face_to_split_symbol_map.insert(face_idx, self.last_encoded_symbol_idx);
                        *self.corner_traversal_stack.last_mut().unwrap() = maybe_left_c.unwrap();
                        self.corner_traversal_stack.push(maybe_right_c.unwrap());
                        break;
                    }
                }
            }
        }
        println!();
        self.corners_for_connected_components.push(most_recent_processed_corner);
        Ok(())
    }

    fn the_other_vertex(edge: [VertexIdx; 2], face: [VertexIdx; 3]) -> VertexIdx {
        debug_assert!(face.contains(&edge[0]) && face.contains(&edge[1]));
        debug_assert!(edge.is_sorted());
        debug_assert!(face.is_sorted());
        debug_assert!(edge[0]!=edge[1]);
        debug_assert!(face[0]!=face[1] && face[0]!=face[2] && face[1]!=face[2]);
        if edge[1] == face[1] {
            face[2]
        } else if edge[0] == face[1] {
            face[0]
        } else {
            face[1]
        }
    }


    /// Checks whether the right face of the corner 'c' is visited.
    /// If the corner is on a boundary and if the right face does not exist,
    /// then it returns true by convention.
    fn is_right_face_visited(&self, c: CornerIdx) -> bool {
        if let Some(c_r) = self.corner_table.get_right_corner(c) {
            self.visited_faces[self.corner_table.face_idx_containing(c_r)]
        } else {
            true
        }
    }

    /// Checks whether the left face of the corner 'c' is visited.
    /// If the corner is on a boundary and if the left face does not exist,
    /// then it returns true by convention.
    fn is_left_face_visited(&self, c: CornerIdx) -> bool {
        if let Some(c_l) = self.corner_table.get_left_corner(c) {
            self.visited_faces[self.corner_table.face_idx_containing(c_l)]
        } else {
            true
        }
    }


    fn encode_topology_splits<W>(&mut self, writer: &mut W) -> Result<(), Err> 
        where W: ByteWriter,
    {
        #[cfg(feature = "evaluation")]
        {
            let mut string = String::new();
            for split in self.topology_splits.iter() {
                string.push_str(&format!("{}:{}({:?}) ", split.merging_symbol_idx, split.split_symbol_idx, split.merging_edge_orientation));
            }
            eval::write_json_pair("topology_splits", serde_json::Value::from(string), writer);
        }
        let mut last_idx = 0;
        // write the number of topology splits.
        leb128_write(self.topology_splits.len() as u64, writer);
        println!("num events: {}", self.topology_splits.len());
        for split in self.topology_splits.iter() {
            println!("source symbol idx: {}\nsplit symbol idx: {}", 
                split.merging_symbol_idx - last_idx,
                split.merging_symbol_idx - split.split_symbol_idx
            );
            leb128_write((split.merging_symbol_idx - last_idx) as u64, writer);
            leb128_write((split.merging_symbol_idx - split.split_symbol_idx) as u64, writer);
            last_idx = split.merging_symbol_idx;
        }
        let mut bit_coder: BitWriter<'_, W, LsbFirst> = BitWriter::spown_from(writer);
        for split in self.topology_splits.iter() {
            let orientation = match split.merging_edge_orientation {
                Orientation::Left => (1,0),
                Orientation::Right => (1,1),
            };
            println!("orientation: {:?}", orientation);
            bit_coder.write_bits(orientation);
        }
        Ok(())
    }

    /// Begins the Edgebreaker iteration from the given face.
    /// The first boolean indicates whether the face is interior (i.e. the face does not touch a boundary) or not.
    /// The second 'usize' element is a corner chosen as follows:
    /// It chooses the first corner of the face as the starting point is such a way that corner faces the the boundary
    /// if the face is on the boundary.
    /// If the face is not on the boundary, then it returns the input corner.
    fn begin_from(&mut self, face_idx: usize) -> (bool, usize) {
        let mut corner_index = 3 * face_idx;
        for _ in 0..3 {
            if self.corner_table.opposite(corner_index).is_none() {
                // corner faces a boundary
                return (false, corner_index);
            }
            if self.vertex_hole_id[self.corner_table.vertex_idx(corner_index)].is_some() {
                // The corner is on a boundary.
                let mut maybe_right_corner = Some(corner_index);
                while let Some(right_corner) = maybe_right_corner {
                    corner_index = right_corner;
                    maybe_right_corner = self.corner_table.swing_right(right_corner);
                }
                let start_corner = self.corner_table.previous(corner_index);
                return (false, start_corner);
            }
            corner_index = self.corner_table.next(corner_index);
        }
        (true, corner_index)
    }


    fn check_and_store_topology_split_event(&mut self, merging_symbol_idx: usize, merging_edge_orientation: Orientation, split_face_idx: usize) {
        let split_symbol_idx = if let Some(&idx) = self.face_to_split_symbol_map.get(&split_face_idx) {
            idx
        } else {
            // The face is not split, so we do not need to store the split event.
            return;
        };
        let split = TopologySplit {
            merging_symbol_idx,
            split_symbol_idx,
            merging_edge_orientation,
        };

        self.topology_splits.push(split);
    }


    fn mark_vertices_as_visited_in_boundary_containing(&mut self, e_idx: EdgeIdx) -> Result<usize, Err> {
        self.mark_vertices_along_edges_of_valency::<1>(e_idx)
    }

    fn mark_vertices_as_visited_in_cutting_path_containing(&mut self, e_idx: EdgeIdx) -> Result<usize, Err> {
        let valency = self.coboundary_map_one[e_idx].len();
        match valency {
            3 => self.mark_vertices_along_edges_of_valency::<3>(e_idx),
            4 => self.mark_vertices_along_edges_of_valency::<4>(e_idx),
            5 => self.mark_vertices_along_edges_of_valency::<5>(e_idx),
            _ => unreachable!("An internal error occured while running Edgebreaker."),
        }
    }

    fn mark_vertices_along_edges_of_valency<const VALENCY: usize>(&mut self, e_idx: EdgeIdx) -> Result<usize, Err> {
        // we need to cut the mesh at this edge.
        // first create a zero dimensional coboundary map if not already created.
        if self.coboundary_map_zero.is_none() {
            self.compute_coboundary_map_zero();
        };

        // Safety: The coboundary map is created just now.
        let coboundary_map_zero = unsafe{ self.coboundary_map_zero.as_ref().unwrap_unchecked() };

        let mut num_visited_points= 2;

        // now compute the loop that we are going to cut along.
        let mut cutting_path = Vec::<VertexIdx>::from(self.edges[e_idx]);
        while cutting_path.first().unwrap() != cutting_path.last().unwrap() {
            let tail = *cutting_path.last().unwrap();
            let next_vertex = if let Some((_,e)) = 
                // ToDo: this can be optimized.
                coboundary_map_zero[tail].iter()
                    .map(|&idx| (idx, self.edges[idx]) )
                    .filter(|(_,e)| !e.contains(&cutting_path[cutting_path.len()-2]))
                    .find(|&(idx,_)| self.coboundary_map_one[idx].len()==VALENCY)
            {
                if e[0]==tail {e[1]} else {e[0]}
            } else {
                // The path is not a loop
                break;
            };
            self.visited_vertices[next_vertex] = true;
            num_visited_points += 1;
            cutting_path.push(next_vertex);
        }

        // if the path is not a loop, then we have to traverse the other direction as well
        if cutting_path.first().unwrap() != cutting_path.last().unwrap() {
            cutting_path.reverse();
            loop {
                let tail = *cutting_path.last().unwrap();
                let next_vertex = if let Some((_,e)) = 
                    // ToDo: this can be optimized.
                    self.edges.iter().enumerate()
                        .filter(|(_,e)| e[0] == tail || e[1] == tail)
                        .filter(|(_,e)| !e.contains(&cutting_path[cutting_path.len()-2]))
                        .find(|&(idx,_)| self.coboundary_map_one[idx].len()==VALENCY)
                {
                    if e[0]==tail {e[1]} else {e[0]}
                } else {
                    break;
                };
                self.visited_vertices[next_vertex] = true;
                num_visited_points += 1;
                cutting_path.push(next_vertex);
            }
        } else {
            num_visited_points -= 1;
        }

        self.cutting_paths.push(cutting_path);

        Ok(num_visited_points)
    }

    fn are_edges_connected_via_unvisited_faces(&self, f_idx: usize, edge_idx1: EdgeIdx, edge_idx2: EdgeIdx, faces: &[[VertexIdx; 3]]) -> bool {
        let mut visited_faces = vec![false; faces.len()];
        visited_faces[f_idx] = true;
        let mut edge_idx_stack = vec![edge_idx1];

        while let Some(edge_idx) = edge_idx_stack.pop() {
            let face = if let Some(&face_idx) = self.coboundary_map_one[edge_idx].iter().find(|&&f_idx| 
                    !visited_faces[f_idx]&&!self.visited_faces[f_idx]
                )
            {
                visited_faces[face_idx] = true;
                faces[face_idx]
            } else {
                continue;
            };

            let mut new_edges = vec![
                [face[0], face[1]],
                [face[0], face[2]],
                [face[1], face[2]],
            ];
            
            let edge = self.edges[edge_idx];
            new_edges.remove(
                new_edges.binary_search(&edge).unwrap()
            );

            let mut new_edges_idx = new_edges.iter()
                .map(|&e| self.edges.binary_search(&e).unwrap())
                .collect::<Vec<_>>();

            if new_edges_idx.contains(&edge_idx2) {
                return true;
            }

            edge_idx_stack.append(&mut new_edges_idx);
        }
        false
    }

    #[allow(dead_code)]
    fn measure_hole_size(&mut self, face_idx: usize, next_vertex: usize, left_vertex: usize, faces: &[[VertexIdx; 3]]) -> usize {
        let mut curr_vertex = left_vertex;
        let mut prev_vertex = next_vertex;

        let mut vertex_counter = 0;

        while curr_vertex != next_vertex {
            let mut tmp_vertex = prev_vertex;
            let mut tmp_edge = if curr_vertex < prev_vertex {
                [curr_vertex, prev_vertex]
            } else {
                [prev_vertex, curr_vertex]
            };
            let mut tmp_edge_idx = self.edges.binary_search(&tmp_edge).unwrap();
            let mut prev_face_idx = face_idx;
            while let Some(&face_idx) = self.coboundary_map_one[tmp_edge_idx]
                .iter()
                .find(|&&f_idx| 
                    !self.visited_faces[f_idx] && 
                    f_idx!=prev_face_idx &&
                    f_idx!=face_idx
                ) 
            {
                let face = faces[face_idx];
                tmp_vertex = Self::the_other_vertex(tmp_edge, face);
                tmp_edge = if curr_vertex < tmp_vertex {
                    [curr_vertex, tmp_vertex]
                } else {
                    [tmp_vertex, curr_vertex]
                };
                tmp_edge_idx = self.edges.binary_search(&tmp_edge).unwrap();
                prev_face_idx = face_idx;
            }
            let next_vertex = tmp_vertex;

            prev_vertex = curr_vertex;
            curr_vertex = next_vertex;
            vertex_counter += 1;
        }

        vertex_counter
    }


    fn compute_decode_order(&mut self, faces: &[[VertexIdx; 3]]) {
        debug_assert!(
            self.symbol_idx_to_face_idx.len() == self.symbols.len(), 
            "symbol_idx_to_face_idx.len(): {}, symbols.len(): {}", 
            self.symbol_idx_to_face_idx.len(), self.symbols.len()
        );

        // fill 'face_decode_order'.
        for (i, face_idx) in self.symbol_idx_to_face_idx.iter().rev().enumerate() {
            self.face_decode_order[*face_idx] = i;
        }

        // create a map from 'E' symbols to the most recent 'S' symbol.
        let mut e_to_s_map = BTreeMap::new();
        let mut most_reccent_s_idx = usize::MAX;
        let mut e_count = 0;
        for (i, &symbol) in self.symbols.iter().enumerate().rev() {
            match symbol {
                Symbol::S => {
                    most_reccent_s_idx = i;
                    e_count = 0;
                },
                Symbol::E => {
                    if most_reccent_s_idx != usize::MAX {
                        e_to_s_map.insert(i, most_reccent_s_idx);
                        if e_count == 1 {
                            // if this is the second 'E' after the last 'S', then the current 'S'
                            // should never be used again.
                            most_reccent_s_idx = usize::MAX;
                        } else {
                            e_count += 1;
                        }
                    }
                },
                _ => {}
            }
        }

        // fill 'vertex_decode_order'.
        for i in (0..self.symbols.len()).rev() {
            let symbol = self.symbols[i];
            let face = faces[self.symbol_idx_to_face_idx[i]];
            match symbol {
                Symbol::E => {
                    // get the most recent adjavent face.
                    // if there is no such adjacent face, then this 'E' symbol is isolated i.e. the solo triangle
                    // of the symbol forms a connected component.
                    let adjacent_face_index = if i>0 && edge_shared_by(&faces[self.symbol_idx_to_face_idx[i-1]], &face).is_some() {
                        self.symbol_idx_to_face_idx[i-1]
                    } else {
                        let parent_s_idx = if let Some(parent_s_idx) = e_to_s_map.get(&i) {
                            *parent_s_idx
                        } else {
                            // if there is no parent 'S' symbol, then this 'E' symbol is isolated.
                            for v in face {
                                self.vertex_decode_order[v] = self.num_decoded_vertices;
                                self.num_decoded_vertices += 1;
                            }
                            continue;
                        };
                        self.symbol_idx_to_face_idx[parent_s_idx]
                    };

                    let adjacent_face = faces[adjacent_face_index];
                    debug_assert!(
                        face.iter().filter(|v| adjacent_face.contains(v)).count() == 2, 
                        "face: {:?}, adjacent_face: {:?}", 
                        face, adjacent_face
                    );  

                    // find the edge shared by the two faces,
                    // and sort the face such that the shared edge comes first.

                    // ToDo: change symbol_idx_to_face_idx to record the index of the faces
                    // instead of the face itself, so that the following binary search will be 
                    // unnecessary.
                    let i_face = faces.binary_search(&face).unwrap();
                    let sorted_face = if !adjacent_face.contains(&face[0]) {
                        let edge = [face[1], face[2]];
                        let [right, left] = if (edge[0] < face[0] && face[0] < edge[1]) ^ self.face_orientation[i_face] {
                            edge
                        } else {
                            [edge[1], edge[0]]
                        };
                        [right, left, face[0]]
                    } else if !adjacent_face.contains(&face[1]) {
                        let edge = [face[0], face[2]];
                        let [right, left] = if (edge[0] < face[1] && face[1] < edge[1]) ^ self.face_orientation[i_face] {
                            edge
                        } else {
                            [edge[1], edge[0]]
                        };
                        [right, left, face[1]]
                    } else {
                        let edge = [face[0], face[1]];
                        let [right, left] = if (edge[0] < face[2] && face[2] < edge[1]) ^ self.face_orientation[i_face] {
                            edge
                        } else {
                            [edge[1], edge[0]]
                        };
                        [right, left, face[2]]
                    };
                    for v in sorted_face {
                        if self.vertex_decode_order[v] == usize::MAX {
                            self.vertex_decode_order[v] = self.num_decoded_vertices;
                            self.num_decoded_vertices += 1;
                        }
                    }
                },
                Symbol::L => {
                    for v in face {
                        if self.vertex_decode_order[v] == usize::MAX {
                            self.vertex_decode_order[v] = self.num_decoded_vertices;
                            self.num_decoded_vertices += 1;
                        }
                    }
                },
                Symbol::R => {
                    for v in face {
                        if self.vertex_decode_order[v] == usize::MAX {
                            self.vertex_decode_order[v] = self.num_decoded_vertices;
                            self.num_decoded_vertices += 1;
                        }
                    }
                },
                _ => {}
            }
        }
    }

    fn reflect_decode_order(&mut self, faces: &mut [[VertexIdx; 3]], children: &mut [&mut Attribute]) 
    {
        debug_assert!(
            self.vertex_decode_order.iter().all(|&v| v!= usize::MAX), 
            "Not all vertices are computed. Vertices not computed are: \n{:?}", 
            self.vertex_decode_order.iter()
                .enumerate()
                .filter(|(_, &v)| v == usize::MAX)
                .map(|x|x.0)
                .collect::<Vec<_>>()
        );
        // reflect the vertex order.
        for f in &mut *faces {
            for v in f {
                *v = self.vertex_decode_order[*v];
            }
        }
        // sort each face.
        for f in faces.iter_mut() {
            f.sort();
        }
        
        // sort the faces.
        let mut new_faces = vec![[0;3]; faces.len()];
        let mut new_signs_of_faces = vec![false; faces.len()];
        for (i, &f_idx) in self.face_decode_order.iter().enumerate() {
            new_faces[f_idx] = faces[i];
            new_signs_of_faces[f_idx] = self.signs_of_faces[i];
        }
        for (a,b) in faces.iter_mut().zip(new_faces.into_iter()) {   
            *a = b;
        }
        for (a,b) in self.signs_of_faces.iter_mut().zip(new_signs_of_faces.into_iter()) {   
            *a = b;
        }

        self.compute_orientation_of_faces(faces);
        
        // reflect the vertex order to the child attributes.
        // ToDo: Optimize this.
        for att in children.iter_mut() {
            att.permute(&self.vertex_decode_order);
        }
    }

    fn compute_orientation_of_faces(&self, faces: &mut [[VertexIdx; 3]]) {
        let mut orientation_of_faces = vec![false; faces.len()];
        let mut visited_faces = vec![false; faces.len()];
        for i in (0..faces.len()).rev() {
            if visited_faces[i] {
                continue;
            }
            visited_faces[i] = true;
            let mut face_stack = vec![i];
            orientation_of_faces[i] = self.signs_of_faces[i];
            while let Some(face_idx) = face_stack.pop() {
                let face = faces[face_idx];
                
                let adjacent_faces = (0..faces.len())
                    .rev()
                    .filter(|f_idx| !visited_faces[*f_idx])
                    .filter_map(|f_idx| edge_shared_by(&face, &faces[f_idx]).map(|e| (e, f_idx)))
                    .take(2)
                    .collect::<Vec<_>>();

                for (shared_edge, adj_face_idx) in adjacent_faces {
                    visited_faces[adj_face_idx] = true;
                    face_stack.push(adj_face_idx);

                    let adj_face = faces[adj_face_idx];
                    orientation_of_faces[adj_face_idx] = orientation_of_next_face(
                        face, 
                        orientation_of_faces[face_idx], 
                        shared_edge, 
                        adj_face
                    );
                }
            }
        }
        for (f, o) in faces.iter_mut().zip(orientation_of_faces) {
            if !o {
                f.swap(1, 2);
            }
        }
    }
}	

impl<'faces, T> ConnectivityEncoder for Edgebreaker<'faces, T> 
    where T: Traversal
{
    type Config = Config;
	type Err = Err;
    type Output = Output<'faces>;
	/// The main encoding paradigm for Edgebreaker.
    fn encode_connectivity<W>(
        mut self, 
        faces: &[[VertexIdx; 3]], 
        writer: &mut W
    ) -> Result<Self::Output, Self::Err> 
        where W: ByteWriter
    {
        debug_write!("Init Decoder", writer);
        // encode the traversal decoder type
        EdgebreakerKind::Standard.write_to(writer);
        debug_write!("Init Decoder Done", writer);

        self.compute_boundaries()?;

        leb128_write(self.corner_table.num_vertices() as u64, writer);
        leb128_write(faces.len() as u64, writer);

        writer.write_u8(self.attribute_encoding_data.len() as u8);

		// Run Edgebreaker once for each connected component.
		for c in 0..self.corner_table.num_corners() {
            let face_idx = self.corner_table.face_idx_containing(c);
            if self.visited_faces[face_idx] {
                // if the face is already visited, then skip it.
                continue;
            }

            let (is_start_face_interior, start_corner) = self.begin_from(face_idx);

            self.traversal.record_start_face_config(is_start_face_interior);

            if is_start_face_interior {
                let corner_index = start_corner;
                let v = self.corner_table.vertex_idx(corner_index);
                let n = self.corner_table.vertex_idx(self.corner_table.next(corner_index));
                let p = self.corner_table.vertex_idx(self.corner_table.previous(corner_index));
                self.visited_vertices[v] = true;
                self.visited_vertices[n] = true;
                self.visited_vertices[p] = true;

                self.vertex_traversal_length.push(1);

                self.visited_faces[face_idx] = true;
                
                self.init_face_connectivity_corners.push(self.corner_table.next(corner_index));
                let corner_opp = self.corner_table.opposite(self.corner_table.next(corner_index)).unwrap(); // it is safe to unwrap since the face is interior.
                self.edgebreaker_from(corner_opp)?;
            } else {
                // if the face is on the boundary, then we start from the boundary.
                self.process_boundary(self.corner_table.next(start_corner), true);
                self.edgebreaker_from(start_corner)?;
            }
		}

        // write the number of symbols.
        println!("Number of symbols: {}", self.traversal.num_symbols());
        leb128_write(self.traversal.num_symbols() as u64, writer);

        // write the number of encoded split symbols.
        println!("Number of split symbols: {}", self.num_split_symbols);
        leb128_write(self.num_split_symbols as u64, writer);

        self.encode_topology_splits(writer)?;
        // encode the edgebreaker symbols.
        self.traversal.encode(writer)?;
        // encode the start face information.



        Ok( Output{
            corner_table: AllInclusiveCornerTable::new(self.corner_table, self.attribute_encoding_data, ),
            corners_for_connected_components: self.corners_for_connected_components,
        })
	}
}
    


pub(crate) trait Traversal {
    fn new(corner_table: & CornerTable<'_>) -> Self;
    fn record_symbol(&mut self, symbol: Symbol, visited_faces: &[bool], corner_table: & CornerTable<'_>);
    fn record_start_face_config(&mut self, interior_cfg: bool);
    fn new_corner_reached(&mut self, corner: CornerIdx);
    fn num_symbols(&self) -> usize;
    fn encode<W>(self, writer: &mut W) -> Result<(), Err> where W: ByteWriter;
}

pub(crate) struct DefaultTraversal {
    symbols: Vec<Symbol>,
    interior_cfg: Vec<bool>,
}

impl Traversal for DefaultTraversal {
    fn new(_corner_table: & CornerTable<'_>) -> Self {
        Self { symbols: Vec::new(), interior_cfg: Vec::new() }
    }

    fn record_symbol(&mut self, symbol: Symbol, _visited_faces: &[bool], _corner_table: & CornerTable<'_>) {
        self.symbols.push(symbol);
    }

    fn new_corner_reached(&mut self, _corner: CornerIdx) {}

    fn record_start_face_config(&mut self, interior_cfg: bool) {
        self.interior_cfg.push(interior_cfg);
    }

    fn num_symbols(&self) -> usize {
        self.symbols.len()
    }

    fn encode<W>(self, final_writer: &mut W) -> Result<(), Err> where W: ByteWriter {
        let mut writer = Vec::new();
        {
            let mut writer: BitWriter<'_, Vec<u8>, LsbFirst> = BitWriter::spown_from(&mut writer);
            for s in self.symbols.into_iter().rev() {
                writer.write_bits(CrLight::encode_symbol(s)?);
            }
        }

        // encode the size
        leb128_write(writer.len() as u64, final_writer);
        println!("Traversal Buffer Size: {} bytes", writer.len());
        for byte in writer.iter() {
            print!("{} ", byte);
        }
        println!();
        // write the encoded symbols.
        for byte in writer {
            final_writer.write_u8(byte);
        }

        
        // encode the start face configurations.
        println!("start face configurations: {:?}", self.interior_cfg);
        let freq_count_0 = self.interior_cfg.iter().filter(|&&cfg| !cfg).count();
        // the probability of zero in [0,1] is scaled to [0,256], and clamped to [1,255] as the rans does not accept the zero probability.
        let zero_prob = (((freq_count_0 as f32 / self.interior_cfg.len() as f32) * 256.0 + 0.5) as u16).clamp(1,255) as u8;
        println!("Zero Probability: {}", zero_prob);
        final_writer.write_u8(zero_prob);
        {
            let mut writer: RabsCoder<> = RabsCoder::new(freq_count_0, None);
            for &cfg in self.interior_cfg.iter().rev() {
                writer.write(if cfg { 1 } else { 0 })?;
            }
            let buffer = writer.flush()?;
            leb128_write(buffer.len() as u64, final_writer);
            println!("Rabs byte length: {}", buffer.len());
            for byte in buffer {
                final_writer.write_u8(byte);
            }
        }
        Ok(())
    }
}

pub(crate) struct ValenceTraversal {
    vertex_valences: Vec<usize>,
    corner_to_vertex_map: Vec<VertexIdx>,
    context_symbols: Vec<Vec<Symbol>>,
    last_corner: CornerIdx,
    prev_symbol: Option<Symbol>,
    interior_cfg: Vec<bool>,
    num_symbols: usize,
}


impl Traversal for ValenceTraversal {
    fn new(corner_table: & CornerTable<'_>) -> Self {
        let mut vertex_valences = Vec::with_capacity(corner_table.num_vertices());
        for i in 0..corner_table.num_vertices() {
            vertex_valences.push( corner_table.vertex_valence(i) );
        }

        let mut corner_to_vertex_map = Vec::with_capacity(corner_table.num_corners());
        for  i in 0..corner_table.num_corners() {
            corner_to_vertex_map[i] = corner_table.vertex_idx(i);
        }

        let num_unique_valences = MAX_VALENCE - MIN_VALENCE + 1;

        let context_symbols = vec![Vec::new(); num_unique_valences];
        Self { 
            vertex_valences,
            corner_to_vertex_map,
            context_symbols,
            last_corner: usize::MAX, // This will be set to a valid corner index in `new_corner_reached` before the first call to record symbol.
            prev_symbol: None,
            interior_cfg: Vec::new(),
            num_symbols: 0,
        }
    }

    fn record_symbol(&mut self, symbol: Symbol, visited_faces: &[bool], corner_table: & CornerTable<'_>) {
        self.num_symbols += 1;
        
        let next = corner_table.next(self.last_corner);
        let prev = corner_table.previous(self.last_corner);


        let active_valence = self.vertex_valences[self.corner_to_vertex_map[next]];
        match symbol {
            Symbol::C => {
                
            },
            Symbol::S => {
                // Update valences.
                self.vertex_valences[self.corner_to_vertex_map[next]] -= 1;
                self.vertex_valences[self.corner_to_vertex_map[prev]] -= 1;

                // Count the number of faces on the left side of the split vertex and
                // update the valence on the "left vertex".
                let mut num_left_faces = 0;
                let mut maybe_act_c = corner_table.opposite(prev);
                while let Some(act_c) = maybe_act_c {
                    if visited_faces[corner_table.face_idx_containing(act_c)] {
                        break;
                    }
                    num_left_faces += 1;
                    maybe_act_c = corner_table.opposite(corner_table.next(act_c));
                }
                self.vertex_valences[self.corner_to_vertex_map[self.last_corner]] = num_left_faces + 1;

                // Create a new vertex for the right side and count the number of
                // faces that should be attached to this vertex.
                let new_vert_id = self.vertex_valences.len();
                let mut num_right_faces = 0;

                maybe_act_c = corner_table.opposite(next);
                while let Some(act_c) = maybe_act_c {
                    if visited_faces[corner_table.face_idx_containing(act_c)] {
                        break;  // Stop when we reach the first visited face.
                    }
                    num_right_faces += 1;
                    // Map corners on the right side to the newly created vertex.
                    self.corner_to_vertex_map[corner_table.next(act_c)] = new_vert_id;
                    maybe_act_c = corner_table.opposite(corner_table.previous(act_c));
                }
                self.vertex_valences.push(num_right_faces + 1);
            },
            Symbol::R => {
                // Update valences.
                self.vertex_valences[self.corner_to_vertex_map[self.last_corner]] -= 1;
                self.vertex_valences[self.corner_to_vertex_map[next]] -= 1;
                self.vertex_valences[self.corner_to_vertex_map[prev]] -= 2;
            },
            Symbol::L =>{
                self.vertex_valences[self.corner_to_vertex_map[self.last_corner]] -= 1;
                self.vertex_valences[self.corner_to_vertex_map[next]] -= 2;
                self.vertex_valences[self.corner_to_vertex_map[prev]] -= 1;
            },
            Symbol::E => {
                self.vertex_valences[self.corner_to_vertex_map[self.last_corner]] -= 2;
                self.vertex_valences[self.corner_to_vertex_map[next]] -= 2;
                self.vertex_valences[self.corner_to_vertex_map[prev]] -= 2;
            }
        }

        if self.prev_symbol.is_some() {
            let clamped_valence = active_valence.clamp(MIN_VALENCE, MAX_VALENCE);

            let context = clamped_valence - MIN_VALENCE;
            self.context_symbols[context].push(self.prev_symbol.unwrap());
        }

        self.prev_symbol = Some(symbol);
    }

    fn record_start_face_config(&mut self, interior_cfg: bool) {
        self.interior_cfg.push(interior_cfg);
    }

    fn new_corner_reached(&mut self, c: CornerIdx) {
        self.last_corner = c;
    }

    fn num_symbols(&self) -> usize {
        self.num_symbols
    }

    fn encode<W>(self, writer: &mut W) -> Result<(), Err> where W: ByteWriter {
        // self.encode_start_faces();
        // self.encode_attribute_seams();

        // Store the contexts.
        for context in self.context_symbols {
            leb128_write(context.len() as u64, writer);
            let context = context.iter().map(|&s| s.get_id() as u64).collect::<Vec<_>>();

            encode_symbols(
                context, 
                1, 
                SymbolEncodingMethod::DirectCoded,
                writer
            )?;
        }

        Ok(())
    }
}


// // #[cfg(not(feature = "evaluation"))]
// #[cfg(test)]
// mod tests {
//     use std::vec;

//     use crate::core::attribute::AttributeId;
//     use crate::core::shared::Vector; 
//     use crate::core::shared::NdVector;
//     use crate::debug_expect;
//     use crate::prelude::{BitReader, ByteReader};
//     use crate::shared::connectivity::eq;
//     use crate::utils::bit_coder::leb128_read;

//     use super::*;

//     // #[test]
//     #[allow(unused)]
//     fn test_decompose_into_manifolds_simple() {
//         let mut faces = vec![
//             [0, 1, 6], // 0
//             [1, 6, 7], // 1
//             [2, 3, 6], // 2
//             [3, 6, 7], // 3
//             [4, 5, 6], // 4
//             [5, 6, 7], // 5
//         ];
//         let mut edgebreaker = Edgebreaker::new(Config::default());

//         let points = vec![NdVector::<3,f32>::zero(); 8];
//         let mut point_att = Attribute::from(
//             AttributeId::new(0), 
//             points, 
//             AttributeType::Position, 
//             Vec::new()
//         );

//         assert!(edgebreaker.init(&mut [&mut point_att], &mut faces).is_ok());

//         let coboundary_map = edgebreaker.coboundary_map_one;

//         let idx_of = |edge: &[usize; 2]| edgebreaker.edges.binary_search(edge).unwrap();
//         assert_eq!(coboundary_map[idx_of(&[0,1])], vec![0]);
//         assert_eq!(coboundary_map[idx_of(&[0,6])], vec![0]);
//         assert_eq!(coboundary_map[idx_of(&[1,6])], vec![0, 1]);
//         assert_eq!(coboundary_map[idx_of(&[1,7])], vec![1]);
//         assert_eq!(coboundary_map[idx_of(&[6,7])], vec![1,3,5]);
//         assert_eq!(coboundary_map[idx_of(&[2,3])], vec![2]);
//         assert_eq!(coboundary_map[idx_of(&[2,6])], vec![2]);
//         assert_eq!(coboundary_map[idx_of(&[3,6])], vec![2,3]);
//         assert_eq!(coboundary_map[idx_of(&[3,7])], vec![3]);
//         assert_eq!(coboundary_map[idx_of(&[4,5])], vec![4]);
//         assert_eq!(coboundary_map[idx_of(&[4,6])], vec![4]);
//         assert_eq!(coboundary_map[idx_of(&[5,6])], vec![4,5]);
//         assert_eq!(coboundary_map[idx_of(&[5,7])], vec![5]);

//     }

//     // #[test]
//     #[allow(unused)]
//     fn test_compute_edges() {
//         let faces = vec![
//             [0, 1, 6], // 0
//             [1, 6, 7], // 1
//             [2, 3, 6], // 2
//             [3, 6, 7], // 3
//             [4, 5, 6], // 4
//             [5, 6, 7], // 5
//         ];
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 8];

//         edgebreaker.compute_edges(&faces);

//         assert_eq!( edgebreaker.edges,
//             vec![
//                 [0, 1],
//                 [0, 6],
//                 [1, 6],
//                 [1, 7],
//                 [2, 3],
//                 [2, 6],
//                 [3, 6],
//                 [3, 7],
//                 [4, 5],
//                 [4, 6],
//                 [5, 6],
//                 [5, 7],
//                 [6, 7],
//             ]
//         );

//         assert_eq!( edgebreaker.coboundary_map_one,
//             vec![
//                 vec![0],
//                 vec![0],
//                 vec![0,1],
//                 vec![1],
//                 vec![2],
//                 vec![2],
//                 vec![2,3],
//                 vec![3],
//                 vec![4],
//                 vec![4],
//                 vec![4,5],
//                 vec![5],
//                 vec![1,3,5],
//             ]
//         )
//     }

//     #[test]
//     fn test_check_orientability() {
//         // test1: orientable mesh
//         let faces = vec![
//             [0,1,4],
//             [0,3,4],
//             [1,2,5],
//             [1,4,5],
//             [2,5,6],
//             [3,4,7],
//             [3,7,10],
//             [4,5,7],
//             [5,6,8],
//             [5,7,8],
//             [7,8,9],
//             [7,9,10],
//             [8,9,11],
//             [9,10,11]
//         ];
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 12];
//         edgebreaker.face_orientation = vec!(false; faces.len());
//         edgebreaker.visited_faces = vec!(false; faces.len());
//         edgebreaker.compute_edges(&faces);
//         assert!(edgebreaker.check_orientability(&faces).is_ok());
//         assert_eq!(edgebreaker.face_orientation, vec![true, false, true, false, false, true, true, true, true, false, true, true, false, false]);


//         // test 2: non-orientable mesh
//         let faces = vec![
//             [0, 1, 3],
//             [0, 1, 4],
//             [0, 2, 3],
//             [0, 4, 5],
//             [2, 3, 5],
//             [2, 4, 5],
//         ];
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 6];

//         edgebreaker.face_orientation = vec!(false; faces.len());
//         edgebreaker.visited_faces = vec!(false; faces.len());
//         edgebreaker.compute_edges(&faces);
//         assert!(edgebreaker.check_orientability(&faces).is_err());

//         let faces = [
//             [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
//             [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
//             [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
//             [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
//         ];
//         let orientation = vec![
//             false, false, true, true, true, false, true, true,
//             false, false, true, false, true, true, false, true,
//             false, false, true, true, true, true, false, true,
//             true, true, false, false, false, false, false, false
//         ];
//         // sort faces while taping orientation
//         let (faces, orientation) = {
//             let mut zipped = faces.iter().zip(orientation.iter()).collect::<Vec<_>>();
//             zipped.sort_by_key(|f| f.0);
//             let faces = zipped.iter().map(|&(&f, _)| f).collect::<Vec<_>>();
//             let orientation = zipped.iter().map(|&(_, &o)| o).collect::<Vec<_>>();
//             (faces, orientation)
//         };
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 12];
//         edgebreaker.face_orientation = vec!(false; faces.len());
//         edgebreaker.visited_faces = vec!(false; faces.len());
//         edgebreaker.compute_edges(&faces);
//         assert!(edgebreaker.check_orientability(&faces).is_ok());
//         assert_eq!(edgebreaker.face_orientation, orientation,
//             "orientation is wrong at: {:?}",
//             edgebreaker.face_orientation.iter()
//                 .zip(orientation.iter())
//                 .enumerate()
//                 .filter(|(_, (a,b))| a!=b)
//                 .map(|(i,_)| faces[i])
//                 .collect::<Vec<_>>()  
//         );
//     }


//     use Symbol::*;
//     fn read_symbols<R>(reader: &mut R, size: usize) -> Vec<Symbol> 
//         where R: ByteReader
//     {
//         let mut out = Vec::new();
//         let mut reader = BitReader::spown_from(reader).unwrap();
//         for _ in 0..size {
//             out.push(
//                 CrLight::decode_symbol(&mut reader)
//             );
//         }
//         out
//     }

//     fn read_topology_splits<R: ByteReader>(reader: &mut R) -> Vec<TopologySplit> {
//         let mut topology_splits = Vec::new();
//         let num_topology_splits = leb128_read(reader).unwrap() as u32;
//         let mut last_idx = 0;
//         for _ in 0..num_topology_splits {
//             let source_symbol_idx = leb128_read(reader).unwrap() as usize + last_idx;
//             let split_symbol_idx = source_symbol_idx - leb128_read(reader).unwrap() as usize;
//             let topology_split = TopologySplit {
//                 source_symbol_idx,
//                 split_symbol_idx,
//                 source_edge_orientation: Orientation::Right, // this value is temporary
//             };
//             topology_splits.push(topology_split);
//             last_idx = source_symbol_idx;
//         }

//         let mut reader: BitReader<_> = BitReader::spown_from(reader).unwrap();
//         for split_mut in topology_splits.iter_mut() {
//             // update the orientation of the topology split.
//             split_mut.source_edge_orientation = match reader.read_bits(1).unwrap() {
//                 0 => Orientation::Left,
//                 1 => Orientation::Right, 
//                 _ => unreachable!(),
//             };
//         }

//         topology_splits
//     }


//     fn manual_test<const TEST_ORIENTABILITY: bool>(
//         mut original_faces: Vec<[VertexIdx; 3]>, 
//         points: Vec<NdVector<3,f32>>, 
//         expected_symbols: Vec<Symbol>, 
//         expected_topology_splits: Vec<TopologySplit>, 
//         expected_faces: Option<Vec<[VertexIdx; 3]>>
//     ) {
//         // positions do not matter
//         let mut point_att = Attribute::from(
//             AttributeId::new(0), 
//             points, 
//             AttributeType::Position, 
//             Vec::new()
//         );

//         let mut buff_writer = Vec::new();
//         Edgebreaker::new(Config::default()).encode_connectivity(&mut original_faces, &mut [&mut point_att], &mut buff_writer).unwrap();

//         let mut reader = buff_writer.into_iter();

//         assert_eq!(reader.read_u8().unwrap(), 0);
//         assert_eq!(reader.read_u64().unwrap(), original_faces.len() as u64);
//         assert_eq!(expected_topology_splits, read_topology_splits(&mut reader));
//         debug_expect!("Start of Symbols", reader);
//         assert_eq!(expected_symbols, read_symbols(&mut reader, original_faces.len()));

//         if !TEST_ORIENTABILITY {
//             original_faces.iter_mut().for_each(|f| f.sort());
//         }
//         if let Some(expected_faces) = expected_faces  {
//             assert_eq!(original_faces, expected_faces);
//         }
//     }

//     #[test]
//     fn edgebreaker_disc() {
//         let faces = vec![
//             [0,1,4],
//             [0,3,4],
//             [1,2,5],
//             [1,4,5],
//             [2,5,6],
//             [3,4,7],
//             [3,7,10],
//             [4,5,7],
//             [5,6,8],
//             [5,7,8],
//             [7,8,9],
//             [7,9,10],
//             [8,9,11],
//             [9,10,11]
//         ];
//         // positions do not matter
//         let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

//         let expected_symbols = vec![E,E,S,R,L,R,R,C,C,R,R,R,C,C];

//         let expected_faces = vec![
//             [0,1,2],
//             [1,3,4],
//             [0,3,1],
//             [0,5,3],
//             [0,6,5],
//             [5,6,7],
//             [6,8,7],
//             [0,8,6],
//             [0,2,8],
//             [2,9,8],
//             [2,10,9],
//             [2,11,10],
//             [1,11,2],
//             [1,4,11] // orientation base
//         ];

//         manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
//     }

//     #[test]
//     fn edgebreaker_split() {
//         let faces = vec![
//             [0,1,2],
//             [0,2,4],
//             [0,4,5],
//             [2,3,4]
//         ];
//         // positions do not matter
//         let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

//         let expected_symbols = vec![E,E,S,R];

//         let expected_faces = vec![
//             [0,2,1], 
//             [1,4,3], 
//             [0,1,3], 
//             [0,3,5] // orientation base
//         ];

//         manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
//     }

//     #[test]
//     fn edgebreaker_triangle() {
//         let faces = vec![
//             [0,1,3],
//             [1,2,3],
//             [2,3,4],
//             [3,4,5]
//         ];

//         let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];
//         let expected_symbols = vec![E,R,R,L];
//         let expected_faces = vec![
//             [0,2,1], 
//             [0,1,3], 
//             [0,3,4], 
//             [0,4,5] // base
//         ];
//         manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
//     }

//     #[test]
//     fn edgebreaker_begin_from_center() {
//         // mesh forming a square whose initial edge is not on the boundary.
//         let mut original_faces = vec![
//             [9,23,24], [8,9,23], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
//             [8,22,23], [7,8,22], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
//             [7,21,22], [6,7,21], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
//             [6,20,21], [6,19,20], [5,6,19], [5,18,19], [4,5,18], [4,17,18], [4,15,17], [15,16,17]
//         ];
//         original_faces.sort();
//         // positions do not matter
//         let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

//         let expected_symbols = vec![E, E, E, S, R, L, R, L, R, R, L, R, S, R, E, S, R, C, R, E, L, S, R, C, C, C, R, C, C, L, S /* hole */, C];
//         let expected_topology_splits = vec![
//             TopologySplit {
//                 source_symbol_idx: 16,
//                 split_symbol_idx: 16,
//                 source_edge_orientation: Orientation::Left,
//             },
//         ];
//         manual_test::<false>(original_faces, points, expected_symbols, expected_topology_splits, None);
//     }

//     #[test]
//     fn edgebreaker_handle() {
//         // create torus in order to test the handle symbol.
//         let mut original_faces = vec![
//             [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
//             [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
//             [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
//             [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
//         ];
//         original_faces.sort();
//         // positions do not matter
//         let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

//         let expected_symbols = vec![E, E, S, R, E, E, S, L, R, S, R, C, S /* handle */, R, C, S /* handle */, R, C, C, R, C, C, R, C, C, C, R, C, C, C, C, C];
//         let expected_topology_splits = vec![
//             TopologySplit {
//                 source_symbol_idx: 31,
//                 split_symbol_idx: 17,
//                 source_edge_orientation: Orientation::Left,
//             },
//             TopologySplit {
//                 source_symbol_idx: 28,
//                 split_symbol_idx: 20,
//                 source_edge_orientation: Orientation::Right,
//             }
//         ];

//         manual_test::<false>(original_faces, points, expected_symbols, expected_topology_splits, None);
//     }


//     // #[test] 
//     #[allow(unused)] // uncomment the test to run it. it is commented out as it takes a long time to run.
//     fn connectivity_check_after_vertex_permutation() {
//         let (bunny,_) = tobj::load_obj(
//             format!("tests/data/punctured_sphere.obj"), 
//             &tobj::GPU_LOAD_OPTIONS
//         ).unwrap();
//         let bunny = &bunny[0];
//         let mesh = &bunny.mesh;

//         let faces_original = mesh.indices.chunks(3)
//             .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
//             .collect::<Vec<_>>();

//         let mut faces = faces_original.clone();

//         let points = mesh.positions.chunks(3)
//             .map(|x| NdVector::<3,f32>::from([x[0], x[1], x[2]]))
//             .collect::<Vec<_>>();

//         let mut point_att = Attribute::from(AttributeId::new(0), points, AttributeType::Position, Vec::new());
//         let mut edgebreaker = Edgebreaker::new(Config::default());
//         assert!(edgebreaker.init(&mut [&mut point_att], &mut faces).is_ok());
//         let mut writer = Vec::new();
//         assert!(edgebreaker.encode_connectivity(&mut faces, &mut [&mut point_att], &mut writer).is_ok());


//         assert!(eq::weak_eq_by_laplacian(&faces, &faces_original).unwrap());
//     }
// }

