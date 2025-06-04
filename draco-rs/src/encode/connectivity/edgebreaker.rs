use std::{
    fmt,
    cmp,
};

use crate::core::bit_coder::{BitWriter, ByteWriter};
use crate::debug_write;
use crate::prelude::{Attribute, AttributeType};
use crate::shared::connectivity::edgebreaker::symbol_encoder::{
	SymbolEncodingConfig,
	Symbol
};

use crate::core::shared::{ConfigType, EdgeIdx, FaceIdx, VertexIdx};

use crate::shared::connectivity::edgebreaker::symbol_encoder::{
	CrLight,
	SymbolEncoder,
};
use crate::shared::connectivity::edgebreaker::{edge_shared_by, orientation_of_next_face, sign_of, Orientation, TopologySplit};
use crate::shared::connectivity::EdgebreakerDecoder;
use crate::utils::bit_coder::leb128_write;
use std::collections::{BTreeMap, VecDeque};
use std::vec;

use crate::encode::connectivity::ConnectivityEncoder;

#[cfg(feature = "evaluation")]
use crate::eval;

pub(crate) struct Edgebreaker {
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

    /// The number of connected components in the mesh.
    num_connected_components: usize,
	
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

    /// records the signs of the permutations when faces are sorted.
    signs_of_faces: Vec<bool>,

    /// The previous face index that was deoded.
    prev_face_idx: FaceIdx,

    /// Records the topology splits detected during the edgebreaker encoding.
    topology_splits: Vec<TopologySplit>,

    /// The map from the face index to the split symbol index.
    map_face_idx_to_split_symbol_idx: BTreeMap<FaceIdx, usize>,
	
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
    symbol_encoding: SymbolEncodingConfig,
    decoder: EdgebreakerDecoder,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self{
			symbol_encoding: SymbolEncodingConfig::default(), 
            decoder: EdgebreakerDecoder::SpiraleReversi
		}
    }
}


#[derive(Debug, cmp::PartialEq)]
#[remain::sorted]
#[derive(thiserror::Error)]
pub enum Err {
    #[error("Too many handles.")]
    HandleSizeTooLarge,
    #[error("Too many holes.")]
    HoleSizeTooLarge,
    #[error("The input mesh is non-orientable.")]
    NonOrientable,
    #[error("The input mesh has too many connected components: {0}")]
    TooManyConnectedComponents(usize),
}

impl Edgebreaker {
	// Build the object with empty arrays.
	pub fn new(config: Config)->Self {
        Self {
            edges: Vec::new(),
            coboundary_map_one: Vec::new(),
            face_connectivity: Vec::new(),
            coboundary_map_zero: None,
            lies_on_boundary_or_cutting_path: Vec::new(),
            visited_vertices: Vec::new(),
            visited_faces: Vec::new(),
            num_connected_components: 0,
            face_orientation: Vec::new(),
            active_edge_face_idx_stack: Vec::new(),
            cutting_paths: Vec::new(),
            symbols: Vec::new(),
            symbol_idx_to_face_idx: Vec::new(),
            vertex_decode_order: Vec::new(),
            face_decode_order: Vec::new(),
            num_decoded_vertices: 0,
            prev_face_idx: usize::MAX,
            signs_of_faces: Vec::new(),
            topology_splits: Vec::new(),
            map_face_idx_to_split_symbol_idx: BTreeMap::new(),
            config,
        }
    }
	
