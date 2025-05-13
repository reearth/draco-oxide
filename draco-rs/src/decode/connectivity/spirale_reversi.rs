use std::cmp;
use crate::decode::connectivity::ConnectivityDecoder;
use crate::core::shared::VertexIdx;

use crate::shared::connectivity::edgebreaker::symbol_encoder::{
    Balanced, CrLight, Rans, Symbol, SymbolEncoder, SymbolEncodingConfig
};

use std::mem;

use crate::shared::connectivity::edgebreaker::{
    orientation_of_next_face, NUM_CONNECTED_COMPONENTS_SLOT, NUM_FACES_SLOT
};

#[derive(thiserror::Error, Debug, Clone)]
#[remain::sorted]
pub enum Err {
    #[error("Stream input returned with None, though more data was expected.")]
    NotEnoughData,
}


pub(crate)struct SpiraleReversi {
    faces: Vec<[VertexIdx; 3]>,
    num_connected_components: usize,
    num_faces: Vec<usize>,
    num_decoded_vertices: usize,
    // active edge is oriented from right to left.
    active_edge: [usize; 2],
    active_edge_stack: Vec<[usize; 2]>,
    boundary_edges: Vec<[usize; 2]>,
    prev_face: [usize;3],
    orientation: Vec<bool>,
}

impl SpiraleReversi {
    pub(super) fn new() -> Self {
        Self {
            faces: vec![],
            num_connected_components: 0,
            num_faces: Vec::new(),
            num_decoded_vertices: 0,
            active_edge: [0,1],
            active_edge_stack: Vec::new(),
            boundary_edges: Vec::new(),
            prev_face: [0,1,2],
            orientation: Vec::new(),
        }
    }

    pub(super) fn init(&mut self) {
        self.faces.clear();
        self.num_connected_components = 0;
        self.num_faces.clear();
        self.num_decoded_vertices = 0;
        self.active_edge = [0,1];
        self.active_edge_stack.clear();
        self.boundary_edges.clear();
        self.prev_face = [0,1,2];
    }

    fn spirale_reversi_impl<SE: SymbolEncoder, F: FnMut(u8)->u64>(&mut self, reader: &mut F) {
        // move the value in order to avoid the borrow checker.
        let mut num_faces = Vec::new();
        mem::swap(&mut num_faces, &mut self.num_faces);
        for _ in 0..self.num_connected_components {
            // get the number of faces = number of symbols.
            let num_faces = reader(NUM_FACES_SLOT);
            self.num_decoded_vertices += 2;
            self.active_edge_stack.clear();
            self.active_edge = [
                self.num_decoded_vertices-2,
                self.num_decoded_vertices-1
            ];
            for i in 0..num_faces {
                self.spirale_reversi_recc::<SE, F>(reader);
            }
        }
    }

