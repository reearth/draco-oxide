use std::fmt::Debug;
use std::{
    fmt,
    cmp,
};

use crate::shared::connectivity::edgebreaker::symbol_encoder::{
	SymbolEncodingConfig,
	Symbol
};

use crate::core::shared::{ConfigType, EdgeIdx, FaceIdx, VertexIdx};

use crate::shared::connectivity::edgebreaker::symbol_encoder::{
	Balanced, 
	CrLight,
	SymbolEncoder,
};
use crate::shared::connectivity::edgebreaker::{NUM_CONNECTED_COMPONENTS_SLOT, NUM_FACES_SLOT};
use crate::shared::connectivity::EdgebreakerDecoder;
use std::collections::VecDeque;
use std::vec;

use crate::encode::connectivity::ConnectivityEncoder;

use crate::core::shared::NdVector;

pub(crate) struct Edgebreaker {
	/// 'edges' is a set of edges of the input mesh, each of which is a two-element 
	/// non-multiset sorted in the increasing order. 'edges' itself is also sorted 
	/// by the initial vertex of its edges in the increasing order.
	edges: Vec<[VertexIdx;2]>,
	
	/// 'coboundary_map_one' records the coboundary information of edges, i.e. the i'th 
	/// entry of this array stores the indexes of the faces that have 'edge[i]'
	/// as the boundary. 
	coboundary_map_one: Vec<Vec<FaceIdx>>,


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
	active_edge_idx_stack: Vec<EdgeIdx>,

	/// This stores the information of the decomposition.
	/// Each element of the vector is a list of vertex indexes that forms a path along a cut.
	cutting_paths: Vec<Vec<VertexIdx>>,

	symbols: Vec<Symbol>,

	/// The orientation of the faces. The 'i'th entry of this array stores the orientation of the 'i'th face.
	face_orientation: Vec<bool>,

    handle_edges: Vec<(usize, [VertexIdx;2])>,

    /// Stores the face corresponding to each symbol in the resulting string.
    /// This will be used in the case of reverse decoding.
    symbol_idx_to_face_idx: Vec<usize>,

    vertex_decode_order: Vec<usize>,

    face_decode_order: Vec<usize>,

    num_decoded_vertices: usize,
	
