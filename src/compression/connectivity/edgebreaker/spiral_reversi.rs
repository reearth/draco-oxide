use crate::compression::connectivity::ConnectivityDecoder;
use crate::core::buffer::reader::Reader;
use crate::core::shared::VertexIdx;

use super::symbol_encoder::{
    SymbolEncodingConf,
    Symbol,
    SymbolEncoder  
};

use std::mem;


struct SpiralReversi {
    faces: Vec<[VertexIdx; 3]>,
    num_faces: Vec<usize>,
    num_decoded_vertices: usize,
    active_edge: [usize; 2],
    active_edge_stack: Vec<[usize; 2]>,
    orientation_of_curr_vertex: bool,
}

impl SpiralReversi {
    fn spiral_reversi() -> Self {
        Self {
            faces: vec![],
            num_faces: Vec::new(),
            num_decoded_vertices: 0,
            active_edge: [0,1],
            active_edge_stack: Vec::new(),
            orientation_of_curr_vertex: true,
        }
    }

    fn spirale_reversi_impl<SE: SymbolEncoder>(&mut self, reader: &mut Reader) {
        // move the value in order to avoid the borrow checker.
        let mut num_faces = Vec::new();
        mem::swap(&mut num_faces, &mut self.num_faces);
        for num_faces in num_faces {
            self.num_decoded_vertices += 2;
            self.active_edge_stack.clear();
            self.active_edge = [
                self.num_decoded_vertices-2,
                self.num_decoded_vertices-1
            ];
            self.orientation_of_curr_vertex = true;
            for _ in 0..num_faces {
                let mut i = 0;
                while i < num_faces {
                    self.spirale_reversi_recc::<SE>(reader);
                }
            }
        }
    }

    #[inline]
    fn spirale_reversi_recc<SE: SymbolEncoder>(&mut self, reader: &mut Reader) {
        match SE::decode_symbol(reader) {
            Symbol::C => {
                let mut new_face = [
                    self.active_edge[0],
                    self.active_edge[1],
                    self.num_decoded_vertices
                ];
                self.faces.push(new_face);
                self.num_decoded_vertices += 1;
                self.active_edge = [self.active_edge[1], self.num_decoded_vertices];
            },
            Symbol::R => {
                let mut new_face = [
                    self.active_edge[0],
                    self.active_edge[1],
                    self.num_decoded_vertices
                ];
                self.faces.push(new_face);
                self.num_decoded_vertices += 1;
                self.active_edge = [self.active_edge[1], self.num_decoded_vertices];
            },
            Symbol::L => {
                let mut new_face = [
                    self.active_edge[0],
                    self.active_edge[1],
                    self.num_decoded_vertices
                ];
                self.faces.push(new_face);
                self.num_decoded_vertices += 1;
                self.active_edge = [self.active_edge[1], self.num_decoded_vertices];
            },
            Symbol::E => {
                let mut new_face = [
                    self.active_edge[0],
                    self.active_edge[1],
                    self.num_decoded_vertices
                ];
                self.faces.push(new_face);
                self.num_decoded_vertices += 1;
                self.active_edge = [self.active_edge[1], self.num_decoded_vertices];
            },
            Symbol::S => {
                let mut new_face = [
                    self.active_edge[0],
                    self.active_edge[1],
                    self.num_decoded_vertices
                ];
                self.faces.push(new_face);
                self.num_decoded_vertices += 1;
                self.active_edge = [self.active_edge[1], self.num_decoded_vertices];
            },
            Symbol::M => {
                let mut new_face = [
                    self.active_edge[0],
                    self.active_edge[1],
                    self.num_decoded_vertices
                ];
                self.faces.push(new_face);
                self.num_decoded_vertices += 1;
                self.active_edge = [self.active_edge[1], self.num_decoded_vertices];
            }
        }
    }
}

impl ConnectivityDecoder for SpiralReversi {
    fn decode_connectivity(mut reader: Reader) -> Vec<[VertexIdx; 3]> {
        let mut faces = vec![];
        let mut i = 0;
        let symbol_encoding = SymbolEncodingConf::get_symbol_encoding(&mut reader);
        let num_connected_components = reader.next(8);
        let mut num_faces = Vec::new();
        for _ in 0..num_connected_components {
            num_faces.push(reader.next(8) as usize);
        }
        let mut num_decoded_vertices = 0;


        faces
    }
}