	/// Initializes the Edgebreaker. This function takes in a mesh and 
	/// decomposes it into manifolds with boundaries if it is not homeomorhic to a
	/// manifold. 
	pub(crate) fn init(&mut self , children: &mut[&mut Attribute], faces: &mut [[VertexIdx; 3]]) -> Result<(), Err> {
        let pos_att_len = children.iter()
            .find(|att| att.get_attribute_type() == AttributeType::Position)
            .unwrap()
            .len();
        self.visited_vertices = vec!(false; pos_att_len);
        self.visited_faces = vec!(false; faces.len());
        self.face_orientation = vec!(false; faces.len());

        self.num_connected_components = 0;

        self.edges.clear();
        self.coboundary_map_one.clear();
        self.face_connectivity.clear();
        self.coboundary_map_zero = None;
        self.lies_on_boundary_or_cutting_path = vec![false; pos_att_len];

        self.swap_vertices_and_reserve_orientation(faces, children);
        self.sort_faces(faces);

        self.compute_edges(faces);

        self.check_orientability(faces)?;
        self.vertex_decode_order = vec![usize::MAX; pos_att_len];
        self.face_decode_order = vec![usize::MAX; faces.len()];
        self.num_decoded_vertices = 0;
        Ok(())
	}


    fn sort_faces(&mut self, faces: &mut [[VertexIdx; 3]]) {
        faces.sort_by_key(|f| *f.iter().min().unwrap() );
        self.signs_of_faces.clear();
        self.signs_of_faces.reserve(faces.len());
        for f in faces.iter_mut() {
            self.signs_of_faces.push( sign_of(*f) );
            f.sort();
        }
        faces.sort();
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

            self.num_connected_components += 1;

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
	
	
	/// A function implementing a step of the Edgebreaker algorithm.
	/// When this function returns, all the CLERS symbols are written to the
	/// buffer. Since the time complexity of 'find_vertices_pinching()' 
	/// is O(1), the complexity of this function (single recursive step) is also O(1).
	fn edgebreaker_recc<const REVERSE_DECODE: bool>(&mut self, faces: &[[VertexIdx; 3]]) -> Result<(), Err> {
        let (curr_edge, curr_face_idx) = self.active_edge_face_idx_stack.pop().unwrap();

        let adjacent_faces = self.face_connectivity[curr_face_idx];
        // compute the left and right faces of the current face.
        let mut maybe_left_face = None;
        let mut maybe_right_face = None;
        if self.prev_face_idx!= usize::MAX {
            println!("prev_face: {:?}, curr_face: {:?}, curr_edge: {:?}", faces[self.prev_face_idx], faces[curr_face_idx], curr_edge);
        }
        for &adj_face_idx in adjacent_faces.iter() {
            if adj_face_idx == curr_face_idx || adj_face_idx == self.prev_face_idx {
                continue;
            }
            
            if faces[adj_face_idx].contains(&curr_edge.left_vertex) {
                maybe_left_face = Some(adj_face_idx);
            } else {
                debug_assert!(
                    faces[adj_face_idx].contains(&curr_edge.right_vertex),
                    "The adjacent face not containing the left or right vertex of the current edge."
                );
                maybe_right_face = Some(adj_face_idx);
            }
        }

        let curr_vertex = *faces[curr_face_idx]
            .iter()
            .find(|&&v| v != curr_edge.left_vertex && v != curr_edge.right_vertex)
            .unwrap();

        let left_edge = OrientedEdge { 
            left_vertex: curr_edge.left_vertex, 
            right_vertex: curr_vertex 
        };
        let right_edge = OrientedEdge { 
            left_vertex: curr_vertex, 
            right_vertex: curr_edge.right_vertex
        };

        // // if 'f_idx' is already visited, then this must be an edge of previous 'H'.
        // // update its metadata and return.
        // let mut maybe_handle_idx = None;
        // for (i, &(symbol_idx, edge)) in self.handle_edges.iter().enumerate() {
        //     // ToDo: This condition can be optimized.
        //     if faces[f_idx].contains(&edge[0]) && faces[f_idx].contains(&edge[1]) {
        //         let is_right_not_left = if edge.contains(&right_vertex) { 1 } else { 0 };
        //         let metadata = self.symbols.len() - symbol_idx << 1 | is_right_not_left;
        //         self.symbols[symbol_idx] = Symbol::H(metadata);
        //         maybe_handle_idx = Some(i);
        //         break;
        //     }
        // }
        // if let Some(handle_idx) = maybe_handle_idx {
        //     self.handle_edges.remove(handle_idx);
        // }
        
        // if we are reverse-decoding, we need to store the face corresponding to the symbol in order
        // to compute the order of the vertices when decoding.
        if REVERSE_DECODE {
            self.symbol_idx_to_face_idx.push(curr_face_idx);
        }

        println!("Encoding face: {:?}", faces[curr_face_idx]);

        let symbol = if self.visited_vertices[curr_vertex] || self.lies_on_boundary_or_cutting_path[curr_vertex] {
            let is_right_proceedable = if let Some(right_face_idx) = maybe_right_face {
                if self.visited_faces[right_face_idx] {
                    // if the right face exists and is visited, then there is a possibility that the right face
                    // is a handle face.
                    let symbol_idx = self.map_face_idx_to_split_symbol_idx.remove(&right_face_idx);
                    if let Some(symbol_idx) = symbol_idx {
                        let split = TopologySplit {
                            source_symbol_idx: self.symbols.len(),
                            split_symbol_idx: symbol_idx,
                            source_edge_orientation: Orientation::Right,
                        };
                        self.topology_splits.push(split);
                    }
                    false
                } else {
                    true
                }
            } else {
                // if there is no right face, then is not proceedable.
                false
            };

            let is_left_proceedable = if let Some(left_face_idx) = maybe_left_face {
                if self.visited_faces[left_face_idx] {
                    // if the left face exists and is visited, then the left face
                    // is a handle face.
                    let symbol_idx = self.map_face_idx_to_split_symbol_idx[&left_face_idx];
                    if self.symbols[symbol_idx] == Symbol::S {
                        let split = TopologySplit {
                            source_symbol_idx: self.symbols.len(),
                            split_symbol_idx: symbol_idx,
                            source_edge_orientation: Orientation::Left,
                        };
                        self.topology_splits.push(split);
                    }
                    false
                } else {
                    true
                }
            } else {
                // if there is no left face, then it is not proceedable.
                false
            };

            // edgebreaker encoding
            if is_right_proceedable {
                if is_left_proceedable {
                    // case S: split
                    self.active_edge_face_idx_stack.push((left_edge, maybe_right_face.unwrap())); // This can be unwrap unchecked.
                    self.active_edge_face_idx_stack.push((right_edge, maybe_left_face.unwrap())); // This can be unwrap unchecked.
                    self.map_face_idx_to_split_symbol_idx.insert(curr_face_idx, self.symbols.len());
                    Symbol::S
                } else {
                    self.active_edge_face_idx_stack.push((right_edge, maybe_right_face.unwrap())); // This can be unwrap unchecked.
                    Symbol::L
                }
            } else {
                if is_left_proceedable {
                    self.active_edge_face_idx_stack.push((left_edge, maybe_left_face.unwrap())); // This can be unwrap unchecked.
                    Symbol::R
                } else {
                    Symbol::E
                }
            }
        } else {
            self.active_edge_face_idx_stack.push((right_edge, maybe_right_face.unwrap_or(curr_face_idx))); // This can be unwrap unchecked.
            Symbol::C
        };
        self.symbols.push(symbol);

        self.visited_vertices[curr_vertex] = true;
        self.visited_faces[curr_face_idx] = true;
        self.prev_face_idx = if symbol == Symbol::E {
            if let Some(most_recent_s_idx) = self.symbols.iter().rposition(|&s| s == Symbol::S) {
                self.symbol_idx_to_face_idx[most_recent_s_idx]
            } else {
                // if there is no 'S' symbol, then we are done encoding a connected component.
                // Thus the previous face index must be invalid.
                usize::MAX
            }
        } else {
            curr_face_idx
        };

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

    fn get_some_unvisited_triangle(&mut self, faces: &[[VertexIdx; 3]]) -> Option<FaceIdx> {
        {
            // safety check.
            debug_assert!(faces.len() == self.visited_faces.len(), "faces.len(): {}, self.visited_faces.len(): {}", faces.len(), self.visited_faces.len());
        }

        for face_idx in 0..faces.len() {
            // Safety: face_idx is always in bounds since 'faces.len()==self.visited_faces.len()'.
            if !unsafe{ *self.visited_faces.get_unchecked(face_idx) } {
                return Some(face_idx);
            }
        }
        None
    }


    fn encode_topology_splits<W>(&mut self, writer: &mut W) -> Result<(), Err> 
        where W: ByteWriter,
    {
        #[cfg(feature = "evaluation")]
        {
            let mut string = String::new();
            for split in self.topology_splits.iter() {
                string.push_str(&format!("{}:{}({:?}) ", split.source_symbol_idx, split.split_symbol_idx, split.source_edge_orientation));
            }
            eval::write_json_pair("topology_splits", serde_json::Value::from(string), writer);
        }
        let mut last_idx = 0;
        // write the number of topology splits.
        leb128_write(self.topology_splits.len() as u64, writer);
        assert!(self.topology_splits.is_empty() );
        for split in self.topology_splits.iter() {
           leb128_write((split.source_symbol_idx - last_idx) as u64, writer);
           leb128_write((split.source_symbol_idx - split.split_symbol_idx) as u64, writer);
           last_idx = split.source_symbol_idx;
        }
        for split in self.topology_splits.iter() {
            let orientation = match split.source_edge_orientation {
                Orientation::Left => 0,
                Orientation::Right => 1,
            };
            writer.write_u8(orientation);
        }
        Ok(())
    }
        


    fn encode_symbols<W>(&mut self, writer: &mut W) -> Result<(), Err> 
        where W: ByteWriter,
    {
        let encoder = self.config.symbol_encoding;
        #[cfg(feature = "evaluation")]
        {
            let mut string = String::new();
            for &symbol in self.symbols.iter().rev() {
                let (symbol, metadata) = symbol.as_char();
                if let Some(metadata) = metadata {
                    string.push_str(&format!("{symbol}({metadata})"));
                } else {
                    string.push_str(&format!("{symbol}"));
                };
            }
            eval::write_json_pair("clers_string", serde_json::Value::from(string), writer);
        }
        let mut writer: BitWriter<_> = BitWriter::spown_from(writer);
        match encoder {
            SymbolEncodingConfig::CrLight => {
                for &symbol in self.symbols.iter().rev() {
                    match CrLight::encode_symbol(symbol) {
                        Ok((size, value)) => writer.write_bits((size, value)),
                        Err(err) => return Err(err),
                    }
                }
            },
            SymbolEncodingConfig::Rans => {
                // ToDo: come back here after implementing the rANS encoder.
                unimplemented!();
            }
        }
        Ok(())
    }

    /// begins the Edgebreaker iteration from the given edge.
    fn begin_from(&mut self, edge: OrientedEdge) -> Result<(), Err> {
        // active_edge_face_idx_stack must be empty at the beginning.
        debug_assert!(self.active_edge_face_idx_stack.is_empty());

        self.active_edge_face_idx_stack.push((edge, self.symbols.len()));
        // mark the face and the vertices as visited.
        {
            self.visited_vertices[edge.left_vertex] = true;
            self.visited_vertices[edge.right_vertex] = true;
        }

        if self.lies_on_boundary_or_cutting_path[edge.left_vertex] {
            if self.coboundary_map_zero.is_none() {
                self.compute_coboundary_map_zero();
            };
            let coboundary_map_zero = unsafe{ self.coboundary_map_zero.as_ref().unwrap_unchecked() };
            for coboundary_edge in coboundary_map_zero[edge.left_vertex].clone() {
                if self.coboundary_map_one[coboundary_edge].len() == 1 {
                    self.mark_vertices_as_visited_in_boundary_containing(coboundary_edge)?;
                } else if self.coboundary_map_one[coboundary_edge].len() > 2 {
                    self.mark_vertices_as_visited_in_cutting_path_containing(coboundary_edge)?;
                }
            }
        } else if self.lies_on_boundary_or_cutting_path[edge.right_vertex] {
            if self.coboundary_map_zero.is_none() {
                self.compute_coboundary_map_zero();
            };
            let coboundary_map_zero = unsafe{ self.coboundary_map_zero.as_ref().unwrap_unchecked() };
            for coboundary_edge in coboundary_map_zero[edge.right_vertex].clone() {
                if self.coboundary_map_one[coboundary_edge].len() == 1 {
                    self.mark_vertices_as_visited_in_boundary_containing(coboundary_edge)?;
                } else if self.coboundary_map_one[coboundary_edge].len() > 2 {
                    self.mark_vertices_as_visited_in_cutting_path_containing(coboundary_edge)?;
                }
            }
        }
        // if 'self.coboundary_map_one[e_idx].len() == 2', then we do not need to do anything, since it is neither a boundary nor a cutting path.

        Ok(())
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

impl ConnectivityEncoder for Edgebreaker {
    type Config = Config;
	type Err = Err;
	/// The main encoding paradigm for Edgebreaker.
    fn encode_connectivity<W>(
        &mut self, 
        faces: &mut [[VertexIdx; 3]], 
        children: &mut[&mut Attribute], 
        writer: &mut W
    ) -> Result<(), Self::Err> 
        where W: ByteWriter
    {
        // encode the encoding configuration
        self.config.symbol_encoding.write_symbol_encoding(writer);
        
        self.init(children, faces)?;

        if self.num_connected_components > 255 {
            return Err(Err::TooManyConnectedComponents(self.num_connected_components));
        }

		// Run Edgebreaker once for each connected component.
		while let Some(f_idx) = self.get_some_unvisited_triangle(faces) {
            let face = faces[f_idx];

            let unvisited_edge = OrientedEdge {
                left_vertex: face[0], 
                right_vertex: face[1]
            };
            self.begin_from(unvisited_edge)?;

            // run Edgebreaker
			while !self.active_edge_face_idx_stack.is_empty() {
                self.edgebreaker_recc::<true>(faces)?;
            }
		}
        writer.write_u64(self.symbols.len() as u64);
        self.encode_topology_splits(writer)?;
        debug_write!("Start of Symbols", writer);
        self.encode_symbols(writer)?;
        self.compute_decode_order(faces);

        self.reflect_decode_order(faces, children);
        Ok(())
	}
}


// #[cfg(not(feature = "evaluation"))]
#[cfg(test)]
mod tests {
    use std::vec;

    use crate::core::attribute::AttributeId;
    use crate::core::shared::Vector; 
    use crate::core::shared::NdVector;
    use crate::debug_expect;
    use crate::prelude::{BitReader, ByteReader};
    use crate::shared::connectivity::eq;
    use crate::utils::bit_coder::leb128_read;

    use super::*;

    // #[test]
    #[allow(unused)]
    fn test_decompose_into_manifolds_simple() {
        let mut faces = vec![
            [0, 1, 6], // 0
            [1, 6, 7], // 1
            [2, 3, 6], // 2
            [3, 6, 7], // 3
            [4, 5, 6], // 4
            [5, 6, 7], // 5
        ];
        let mut edgebreaker = Edgebreaker::new(Config::default());

        let points = vec![NdVector::<3,f32>::zero(); 8];
        let mut point_att = Attribute::from(
            AttributeId::new(0), 
            points, 
            AttributeType::Position, 
            Vec::new()
        );

        assert!(edgebreaker.init(&mut [&mut point_att], &mut faces).is_ok());

        let coboundary_map = edgebreaker.coboundary_map_one;

        let idx_of = |edge: &[usize; 2]| edgebreaker.edges.binary_search(edge).unwrap();
        assert_eq!(coboundary_map[idx_of(&[0,1])], vec![0]);
        assert_eq!(coboundary_map[idx_of(&[0,6])], vec![0]);
        assert_eq!(coboundary_map[idx_of(&[1,6])], vec![0, 1]);
        assert_eq!(coboundary_map[idx_of(&[1,7])], vec![1]);
        assert_eq!(coboundary_map[idx_of(&[6,7])], vec![1,3,5]);
        assert_eq!(coboundary_map[idx_of(&[2,3])], vec![2]);
        assert_eq!(coboundary_map[idx_of(&[2,6])], vec![2]);
        assert_eq!(coboundary_map[idx_of(&[3,6])], vec![2,3]);
        assert_eq!(coboundary_map[idx_of(&[3,7])], vec![3]);
        assert_eq!(coboundary_map[idx_of(&[4,5])], vec![4]);
        assert_eq!(coboundary_map[idx_of(&[4,6])], vec![4]);
        assert_eq!(coboundary_map[idx_of(&[5,6])], vec![4,5]);
        assert_eq!(coboundary_map[idx_of(&[5,7])], vec![5]);

    }

    // #[test]
    #[allow(unused)]
    fn test_compute_edges() {
        let faces = vec![
            [0, 1, 6], // 0
            [1, 6, 7], // 1
            [2, 3, 6], // 2
            [3, 6, 7], // 3
            [4, 5, 6], // 4
            [5, 6, 7], // 5
        ];
        let mut edgebreaker = Edgebreaker::new(Config::default());
        edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 8];

        edgebreaker.compute_edges(&faces);

        assert_eq!( edgebreaker.edges,
            vec![
                [0, 1],
                [0, 6],
                [1, 6],
                [1, 7],
                [2, 3],
                [2, 6],
                [3, 6],
                [3, 7],
                [4, 5],
                [4, 6],
                [5, 6],
                [5, 7],
                [6, 7],
            ]
        );

        assert_eq!( edgebreaker.coboundary_map_one,
            vec![
                vec![0],
                vec![0],
                vec![0,1],
                vec![1],
                vec![2],
                vec![2],
                vec![2,3],
                vec![3],
                vec![4],
                vec![4],
                vec![4,5],
                vec![5],
                vec![1,3,5],
            ]
        )
    }

    #[test]
    fn test_check_orientability() {
        // test1: orientable mesh
        let faces = vec![
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
        let mut edgebreaker = Edgebreaker::new(Config::default());
        edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 12];
        edgebreaker.face_orientation = vec!(false; faces.len());
        edgebreaker.visited_faces = vec!(false; faces.len());
        edgebreaker.compute_edges(&faces);
        assert!(edgebreaker.check_orientability(&faces).is_ok());
        assert_eq!(edgebreaker.face_orientation, vec![true, false, true, false, false, true, true, true, true, false, true, true, false, false]);


        // test 2: non-orientable mesh
        let faces = vec![
            [0, 1, 3],
            [0, 1, 4],
            [0, 2, 3],
            [0, 4, 5],
            [2, 3, 5],
            [2, 4, 5],
        ];
        let mut edgebreaker = Edgebreaker::new(Config::default());
        edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 6];

        edgebreaker.face_orientation = vec!(false; faces.len());
        edgebreaker.visited_faces = vec!(false; faces.len());
        edgebreaker.compute_edges(&faces);
        assert!(edgebreaker.check_orientability(&faces).is_err());

        let faces = [
            [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
        ];
        let orientation = vec![
            false, false, true, true, true, false, true, true,
            false, false, true, false, true, true, false, true,
            false, false, true, true, true, true, false, true,
            true, true, false, false, false, false, false, false
        ];
        // sort faces while taping orientation
        let (faces, orientation) = {
            let mut zipped = faces.iter().zip(orientation.iter()).collect::<Vec<_>>();
            zipped.sort_by_key(|f| f.0);
            let faces = zipped.iter().map(|&(&f, _)| f).collect::<Vec<_>>();
            let orientation = zipped.iter().map(|&(_, &o)| o).collect::<Vec<_>>();
            (faces, orientation)
        };
        let mut edgebreaker = Edgebreaker::new(Config::default());
        edgebreaker.lies_on_boundary_or_cutting_path = vec![false; 12];
        edgebreaker.face_orientation = vec!(false; faces.len());
        edgebreaker.visited_faces = vec!(false; faces.len());
        edgebreaker.compute_edges(&faces);
        assert!(edgebreaker.check_orientability(&faces).is_ok());
        assert_eq!(edgebreaker.face_orientation, orientation,
            "orientation is wrong at: {:?}",
            edgebreaker.face_orientation.iter()
                .zip(orientation.iter())
                .enumerate()
                .filter(|(_, (a,b))| a!=b)
                .map(|(i,_)| faces[i])
                .collect::<Vec<_>>()  
        );
    }


    use Symbol::*;
    fn read_symbols<R>(reader: &mut R, size: usize) -> Vec<Symbol> 
        where R: ByteReader
    {
        let mut out = Vec::new();
        let mut reader = BitReader::spown_from(reader).unwrap();
        for _ in 0..size {
            out.push(
                CrLight::decode_symbol(&mut reader)
            );
        }
        out
    }

    fn read_topology_splits<R: ByteReader>(reader: &mut R) -> Vec<TopologySplit> {
        let mut topology_splits = Vec::new();
        let num_topology_splits = leb128_read(reader).unwrap() as u32;
        let mut last_idx = 0;
        for _ in 0..num_topology_splits {
            let source_symbol_idx = leb128_read(reader).unwrap() as usize + last_idx;
            let split_symbol_idx = source_symbol_idx - leb128_read(reader).unwrap() as usize;
            let topology_split = TopologySplit {
                source_symbol_idx,
                split_symbol_idx,
                source_edge_orientation: Orientation::Right, // this value is temporary
            };
            topology_splits.push(topology_split);
            last_idx = source_symbol_idx;
        }

        let mut reader: BitReader<_> = BitReader::spown_from(reader).unwrap();
        for split_mut in topology_splits.iter_mut() {
            // update the orientation of the topology split.
            split_mut.source_edge_orientation = match reader.read_bits(1).unwrap() {
                0 => Orientation::Left,
                1 => Orientation::Right, 
                _ => unreachable!(),
            };
        }

        topology_splits
    }


    fn manual_test<const TEST_ORIENTABILITY: bool>(
        mut original_faces: Vec<[VertexIdx; 3]>, 
        points: Vec<NdVector<3,f32>>, 
        expected_symbols: Vec<Symbol>, 
        expected_topology_splits: Vec<TopologySplit>, 
        expected_faces: Option<Vec<[VertexIdx; 3]>>
    ) {
        // positions do not matter
        let mut point_att = Attribute::from(
            AttributeId::new(0), 
            points, 
            AttributeType::Position, 
            Vec::new()
        );

        let mut buff_writer = Vec::new();
        Edgebreaker::new(Config::default()).encode_connectivity(&mut original_faces, &mut [&mut point_att], &mut buff_writer).unwrap();

        let mut reader = buff_writer.into_iter();

        assert_eq!(reader.read_u8().unwrap(), 0);
        assert_eq!(reader.read_u64().unwrap(), original_faces.len() as u64);
        assert_eq!(expected_topology_splits, read_topology_splits(&mut reader));
        debug_expect!("Start of Symbols", reader);
        assert_eq!(expected_symbols, read_symbols(&mut reader, original_faces.len()));

        if !TEST_ORIENTABILITY {
            original_faces.iter_mut().for_each(|f| f.sort());
        }
        if let Some(expected_faces) = expected_faces  {
            assert_eq!(original_faces, expected_faces);
        }
    }

    #[test]
    fn edgebreaker_disc() {
        let faces = vec![
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
        let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let expected_symbols = vec![E,E,S,R,L,R,R,C,C,R,R,R,C,C];

        let expected_faces = vec![
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
            [1,4,11] // orientation base
        ];

        manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
    }

    #[test]
    fn edgebreaker_split() {
        let faces = vec![
            [0,1,2],
            [0,2,4],
            [0,4,5],
            [2,3,4]
        ];
        // positions do not matter
        let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let expected_symbols = vec![E,E,S,R];

        let expected_faces = vec![
            [0,2,1], 
            [1,4,3], 
            [0,1,3], 
            [0,3,5] // orientation base
        ];

        manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
    }

    #[test]
    fn edgebreaker_triangle() {
        let faces = vec![
            [0,1,3],
            [1,2,3],
            [2,3,4],
            [3,4,5]
        ];

        let points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];
        let expected_symbols = vec![E,R,R,L];
        let expected_faces = vec![
            [0,2,1], 
            [0,1,3], 
            [0,3,4], 
            [0,4,5] // base
        ];
        manual_test::<true>(faces, points, expected_symbols, Vec::new(), Some(expected_faces));
    }

    #[test]
    fn edgebreaker_begin_from_center() {
        // mesh forming a square whose initial edge is not on the boundary.
        let mut original_faces = vec![
            [9,23,24], [8,9,23], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,22,23], [7,8,22], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,21,22], [6,7,21], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,20,21], [6,19,20], [5,6,19], [5,18,19], [4,5,18], [4,17,18], [4,15,17], [15,16,17]
        ];
        original_faces.sort();
        // positions do not matter
        let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

        let expected_symbols = vec![E, E, E, S, R, L, R, L, R, R, L, R, S, R, E, S, R, C, R, E, L, S, R, C, C, C, R, C, C, L, S /* hole */, C];
        let expected_topology_splits = vec![
            TopologySplit {
                source_symbol_idx: 16,
                split_symbol_idx: 16,
                source_edge_orientation: Orientation::Left,
            },
        ];
        manual_test::<false>(original_faces, points, expected_symbols, expected_topology_splits, None);
    }

