use super::symbol_encoder::{Balanced, CrLight};
use super::Symbol;
use std::collections::VecDeque;
use std::vec;

use super::{
    EdgeBreaker, 
    Config,
    Err,
};

use crate::compression::connectivity::{
    ConnectivityEncoder,
    edgebreaker::symbol_encoder::{
        SymbolEncoder,
        SymbolEncodingConf
    }
};

use crate::core::shared::{
    FaceIdx, VertexIdx, EdgeIdx, ConfigType
};

use crate::core::buffer::writer::Writer;

impl EdgeBreaker {
	// Build the object with empty arrays.
	pub fn new()->Self {
        Self {
            edges: Vec::new(),
            coboundary_map_one: Vec::new(),
            coboundary_map_zero: None,
            visited_vertices: Vec::new(),
            visited_faces: Vec::new(),
            face_orientation: Vec::new(),
            active_edge_idx_stack: Vec::new(),
            cutting_paths: Vec::new(),
            symbols: Vec::new(),
            config: Config::default(),
        }
    }
	
	/// Initializes the edgebreaker. This function takes in a mesh and 
	/// decomposes it into manifolds with boundaries if it is not homeomorhic to a
	/// manifold. 
	pub(crate) fn init<CoordValType>(&mut self , points: &mut [[CoordValType;3]], faces: &[[VertexIdx; 3]], config: &Config) -> Result<(), Err> {
        debug_assert!(faces.is_sorted(), "Faces are not sorted");

        self.visited_vertices = vec!(false; points.len());
        self.visited_faces = vec!(false; faces.len());
        self.face_orientation = vec!(false; faces.len());

        self.compute_edges(faces);

        self.check_orientability(faces)?;
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
            self.coboundary_map_one.last_mut().unwrap().sort();
            self.coboundary_map_one.last_mut().unwrap().dedup();
            i = j;
        }
    }


    fn check_orientability(&mut self, faces: &[[VertexIdx; 3]]) -> Result<(), Err> {
        // we use 'visited_faces' to store the orientation of the faces.
        // since we use this for edgebreaker as well, this must be cleared at the end of the function.
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
                // Otherwise, push unchecked neighbors onto the queue.
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
                        let orientation_of_adjacent_face = {
                            let face_orientation_on_edge = |edge: [usize;2], face: [usize;3]| -> bool {
                                debug_assert!(edge.iter().all(|v| face.contains(v)), "edge: {:?}, face: {:?}", edge, face);
                                !(face[0]==edge[0] && face[2]==edge[1])
                            };
                            let edge = self.edges[edge_idx];
                            let face = faces[face_idx];
                            let adjacent_face = faces[adjacent_face_idx];

                            // safety: 'self.orientation.len()==self.faces.len()'.
                            unsafe {
                                if face_orientation_on_edge(edge, face) ^ face_orientation_on_edge(edge, adjacent_face) {
                                    *self.face_orientation.get_unchecked(face_idx)
                                } else {
                                    !*self.face_orientation.get_unchecked(face_idx)
                                }
                            }
                        };
                        unsafe{
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
	
	
	/// A function implementing a step of the edgebreaker algorithm.
	/// When this function returns, all the CLERS symbols are written in the
	/// buffer in the reverse order. Since the time complexity of 'find_vertices_pinching()' 
	/// is O(1), the complexity of this function (single recursive step) is also O(1).
	fn edge_breaker_recc(&mut self, faces: &[[VertexIdx; 3]], num_points: usize) -> Result<(), Err>{
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

        let symbol = if self.visited_vertices[next_vertex] {
            let right_edge_coboundary = &self.coboundary_map_one[right_edge_idx];
            let is_right_proceedable = match right_edge_coboundary.len() {
                1 => false,
                2 => {
                    let [f1_idx, f2_idx] = [right_edge_coboundary[0], right_edge_coboundary[1]];
                    !(self.visited_faces[f1_idx] || self.visited_faces[f2_idx])
                },
                3 => {
                    self.mark_vertices_along_edges_of_valency::<3>(right_edge_idx, num_points)?;
                    false
                },
                4 => {
                    self.mark_vertices_along_edges_of_valency::<4>(right_edge_idx, num_points)?;
                    false
                },
                5 => {
                    self.mark_vertices_along_edges_of_valency::<5>(right_edge_idx, num_points)?;
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
                    self.active_edge_idx_stack.push(left_edge_idx);
                    self.active_edge_idx_stack.push(right_edge_idx);
                    Symbol::S
                } else {
                    self.active_edge_idx_stack.push(right_edge_idx);
                    Symbol::R
                }
            } else {
                if is_left_proceedable {
                    self.active_edge_idx_stack.push(left_edge_idx);
                    Symbol::L
                } else {
                    Symbol::E
                }
            }
        } else {
            self.active_edge_idx_stack.push(right_edge_idx);
            Symbol::C
        };

        self.visited_vertices[next_vertex] = true;
        self.visited_faces[f_idx] = true;

        self.symbols.push(symbol);

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


    fn encode_symbols(&mut self, writer: &mut Writer, config: &Config) {
        let encoder = config.symbol_encoding;
        match encoder {
            SymbolEncodingConf::CrLight => {
                for &symbol in self.symbols.iter().rev() {
                    writer.next(CrLight::encode_symbol(symbol));
                }
            },
            SymbolEncodingConf::Balanced => {
                for &symbol in self.symbols.iter().rev() {
                    writer.next(Balanced::encode_symbol(symbol));
                }
            },
            SymbolEncodingConf::Rans => {
                // ToDo: come back here after implementing the rANS encoder.
                unimplemented!();
            }
        }
    }

    /// begins the edgebreaker iteration from the given edge.
    fn begin_from(&mut self, e_idx: EdgeIdx, num_points: usize) -> Result<(), Err> {
        // active_edge_idx_stack must be empty at the beginning.
        debug_assert!(self.active_edge_idx_stack.is_empty());

        self.active_edge_idx_stack.push(e_idx);
        let e = self.edges[e_idx];
        // mark the face and the vertices as visited.
        {
            self.visited_vertices[e[0]] = true;
            self.visited_vertices[e[1]] = true;
        }

        if self.coboundary_map_one[e_idx].len() == 1 {
            // This means that the edge is a boundary.
            // we need to mark all the vertices in this boundary as visited.
            self.mark_vertices_as_visited_in_boundary_containing(e_idx, num_points)?;
        } else if self.coboundary_map_one[e_idx].len() != 2 {
            // This means that the edge is part of a cutting path.
            // we need to find the next vertex to visit.
            self.mark_vertices_as_visited_in_cutting_path_containing(e_idx, num_points)?;
        }
        // if 'self.coboundary_map_one[e_idx].len() == 2', then we do not need to do anything, since it is neither a boundary nor a cutting path.

        Ok(())
    }


    fn mark_vertices_as_visited_in_boundary_containing(&mut self, e_idx: EdgeIdx, num_points: usize) -> Result<(), Err> {
        self.mark_vertices_along_edges_of_valency::<1>(e_idx, num_points)
    }

    fn mark_vertices_as_visited_in_cutting_path_containing(&mut self, e_idx: EdgeIdx, num_points: usize) -> Result<(), Err> {
        let valency = self.coboundary_map_one[e_idx].len();
        match valency {
            3 => self.mark_vertices_along_edges_of_valency::<3>(e_idx, num_points),
            4 => self.mark_vertices_along_edges_of_valency::<4>(e_idx, num_points),
            5 => self.mark_vertices_along_edges_of_valency::<5>(e_idx, num_points),
            _ => unreachable!("An internal error occured while running Edgebreaker."),
        }
    }

    fn mark_vertices_along_edges_of_valency<const VALENCY: usize>(&mut self, e_idx: EdgeIdx, num_points: usize) -> Result<(), Err> {
        // we need to cut the mesh at this edge.
        // first create a zero dimensianal coboundary map.
        if self.coboundary_map_zero.is_none() {
            self.coboundary_map_zero = {
                let mut out = Vec::new();
                out.reserve(num_points);
                for _ in 0..num_points {
                    out.push(Vec::new());
                }
                for (edges_idx, e) in self.edges.iter().enumerate() {
                    out[e[0]].push(edges_idx);
                    out[e[1]].push(edges_idx);
                }
                Some(out)
            };
        };

        // Safety: The coboundary map is created just now.
        let coboundary_map_zero = unsafe{ self.coboundary_map_zero.as_ref().unwrap_unchecked() };

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
                cutting_path.push(next_vertex);
            }
        }
        
        self.cutting_paths.push(cutting_path);

        Ok(())
    }
}	