	/// configurations for the encoder
	config: Config
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
    #[error("Faces are not sorted.")]
    FaceNotSorted,
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
            coboundary_map_zero: None,
            lies_on_boundary_or_cutting_path: Vec::new(),
            visited_vertices: Vec::new(),
            visited_faces: Vec::new(),
            num_connected_components: 0,
            face_orientation: Vec::new(),
            active_edge_idx_stack: Vec::new(),
            cutting_paths: Vec::new(),
            symbols: Vec::new(),
            handle_edges: Vec::new(),
            symbol_idx_to_face_idx: Vec::new(),
            vertex_decode_order: Vec::new(),
            face_decode_order: Vec::new(),
            num_decoded_vertices: 0,
            config,
        }
    }
	
	/// Initializes the Edgebreaker. This function takes in a mesh and 
	/// decomposes it into manifolds with boundaries if it is not homeomorhic to a
	/// manifold. 
	pub(crate) fn init<CoordValType>(&mut self , points: &mut [NdVector<3, CoordValType>], faces: &[[VertexIdx; 3]]) -> Result<(), Err> {
        if !faces.is_sorted() || !faces.iter().all(|x| x.is_sorted()) {
            return Err(Err::FaceNotSorted);
        }
        self.visited_vertices = vec!(false; points.len());
        self.visited_faces = vec!(false; faces.len());
        self.face_orientation = vec!(false; faces.len());

        self.num_connected_components = 0;

        self.edges.clear();
        self.coboundary_map_one.clear();
        self.coboundary_map_zero = None;
        self.lies_on_boundary_or_cutting_path = vec![false; points.len()];

        self.compute_edges(faces);

        self.check_orientability(faces)?;
        self.vertex_decode_order = vec![usize::MAX; points.len()];
        self.face_decode_order = vec![usize::MAX; faces.len()];
        self.num_decoded_vertices = 0;
        Ok(())
	}

    /// computes all the edges of the mesh and returns the raw coboundary map.
    fn compute_edges(&mut self, faces: &[[VertexIdx; 3]]) {
        // input faces must be sorted.
        debug_assert!(faces.is_sorted(), "Faces are not sorted");

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
	/// When this function returns, all the CLERS symbols are written in the
	/// buffer in the reverse order. Since the time complexity of 'find_vertices_pinching()' 
	/// is O(1), the complexity of this function (single recursive step) is also O(1).
	fn edgebreaker_recc<const REVERSE_DECODE: bool>(&mut self, faces: &[[VertexIdx; 3]]) -> Result<(), Err> {
        let active_edge_idx = self.active_edge_idx_stack.pop().unwrap();
        let active_edge = self.edges[active_edge_idx];
        let active_edge_coboundary = &self.coboundary_map_one[active_edge_idx];

        let f_idx = match active_edge_coboundary.len() {
            1 =>
                // safety: just checked
                unsafe{ *active_edge_coboundary.get_unchecked(0) }

            ,
            2 => {
                // safety: just checked
                let f1_idx = unsafe{ *active_edge_coboundary.get_unchecked(0) };
                let f2_idx = unsafe{ *active_edge_coboundary.get_unchecked(1) };
                if self.visited_faces[f1_idx] {
                    f2_idx
                } else {
                    f1_idx
                }
            },
            _ => unreachable!("An internal error occured while running Edgebreaker."),
        };
        
        
        let next_vertex = Self::the_other_vertex(active_edge, faces[f_idx]);

        let right_vertex = if active_edge[0] < next_vertex && next_vertex < active_edge[1] {
            active_edge[if self.face_orientation[f_idx] {0} else {1}]
        } else {
            active_edge[if self.face_orientation[f_idx] {1} else {0}]
        };
        // ToDo: this binary search can be optimized.
        let right_edge_idx = self.edges.binary_search( & if next_vertex < right_vertex {
            [next_vertex, right_vertex]
        } else {
            [right_vertex, next_vertex]
        }).unwrap();


        // if 'f_idx' is already visited, then this must be an edge of previous 'H'.
        // update its metadata and return.
        let mut maybe_handle_idx = None;
        for (i, &(symbol_idx, edge)) in self.handle_edges.iter().enumerate() {
            // ToDo: This condition can be optimized.
            if faces[f_idx].contains(&edge[0]) && faces[f_idx].contains(&edge[1]) {
                let is_right_not_left = if edge.contains(&right_vertex) { 1 } else { 0 };
                let metadata = self.symbols.len() - symbol_idx << 1 | is_right_not_left;
                self.symbols[symbol_idx] = Symbol::H(metadata);
                maybe_handle_idx = Some(i);
                break;
            }
        }
        if let Some(handle_idx) = maybe_handle_idx {
            self.handle_edges.remove(handle_idx);
        }

        let symbol = if self.visited_vertices[next_vertex] || self.lies_on_boundary_or_cutting_path[next_vertex] {
            let right_edge_coboundary = &self.coboundary_map_one[right_edge_idx];
            let is_right_proceedable = match right_edge_coboundary.len() {
                1 => false,
                2 => {
                    let [f1_idx, f2_idx] = [right_edge_coboundary[0], right_edge_coboundary[1]];
                    !(self.visited_faces[f1_idx] || self.visited_faces[f2_idx])
                },
                3 => {
                    self.mark_vertices_along_edges_of_valency::<3>(right_edge_idx)?;
                    false
                },
                4 => {
                    self.mark_vertices_along_edges_of_valency::<4>(right_edge_idx)?;
                    false
                },
                5 => {
                    self.mark_vertices_along_edges_of_valency::<5>(right_edge_idx)?;
                    false
                },
                _ => unreachable!("An internal error occured while running Edgebreaker."),
            };

            let left_vertex = if active_edge[0] == right_vertex {
                active_edge[1]
            } else {
                active_edge[0]
            };
            // ToDo: this binary search can be optimized.
            let left_edge_idx = self.edges.binary_search( & if next_vertex < left_vertex {
                [next_vertex, left_vertex]
            } else {
                [left_vertex, next_vertex]
            }).unwrap();
            let left_edge_coboundary = &self.coboundary_map_one[left_edge_idx];
            let is_left_proceedable = if left_edge_coboundary.len()==2 {
                let [f1_idx, f2_idx] = [left_edge_coboundary[0], left_edge_coboundary[1]];
                !(self.visited_faces[f1_idx] || self.visited_faces[f2_idx])
            } else {
                false
            };

            if is_right_proceedable {
                if is_left_proceedable {
                    if !self.lies_on_boundary_or_cutting_path[next_vertex] && 
                        self.visited_vertices[next_vertex] &&
                        self.are_edges_connected_via_unvisited_faces(f_idx, left_edge_idx, right_edge_idx, faces)
                    {
                        // case H: handle
                        let metadata = if REVERSE_DECODE {
                            self.active_edge_idx_stack.push(right_edge_idx);
                            self.handle_edges.push((self.symbols.len(), self.edges[left_edge_idx]));
                            0
                        } else {
                            self.active_edge_idx_stack.push(right_edge_idx);
                            self.measure_hole_size(f_idx, next_vertex, left_vertex, faces)
                        };
                        Symbol::H(metadata)
                    } else if self.lies_on_boundary_or_cutting_path[next_vertex] && !self.visited_vertices[next_vertex] {
                        // case M: merge holes
                        self.active_edge_idx_stack.push(right_edge_idx);
                        if self.coboundary_map_zero.is_none() {
                            self.compute_coboundary_map_zero();
                        };
                        let coboundary_map_zero = unsafe{ self.coboundary_map_zero.as_ref().unwrap_unchecked() };
                        let boundary_edge = *coboundary_map_zero[next_vertex].iter()
                                                    .find(|&&coboundary_edge|self.coboundary_map_one[coboundary_edge].len() == 1 )
                                                    .unwrap();
                        let hole_size = self.mark_vertices_as_visited_in_boundary_containing(boundary_edge)?;
                        
                        Symbol::M(hole_size)
                    } else {
                        // case S: split
                        self.active_edge_idx_stack.push(left_edge_idx);
                        self.active_edge_idx_stack.push(right_edge_idx);
                        Symbol::S
                    }
                } else {
                    self.active_edge_idx_stack.push(right_edge_idx);
                    Symbol::L
                }
            } else {
                if is_left_proceedable {
                    self.active_edge_idx_stack.push(left_edge_idx);
                    Symbol::R
                } else {
                    Symbol::E
                }
            }
        } else {
            self.active_edge_idx_stack.push(right_edge_idx);
            Symbol::C
        };
        self.symbols.push(symbol);
        // if we are reverse-decoding, we need to store the face corresponding to the symbol in order
        // to compute the order of the vertices when decoding.
        if REVERSE_DECODE {
            self.symbol_idx_to_face_idx.push(f_idx);
        }

        self.visited_vertices[next_vertex] = true;
        self.visited_faces[f_idx] = true;

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


    fn encode_symbols<F>(&mut self, writer: &mut F) -> Result<(), Err> 
        where F: FnMut((u8, u64)),
    {
        let encoder = self.config.symbol_encoding;
        match encoder {
            SymbolEncodingConfig::CrLight => {
                for &symbol in self.symbols.iter().rev() {
                    match CrLight::encode_symbol(symbol) {
                        Ok((size, value)) => writer((size, value)),
                        Err(err) => return Err(err),
                    }
                }
            },
            SymbolEncodingConfig::Balanced => {
                for &symbol in self.symbols.iter().rev() {
                    match Balanced::encode_symbol(symbol) {
                        Ok((size, value)) => writer((size, value)),
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
    fn begin_from(&mut self, e_idx: EdgeIdx) -> Result<(), Err> {
        // active_edge_idx_stack must be empty at the beginning.
        debug_assert!(self.active_edge_idx_stack.is_empty());

        self.active_edge_idx_stack.push(e_idx);
        let e = self.edges[e_idx];
        // mark the face and the vertices as visited.
        {
            self.visited_vertices[e[0]] = true;
            self.visited_vertices[e[1]] = true;
        }

        if self.lies_on_boundary_or_cutting_path[e[0]] {
            if self.coboundary_map_zero.is_none() {
                self.compute_coboundary_map_zero();
            };
            let coboundary_map_zero = unsafe{ self.coboundary_map_zero.as_ref().unwrap_unchecked() };
            for coboundary_edge in coboundary_map_zero[e[0]].clone() {
                if self.coboundary_map_one[coboundary_edge].len() == 1 {
                    self.mark_vertices_as_visited_in_boundary_containing(coboundary_edge)?;
                } else if self.coboundary_map_one[coboundary_edge].len() > 2 {
                    self.mark_vertices_as_visited_in_cutting_path_containing(coboundary_edge)?;
                }
            }
        } else if self.lies_on_boundary_or_cutting_path[e[1]] {
            if self.coboundary_map_zero.is_none() {
                self.compute_coboundary_map_zero();
            };
            let coboundary_map_zero = unsafe{ self.coboundary_map_zero.as_ref().unwrap_unchecked() };
            for coboundary_edge in coboundary_map_zero[e[1]].clone() {
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


    fn compute_decode_order(&mut self, faces: &mut [[VertexIdx; 3]]) {
        debug_assert!(
            self.symbol_idx_to_face_idx.len() == self.symbols.len(), 
            "symbol_idx_to_face_idx.len(): {}, symbols.len(): {}", 
            self.symbol_idx_to_face_idx.len(), self.symbols.len()
        );

        // fill 'face_decode_order'.
        for (i, face_idx) in self.symbol_idx_to_face_idx.iter().rev().enumerate() {
            self.face_decode_order[*face_idx] = i;
        }

        // fill 'vertex_decode_order'.
        for i in (0..self.symbols.len()).rev() {
            let symbol = self.symbols[i];
            let face = faces[self.symbol_idx_to_face_idx[i]];
            match symbol {
                Symbol::E => {
                    if self.symbols.len() == 1 {
                        for v  in face {
                            self.vertex_decode_order[v] = self.num_decoded_vertices;
                            self.num_decoded_vertices += 1;
                        }
                        return;
                    }
                    let j = if self.symbols[i-1] == Symbol::E {
                        let mut idx = i-1;
                        let mut count = 2;
                        // find 'S' that matches the two 'E's.
                        while count > 0 {
                            idx -= 1;
                            match self.symbols[idx] {
                                Symbol::E => count+=1,
                                Symbol::S => count-=2,
                                _ => {}
                            }
                        }
                        idx
                    } else {
                        i-1
                    };
                    let adjacent_face = faces[self.symbol_idx_to_face_idx[j]];
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
                Symbol::H(_metadata) => {
                    for v in face {
                        if self.vertex_decode_order[v] == usize::MAX {
                            self.vertex_decode_order[v] = self.num_decoded_vertices;
                            self.num_decoded_vertices += 1;
                        }
                    }
                },
                Symbol::M(_hole_size) => {
                    for v in face {
                        if self.vertex_decode_order[v] == usize::MAX {
                            self.vertex_decode_order[v] = self.num_decoded_vertices;
                            self.num_decoded_vertices += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn reflect_decode_order<CoordValType: Copy + std::fmt::Debug>(&self, faces: &mut [[VertexIdx; 3]], points: &mut [NdVector<3, CoordValType>]) 
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
        for f in &mut *faces {
            f.sort();
        }
        
        // reflect the vertex order to the points.
        // ToDo: Optimize this.
        let mut new_points = points.to_vec();
        for (i, &new_idx) in self.vertex_decode_order.iter().enumerate() {
            new_points[new_idx] = points[i];
        }
        for (a,b) in points.iter_mut().zip(new_points.into_iter()) {   
            *a = b;
        }

        // sort the faces.
        let mut new_faces = vec![[0;3]; faces.len()];
        for (i, &f_idx) in self.face_decode_order.iter().enumerate() {
            new_faces[f_idx] = faces[i];
        }
        for (a,b) in faces.iter_mut().zip(new_faces.into_iter()) {   
            *a = b;
        }
    }
}	

impl ConnectivityEncoder for Edgebreaker {
    type Config = Config;
	type Err = Err;
	/// The main encoding paradigm for Edgebreaker.
    fn encode_connectivity<CoordValType: Copy + Debug, F>(
        &mut self, 
        faces: &mut [[VertexIdx; 3]], 
        points: &mut [NdVector<3, CoordValType>], 
        writer: &mut F
    ) -> Result<(), Self::Err> 
        where 
            F: FnMut((u8, u64))
    {
        // encode the encoding configuration
        self.config.symbol_encoding.write_symbol_encoding(writer);
        
        self.init(points, faces)?;

        if self.num_connected_components > 255 {
            return Err(Err::TooManyConnectedComponents(self.num_connected_components));
        }
        writer((NUM_CONNECTED_COMPONENTS_SLOT, self.num_connected_components as u64));

		// Run Edgebreaker once for each connected component.
		while let Some(f_idx) = self.get_some_unvisited_triangle(faces) {
            let unvisited_edge_idx = {
                let face = faces[f_idx];
                let edge_idx = [face[0], face[1]];
                // safety: see 'compute_edges()'.
                unsafe {
                    self.edges.binary_search(&edge_idx).unwrap_unchecked()
                }
            };
            self.begin_from(unvisited_edge_idx)?;

            // run Edgebreaker
			while !self.active_edge_idx_stack.is_empty() {
                self.edgebreaker_recc::<true>(faces)?;
            }

            writer((NUM_FACES_SLOT, self.symbols.len() as u64));

            self.encode_symbols(writer)?;

            self.compute_decode_order(faces);
            self.symbols.clear();
            self.symbol_idx_to_face_idx.clear();
		}

        self.reflect_decode_order(faces, points);
        Ok(())
	}
}


#[cfg(test)]
mod tests {
    use std::vec;

    use crate::{core::{buffer::{self, MsbFirst}, shared::Vector}, shared::connectivity::edgebreaker::{NUM_CONNECTED_COMPONENTS_SLOT, NUM_FACES_SLOT, SYMBOL_ENCODING_CONFIG_SLOT}};

    use super::*;

    #[test]
    fn test_decompose_into_manifolds_simple() {
        let mut points = vec![NdVector::<3,f32>::zero();8];
        let mut faces = vec![
            [0, 1, 6], // 0
            [1, 6, 7], // 1
            [2, 3, 6], // 2
            [3, 6, 7], // 3
            [4, 5, 6], // 4
            [5, 6, 7], // 5
        ];
        let mut edgebreaker = Edgebreaker::new(Config::default());

        assert!(edgebreaker.init(&mut points, &mut faces).is_ok());

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

    #[test]
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
    fn read_symbols<F>(mut reader: F, size: usize) -> Vec<Symbol> 
        where F: FnMut(u8) -> u64
    {
        let mut out = Vec::new();
        for _ in 0..size {
            out.push(
                CrLight::decode_symbol(&mut reader)
            );
        }
        out
    }

    #[test]
    fn too_many_connected_components() {
        let mut faces = (0..255).map(|a|[3*a, 3*a+1, 3*a+2]).collect::<Vec<_>>();
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input: (u8, u64)| buff_writer.next(input);
        Edgebreaker::new(Config::default()).encode_connectivity(&mut faces, &mut points, &mut writer).unwrap();

        let buffer: buffer::Buffer = buff_writer.into();
        let mut reader = buffer.into_reader();

        assert_eq!(reader.next(SYMBOL_ENCODING_CONFIG_SLOT), 0);
        assert_eq!(reader.next(NUM_CONNECTED_COMPONENTS_SLOT), 255);
        for _ in 0..255 {
            assert_eq!(reader.next(NUM_FACES_SLOT), 1);
            assert_eq!(reader.next(4), 0b1101 /* E */);
        }

        // too many components
        let mut faces = (0..256).map(|a|[3*a, 3*a+1, 3*a+2]).collect::<Vec<_>>();
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut buff_writer = buffer::writer::Writer::<MsbFirst>::new();
        let mut writer = |input| buff_writer.next(input);
        let err= Edgebreaker::new(Config::default()).encode_connectivity(&mut faces, &mut points, &mut writer).unwrap_err();

        assert_eq!(err, Err::TooManyConnectedComponents(256));
    }

    #[test]
    fn edgebreaker_disc() {
        let mut faces = vec![
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
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input: (u8, u64)| buff_writer.next(input);
        Edgebreaker::new(Config::default()).encode_connectivity(&mut faces, &mut points, &mut writer).unwrap();

        let buffer: buffer::Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input: u8| buff_reader.next(input);

        assert_eq!(reader(SYMBOL_ENCODING_CONFIG_SLOT), 0);
        assert_eq!(reader(NUM_CONNECTED_COMPONENTS_SLOT), 1);
        assert_eq!(reader(NUM_FACES_SLOT), faces.len() as u64);

        let answer = vec![E,E,S,R,L,R,R,C,C,R,R,R,C,C];
        assert_eq!(answer, read_symbols(reader, faces.len()));

        let faces_decode = [
            [0,1,2],
            [1,3,4],
            [0,1,3],
            [0,3,5],
            [0,5,6],
            [5,6,7],
            [6,7,8],
            [0,6,8],
            [0,2,8],
            [2,8,9],
            [2,9,10],
            [2,10,11],
            [1,2,11],
            [1,4,11]
        ];
        assert_eq!(faces, faces_decode);
    }

    #[test]
    fn edgebreaker_split() {
        let mut faces = vec![
            [0,1,2],
            [0,2,4],
            [0,4,5],
            [2,3,4]
        ];
        // positions do not matter
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input: (u8, u64)| buff_writer.next(input);
        Edgebreaker::new(Config::default()).encode_connectivity(&mut faces, &mut points, &mut writer).unwrap();

        let buffer: buffer::Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input: u8| buff_reader.next(input);

        assert_eq!(reader(SYMBOL_ENCODING_CONFIG_SLOT), 0);
        assert_eq!(reader(NUM_CONNECTED_COMPONENTS_SLOT), 1);
        assert_eq!(reader(NUM_FACES_SLOT), faces.len() as u64);

        let answer = vec![E,E,S,R];

        assert_eq!(answer, read_symbols(reader, faces.len()));

        assert_eq!(faces, vec![
            [0,1,2], [1,3,4], [0,1,3], [0,3,5]
        ]);
    }

    #[test]
    fn edgebreaker_begin_from_center() {
        // mesh forming a square whose initial edge is not on the boundary.
        let mut faces = [
            [9,23,24], [8,9,23], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,22,23], [7,8,22], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,21,22], [6,7,21], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,20,21], [6,19,20], [5,6,19], [5,18,19], [4,5,18], [4,17,18], [4,15,17], [15,16,17]
        ];
        faces.sort();
        // positions do not matter
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input: (u8, u64)| buff_writer.next(input);
        Edgebreaker::new(Config::default()).encode_connectivity(&mut faces, &mut points, &mut writer).unwrap();

        let buffer: buffer::Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input: u8| buff_reader.next(input);

        assert_eq!(reader(SYMBOL_ENCODING_CONFIG_SLOT), 0);
        assert_eq!(reader(NUM_CONNECTED_COMPONENTS_SLOT), 1);
        assert_eq!(reader(NUM_FACES_SLOT), faces.len() as u64);

        let answer = vec![E, E, E, S, R, L, R, L, R, R, L, R, S, R, E, S, R, C, R, E, L, S, R, C, C, C, R, C, C, L, M(16), C];

        assert_eq!(answer, read_symbols(reader, faces.len()));
    }

    #[test]
    fn edgebreaker_handle() {
        // create torus in order to test the handle symbol.
        let mut faces = [
            [9,12,13], [8,9,13], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,13,14], [7,8,14], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,14,15], [6,7,15], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,12,15], [6,9,12], [5,6,9], [5,9,10], [4,5,10], [4,10,11], [4,11,15], [11,12,15]
        ];
        faces.sort();
        // positions do not matter
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut buff_writer = buffer::writer::Writer::new();
        let mut writer = |input: (u8, u64)| buff_writer.next(input);
        Edgebreaker::new(Config::default()).encode_connectivity(&mut faces, &mut points, &mut writer).unwrap();

        let buffer: buffer::Buffer = buff_writer.into();

        let mut buff_reader = buffer.into_reader();
        let mut reader = |input: u8| buff_reader.next(input);

        assert_eq!(reader(SYMBOL_ENCODING_CONFIG_SLOT), 0);
        assert_eq!(reader(NUM_CONNECTED_COMPONENTS_SLOT), 1);
        assert_eq!(reader(NUM_FACES_SLOT), faces.len() as u64);

        let answer = vec![E, E, S, R, E, E, S, L, R, S, R, C, H(17), R, C, H(29), R, C, C, R, C, C, R, C, C, C, R, C, C, C, C, C];

        assert_eq!(answer, read_symbols(reader, faces.len()));
    }
}