    #[test]
    fn edgebreaker_handle() {
        // create torus in order to test the handle symbol.
        let mut original_faces = vec![
            [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
        ];
        original_faces.sort();
        // positions do not matter
        let points = vec![NdVector::<3,f32>::zero(); original_faces.iter().flatten().max().unwrap()+1];

        let expected_symbols = vec![E, E, S, R, E, E, S, L, R, S, R, C, S /* handle */, R, C, S /* handle */, R, C, C, R, C, C, R, C, C, C, R, C, C, C, C, C];
        let expected_topology_splits = vec![
            TopologySplit {
                source_symbol_idx: 31,
                split_symbol_idx: 17,
                source_edge_orientation: Orientation::Left,
            },
            TopologySplit {
                source_symbol_idx: 28,
                split_symbol_idx: 20,
                source_edge_orientation: Orientation::Right,
            }
        ];

        manual_test::<false>(original_faces, points, expected_symbols, expected_topology_splits, None);
    }


    // #[test] 
    #[allow(unused)] // uncomment the test to run it. it is commented out as it takes a long time to run.
    fn connectivity_check_after_vertex_permutation() {
        let (bunny,_) = tobj::load_obj(
            format!("tests/data/punctured_sphere.obj"), 
            &tobj::GPU_LOAD_OPTIONS
        ).unwrap();
        let bunny = &bunny[0];
        let mesh = &bunny.mesh;

        let faces_original = mesh.indices.chunks(3)
            .map(|x| [x[0] as usize, x[1] as usize, x[2] as usize])
            .collect::<Vec<_>>();

        let mut faces = faces_original.clone();

        let points = mesh.positions.chunks(3)
            .map(|x| NdVector::<3,f32>::from([x[0], x[1], x[2]]))
            .collect::<Vec<_>>();

        let mut point_att = Attribute::from(AttributeId::new(0), points, AttributeType::Position, Vec::new());
        let mut edgebreaker = Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut [&mut point_att], &mut faces).is_ok());
        let mut writer = Vec::new();
        assert!(edgebreaker.encode_connectivity(&mut faces, &mut [&mut point_att], &mut writer).is_ok());


        assert!(eq::weak_eq_by_laplacian(&faces, &faces_original).unwrap());
    }
}

