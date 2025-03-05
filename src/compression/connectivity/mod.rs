mod sequential;
mod edgebreaker;

use crate::core::buffer::writer::Writer;
use crate::core::shared::VertexIdx;

pub trait ConnectivityEncoder {
    type Err;
    type Config;
    fn encode_connectivity<CoordValType>(&mut self, faces: &[[VertexIdx; 3]], config: &Self::Config, points: &mut [[CoordValType; 3]], buffer: &mut Writer) -> Result<(), Self::Err>;
}