impl ConnectivityEncoder for EdgeBreaker {
    type Config = Config;
	type Err = Err;
	/// The main encoding paradigm for edgebreaker.
	fn encode_connectivity<CoordValType>(&mut self, faces: &[[VertexIdx; 3]], config: &Config, points: &mut[[CoordValType;3]], buffer: &mut Writer) -> Result<(), Self::Err> {
        self.init(points, faces, config)?;

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
            self.begin_from(unvisited_edge_idx, points.len())?;

            // run Edgebreaker
			while !self.active_edge_idx_stack.is_empty() { 
                self.edge_breaker_recc(faces, points.len())?;
            }

            self.encode_symbols(buffer, config);

            self.symbols.clear();
		}
        Ok(())
	}
}


#[cfg(test)]
mod tests {
    use core::panic;
    use std::vec;

    use crate::core::buffer;

    use super::*;

    #[test]
    fn test_decompose_into_manifolds_simple() {
        let mut points = vec![[0_f32;3];8];
        let mut faces = vec![
            [0, 1, 6], // 0
            [1, 6, 7], // 1
            [2, 3, 6], // 2
            [3, 6, 7], // 3
            [4, 5, 6], // 4
            [5, 6, 7], // 5
        ];
        let mut edgebreaker = EdgeBreaker::new();

        assert!(edgebreaker.init(&mut points, &mut faces, &Config::default()).is_ok());

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
        let mut edgebreaker = EdgeBreaker::new();

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
        let mut edgebreaker = EdgeBreaker::new();
        
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
        let mut edgebreaker = EdgeBreaker::new();

        edgebreaker.face_orientation = vec!(false; faces.len());
        edgebreaker.visited_faces = vec!(false; faces.len());
        edgebreaker.compute_edges(&faces);
        assert!(edgebreaker.check_orientability(&faces).is_err());
    }

    // #[test]
    fn test_symbols() {
        // positions do not matter
        let mut points = vec![[0_f32; 3]; 12];
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

        let mut writer = buffer::writer::Writer::new();
        EdgeBreaker::new().encode_connectivity(&faces, &Config::default(), &mut points, &mut writer).unwrap();

        let buffer: buffer::Buffer = writer.into();
        let size = buffer.len();
        let mut reader = buffer.into_reader();

        use Symbol::*;
        let mut read_symbols = || -> Vec<Symbol> {
            let mut out = Vec::new();
            let mut i = 0;
            while i < size {
                let first = reader.next(1);
                i += 1;
                if first==0 {
                    out.push(C);
                    i += 1;
                } else {
                    let second = reader.next(1);
                    i += 1;
                    if second==0 {
                        out.push(R);
                    } else {
                        let third = reader.next(2);
                        i += 2;
                        if third==0 {
                            out.push(L);
                        } else if third==1 {
                            out.push(E);
                        } else if third==2 {
                            out.push(S);
                        } else {
                            out.push(M);
                        }
                    }
                }
            }
            out
        };

        let answer = vec![C];

        assert_eq!(answer, read_symbols());

    }
}
