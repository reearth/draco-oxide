pub mod config;
pub mod err;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use crate::core::buffer::writer::Writer;
use crate::core::buffer::MsbFirst;
use crate::core::shared::VertexIdx;

pub trait ConnectivityEncoder {
    type Err;
    type Config;
    fn encode_connectivity<CoordValType>(
        &mut self, 
        faces: &mut [[VertexIdx; 3]],
        config: &Self::Config, 
        points: &mut [[CoordValType; 3]], 
        buffer: &mut Writer<MsbFirst>
    ) -> Result<(), Self::Err>;
}