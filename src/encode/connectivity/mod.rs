pub mod config;
pub mod err;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use crate::core::buffer::writer::Writer;
use crate::core::buffer::MsbFirst;
use crate::core::shared::VertexIdx;
use crate::core::shared::NdVector;

pub trait ConnectivityEncoder {
    type Err;
    type Config;
    fn encode_connectivity<CoordValType: Copy>(
        &mut self, 
        faces: &mut [[VertexIdx; 3]],
        config: &Self::Config, 
        points: &mut [NdVector<3, CoordValType>], 
        buffer: &mut Writer<MsbFirst>
    ) -> Result<(), Self::Err>;
}