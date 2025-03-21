mod spirale_reversi;
use crate::core::shared::VertexIdx;
pub trait ConnectivityDecoder {
    fn decode_connectivity(&mut self, reader: crate::core::buffer::reader::Reader) -> Vec<[VertexIdx; 3]>; 
}