    #[inline]
    fn spirale_reversi_recc<SE: SymbolEncoder, F: FnMut(u8)->u64>(&mut self, reader: &mut F) {
        match SE::decode_symbol(reader) {
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
            Symbol::M(n_vertices) => {
                // a hole starting and ending at 'self.active_edge[0]' must get created.
                let mut prev_vertex = self.active_edge[0];
                let mut curr_vertex = *self.boundary_edges.iter()
                    .filter(|e| e.contains(&self.active_edge[0]))
                    .find(|&&e| !e.contains(&self.active_edge[1]))
                    .unwrap()
                    .iter().find(|&&v| v != self.active_edge[0]).unwrap();
                let mut boundary_to_remove = vec![
                    self.boundary_edges.binary_search(
                        &[cmp::min(prev_vertex, curr_vertex), cmp::max(prev_vertex, curr_vertex)]
                    ).unwrap()
                ];
                for _ in 0..n_vertices-1 {
                    let next_vertex = *self.boundary_edges.iter()
                        .filter(|e| e.contains(&curr_vertex))
                        .find(|&&e| !e.contains(&prev_vertex))
                        .unwrap()
                        .iter().find(|&&v| v != curr_vertex).unwrap();
                
                    prev_vertex = curr_vertex;
                    curr_vertex = next_vertex;
                    boundary_to_remove.push(
                        self.boundary_edges.binary_search(
                            &[cmp::min(prev_vertex, curr_vertex), cmp::max(prev_vertex, curr_vertex)]
                        ).unwrap()
                    );
                }
                // find the next vertex once more to get the active edge.
                let mut next_vertex = *self.boundary_edges.iter()
                    .filter(|e| e.contains(&curr_vertex))
                    .find(|&&e| !e.contains(&prev_vertex))
                    .unwrap()
                    .iter().find(|&&v| v != curr_vertex).unwrap();
                // remove the active edge from the boundary edges.
                boundary_to_remove.sort();
                for idx in boundary_to_remove.iter().rev() {
                    self.boundary_edges.remove(*idx);
                }
                let removed_edge = [
                    cmp::min(self.active_edge[0], self.active_edge[1]),
                    cmp::max(self.active_edge[0], self.active_edge[1]),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap()
                );
                let removed_edge = [
                    cmp::min(curr_vertex, next_vertex),
                    cmp::max(curr_vertex, next_vertex),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap()
                );

                let mut new_face = [
                    curr_vertex,
                    self.active_edge[1],
                    next_vertex
                ];
                new_face.sort();
                self.faces.push(new_face);

                // renumber the vertices
                for face in &mut self.faces {
                    let mut is_face_updated = false;
                    for vertex in &mut *face {
                        if *vertex == self.active_edge[0] {
                            *vertex = curr_vertex;
                            is_face_updated = true;
                        } else if *vertex > self.active_edge[0] {
                            *vertex -= 1;
                        }
                    }
                    if is_face_updated {
                        face.sort();
                    }
                }
                for edge in &mut self.active_edge_stack {
                    for vertex in edge.iter_mut() {
                        if *vertex == self.active_edge[0] {
                            *vertex = curr_vertex;
                        } else if *vertex > self.active_edge[0] {
                            *vertex -= 1;
                        }
                    }
                }
                if self.active_edge[1] > self.active_edge[0] {
                    self.active_edge[1] -= 1;
                }
                for edge in &mut self.boundary_edges {
                    for vertex in edge.iter_mut() {
                        if *vertex == self.active_edge[0] {
                            *vertex = curr_vertex;
                        } else if *vertex > self.active_edge[0] {
                            *vertex -= 1;
                        }
                    }
                    edge.sort();
                }
                self.boundary_edges.sort();
                if next_vertex == self.active_edge[0] {
                    next_vertex = curr_vertex;
                } else if next_vertex > self.active_edge[0] {
                    next_vertex -= 1;
                }


                // add the new edge to the boundary edges
                let new_edge = [
                    cmp::min(next_vertex, self.active_edge[1]),
                    cmp::max(next_vertex, self.active_edge[1]),
                ];
                let idx = self.boundary_edges.binary_search(&new_edge).unwrap_err();
                self.boundary_edges.insert(idx, new_edge);

                self.num_decoded_vertices -= 1;
                self.active_edge = [self.active_edge[1], next_vertex];
            },
            Symbol::H(metadata) => {
                unimplemented!();
                let mut new_face = [
                    self.active_edge[0],
                    self.active_edge[1],
                    self.num_decoded_vertices
                ];
                new_face.sort();
                self.faces.push(new_face);
                
                // remove the active edge from the boundary edges.
                let removed_edge = [
                    cmp::min(self.active_edge[0], self.active_edge[1]),
                    cmp::max(self.active_edge[0], self.active_edge[1]),
                ];
                self.boundary_edges.remove(
                    self.boundary_edges.binary_search(&removed_edge).unwrap_err()
                );

                let edges = {
                    let mut out = self.faces.iter().map(|face| [
                        [face[0], face[1]],
                        [face[1], face[2]],
                        [face[0], face[2]]
                    ]).flatten().collect::<Vec<_>>();
                    out.sort();
                    out
                };

                let one_coboundary = {
                    let mut one_coboundary = vec![Vec::new(); edges.len()];
                    for (i,face) in self.faces.iter().enumerate() {
                        debug_assert!(face.is_sorted());
                        let boundary_edges = [
                            [face[0], face[1]],
                            [face[1], face[2]],
                            [face[0], face[2]]
                        ];
                        for edge in boundary_edges.iter() {
                            let edge_idx = edges.binary_search(edge).unwrap();
                            one_coboundary[edge_idx].push(i);
                        }
                    }
                    one_coboundary.iter_mut().for_each(|coboundary| coboundary.sort());
                    one_coboundary
                };

                // unpack the metadata
                let index = self.faces.len()-1 - (metadata >> 1);
                let is_right_not_left = metadata & 1 == 1;

                let merge_face = self.faces[index];
                let boundary_edges = [
                    [merge_face[0], merge_face[1]], 
                    [merge_face[1], merge_face[2]], 
                    [merge_face[0], merge_face[2]]
                ];
                debug_assert!(merge_face.is_sorted());
                let edges_of_valency_2 = boundary_edges
                    .iter()
                    .map(|edge| edges.binary_search(edge).unwrap())
                    .filter(|&edge_idx| one_coboundary[edge_idx].len() == 2)
                    .collect::<Vec<_>>();

                self.orientation = vec![false; self.faces.len()];
                let mut visited_faces = vec![false; self.faces.len()];
                let mut face_stack = vec![index];
                while let Some(face_idx) = face_stack.pop() {
                    if visited_faces[face_idx] {
                        continue;
                    }

                    if face_idx == self.faces.len()-1 {
                        break;
                    }

                    visited_faces[face_idx] = true;
                    let face = self.faces[face_idx];
                    let boundary_edges = [
                        [face[0], face[1]], 
                        [face[1], face[2]], 
                        [face[0], face[2]]
                    ];
                    let adjacent_faces = boundary_edges.iter()
                        .map(|edge| edges.binary_search(edge).unwrap())
                        .filter_map(|edge_idx| {
                            let face_indices = &one_coboundary[edge_idx];
                            if face_indices.len() == 2 {
                                let adj_face_idx = *face_indices.iter()
                                    .find(|&&idx| idx != face_idx)
                                    .unwrap();
                                Some((edge_idx, adj_face_idx))
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    for (common_edge_idx, adj_face_idx) in adjacent_faces {
                        self.orientation[adj_face_idx] = orientation_of_next_face(
                            face, 
                            self.orientation[face_idx], 
                            edges[common_edge_idx], 
                            self.faces[adj_face_idx]
                        );

                        face_stack.push(adj_face_idx);
                    }

                }

                let merge_edge = if edges_of_valency_2.len() ==2 {
                    let idx = boundary_edges.iter()
                        .map(|edge| edges.binary_search(edge).unwrap())
                        .find(|edge_idx| !edges_of_valency_2.contains(edge_idx))
                        .unwrap();
                    edges[idx]
                } else {
                    debug_assert!(edges_of_valency_2.len() == 1, "edges_of_valency_2: {:?}", edges_of_valency_2);
                    let edge_of_valency_2 = edges_of_valency_2[0];
                    let next_vertex = *self.faces[index].iter()
                        .find(|&v| !edges[edge_of_valency_2].contains(v))
                        .unwrap();
                    if is_right_not_left ^ self.orientation[index] {
                        [edges[edge_of_valency_2][1], next_vertex]
                    } else {
                        [edges[edge_of_valency_2][0], next_vertex]
                    }
                };

                // merge merge_edge[0] and 'num_decoded_vertices'
                debug_assert!(self.faces.last_mut().unwrap()[2] == self.num_decoded_vertices);
                self.faces.last_mut().unwrap()[2] = merge_edge[0];

                // merge merge_edge[1] and active_edge[0]
                let [min, max] = if merge_edge[1] < self.active_edge[0] {
                    [merge_edge[1], self.active_edge[0]]
                } else {
                    [self.active_edge[0], merge_edge[1]]
                };
                for face in &mut self.faces {
                    for vertex in face.iter_mut() {
                        if *vertex == max {
                            *vertex = min;
                        } else if *vertex > max {
                            *vertex -= 1;
                        }
                    }
                }
                if self.active_edge[1] == max {
                    self.active_edge[1] = min;
                } else if self.active_edge[1] > max {
                    self.active_edge[1] -= 1;
                }

                
                self.num_decoded_vertices -= 1;
                self.active_edge[0] = self.faces.last_mut().unwrap()[2];

                for face in &mut self.faces {
                    face.sort();
                }

                // add the new edge to the boundary edges
                let new_edge = [
                    cmp::min(self.active_edge[0], self.active_edge[1]),
                    cmp::max(self.active_edge[0], self.active_edge[1]),
                ];
                let idx = self.boundary_edges.binary_search(&new_edge).unwrap_err();
                self.boundary_edges.insert(idx, new_edge);
            }
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
}

impl ConnectivityDecoder for SpiraleReversi {
    fn decode_connectivity<F>(&mut self, reader: &mut F) -> Result<Vec<[VertexIdx; 3]>, super::Err> 
        where F: FnMut(u8) -> u64
    {
        self.init();
        let symbol_encoding = SymbolEncodingConfig::get_symbol_encoding(reader);
        self.num_connected_components = reader(NUM_CONNECTED_COMPONENTS_SLOT) as usize;

        // unwrap the symbol encoding config here so that the spirale reversi does not 
        // need to unwrap config during each iteration.
        match symbol_encoding {
            SymbolEncodingConfig::CrLight => self.spirale_reversi_impl::<CrLight, _>(reader),
            SymbolEncodingConfig::Balanced => self.spirale_reversi_impl::<Balanced, _>(reader),
            SymbolEncodingConfig::Rans => self.spirale_reversi_impl::<Rans, _>(reader),
        }


        let mut faces = Vec::new();
        mem::swap(&mut faces, &mut self.faces);
        Ok(faces)
    }
}


#[cfg(test)]
mod tests {
    use crate::core::buffer::{writer, Buffer};
    use crate::encode::connectivity::edgebreaker::Config;
    use crate::encode::connectivity::{edgebreaker, ConnectivityEncoder};
    use crate::core::shared::{
        ConfigType,
        NdVector,
        Vector
    };
    use super::*;
    use crate::decode::connectivity::ConnectivityDecoder;

    #[test]
    fn simplest() {
        let mut faces = vec![
            [0,1,2],
            [1,2,3]
        ];
        let  mut points = vec![NdVector::<3,f32>::zero(); 4];
        let mut edgebreaker = edgebreaker::Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut points, &faces).is_ok());
        let mut buff_writer = writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        assert!(edgebreaker.encode_connectivity(&mut faces, &mut points, &mut writer).is_ok());
        let buffer: Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input| buff_reader.next(input);
        let mut spirale_reversi = SpiraleReversi::new();
        let decoded_faces = spirale_reversi.decode_connectivity(&mut reader);

        assert_eq!(decoded_faces.as_ref().unwrap(), &vec![
            [0,1,2],
            [0,1,3]
        ]);
        assert_eq!(faces, &*decoded_faces.unwrap());
    }

    #[test]
    fn test_split() {
        let mut faces = vec![
            [0,1,2],
            [0,2,4],
            [0,4,5],
            [2,3,4]
        ];
        // positions do not matter
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut edgebreaker = edgebreaker::Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut points, &faces).is_ok());
        let mut buff_writer = writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        assert!(edgebreaker.encode_connectivity(&mut faces, &mut points, &mut writer).is_ok());
        let buffer: Buffer = buff_writer.into();
        let mut reader = buffer.into_reader();
        let mut reader = |input| reader.next(input);
        let mut spirale_reversi = SpiraleReversi::new();
        let decoded_faces = spirale_reversi.decode_connectivity(&mut reader);

        assert_eq!(decoded_faces.as_ref().unwrap(), &vec![
            [0,1,2], [1,3,4], [0,1,3], [0,3,5]
        ]);
        assert_eq!(faces, &*decoded_faces.unwrap());
    }

    #[test]
    fn test_disc() {
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

        let mut edgebreaker = edgebreaker::Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut points, &faces).is_ok());
        let mut buff_writer = writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        assert!(edgebreaker.encode_connectivity(&mut faces, &mut points, &mut writer).is_ok());
        let buffer: Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input| buff_reader.next(input);
        let mut spirale_reversi = SpiraleReversi::new();
        let decoded_faces = spirale_reversi.decode_connectivity(&mut reader);

