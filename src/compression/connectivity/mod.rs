mod sequential;
mod edgebreaker;

use crate::core::buffer::writer::Writer;
use crate::core::buffer::MSB_FIRST;
use crate::core::shared::VertexIdx;

pub trait ConnectivityEncoder {
    type Err;
    type Config;
    fn encode_connectivity<CoordValType>(&mut self, faces: &[[VertexIdx; 3]], config: &Self::Config, points: &mut [[CoordValType; 3]], buffer: &mut Writer<MSB_FIRST>) -> Result<(), Self::Err>;
}

pub trait ConnectivityDecoder {
    fn decode_connectivity(reader: crate::core::buffer::reader::Reader) -> Vec<[VertexIdx; 3]>; 
}