        assert_eq!(decoded_faces.as_ref().unwrap(), &vec![
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
        ]);
        assert_eq!(faces, &*decoded_faces.unwrap());
    }

    #[test]
    fn test_long_split() {
        let mut faces = vec![
            [0,1,2],
            [0,2,3],
            [0,3,4],
            [1,2,6],
            [1,5,6]
        ];
        // positions do not matter
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut edgebreaker = edgebreaker::Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut points, &faces).is_ok());
        let mut buff_writer = writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        assert!(edgebreaker.encode_connectivity(&mut faces, &mut points, &mut writer).is_ok());
        let buffer: Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input| buff_reader.next(input);
        let mut spirale_reversi = SpiraleReversi::new();
        let decoded_faces = spirale_reversi.decode_connectivity(&mut reader);

        assert_eq!(decoded_faces.as_ref().unwrap(), &vec![
            [0,1,2], 
            [0,1,3], 
            [4,5,6], 
            [3,4,5], 
            [0,3,5]
        ]);
        assert_eq!(faces, &*decoded_faces.unwrap());
    }

    #[test]
    fn test_hole() {
        let mut faces = [
            [9,23,24], [8,9,23], [8,9,10], [1,8,10], [1,10,11], [1,2,11], [2,11,12], [2,12,13],
            [8,22,23], [7,8,22], [1,7,8], [0,1,7], [0,1,2], [0,2,3], [2,3,13], [3,13,14],
            [7,21,22], [6,7,21], [0,6,7], [0,5,6], [0,3,5], [3,4,5], [3,4,14], [4,14,15],
            [6,20,21], [6,19,20], [5,6,19], [5,18,19], [4,5,18], [4,17,18], [4,15,17], [15,16,17]
        ];
        faces.sort();

        // positions do not matter
        let mut points = vec![NdVector::<3,f32>::zero(); faces.iter().flatten().max().unwrap()+1];

        let mut edgebreaker = edgebreaker::Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut points, &faces).is_ok());
        let mut buff_writer = writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        assert!(edgebreaker.encode_connectivity(&mut faces, &mut points, &mut writer).is_ok());
        let buffer: Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input| buff_reader.next(input);
        let mut spirale_reversi = SpiraleReversi::new();
        let decoded_faces = spirale_reversi.decode_connectivity(&mut reader);

        assert_eq!(decoded_faces.as_ref().unwrap(), 
            &vec![
                [0,1,2], [3,4,5], [4,6,7], [3,4,6], [3,6,8], [3,8,9], [8,9,10], [9,10,11], 
                [10,11,12], [11,12,13], [1,11,13], [1,13,14], [0,1,14], [0,14,15], [15,16,17], [0,15,16], 
                [0,16,18], [0,2,18], [2,18,19], [20,21,22], [19,20,21], [2,19,21], [2,21,23], [1,2,23],
                [1,11,23], [9,11,23], [9,23,24], [3,9,24], [3,5,24], [5,22,24], [21,22,24], [21,23,24]
            ]
        );
        // assert_eq!(faces, &*decoded_faces);
    }

    // #[test]
    fn test_handle() {
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

        let mut edgebreaker = edgebreaker::Edgebreaker::new(Config::default());
        assert!(edgebreaker.init(&mut points, &faces).is_ok());
        let mut buff_writer = writer::Writer::new();
        let mut writer = |input| buff_writer.next(input);
        assert!(edgebreaker.encode_connectivity(&mut faces, &mut points, &mut writer).is_ok());
        let buffer: Buffer = buff_writer.into();
        let mut buff_reader = buffer.into_reader();
        let mut reader = |input| buff_reader.next(input);
        let mut spirale_reversi = SpiraleReversi::new();
        let decoded_faces = spirale_reversi.decode_connectivity(&mut reader);

        assert_eq!(decoded_faces.as_ref().unwrap(), &vec![
            [0,1,2], [1,3,4], [0,1,3], [0,3,5], [2,6,7], [4,7,8], [6,7,8], [5,6,8], 
            [5,8,9], [0,5,9], [0,9,10], [0,2,10], [2,7,10], [7,10,11], [4,7,11], [3,4,11], 
            [3,11,12], [3,5,12], [5,6,12], [6,12,13], [2,6,13], [1,2,13], [1,13,14], [1,4,14], 
            [4,8,14], [8,9,14], [9,14,15], [9,10,15], [10,11,15], [11,12,15], [12,13,15], [13,14,15]
        ]);
        assert_eq!(faces, &*decoded_faces.unwrap());
    }
}
