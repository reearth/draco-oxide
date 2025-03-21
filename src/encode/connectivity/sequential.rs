use crate::core::buffer::writer::Writer;
use crate::core::buffer::MsbFirst;
use crate::core::shared::VertexIdx;
use crate::core::shared::ConfigType;
use crate::encode::connectivity::ConnectivityEncoder;
use crate::shared::connectivity::sequential::index_size_from_vertex_count;
use crate::shared::connectivity::sequential::{
    NUM_POINTS_SLOT,
    NUM_FACES_SLOT
};

pub(crate) struct Sequential;

impl ConnectivityEncoder for Sequential {
    type Err = Err;
    type Config = Config;

    fn encode_connectivity<CoordValType>(
        &mut self, faces: &[[VertexIdx; 3]], 
        _: &Self::Config, 
        points: &mut [[CoordValType; 3]], 
        buffer: &mut Writer<MsbFirst>
    ) -> Result<(), Err> {
        let index_size = match index_size_from_vertex_count(points.len()) {
            Ok(index_size) => index_size,
            Err(err) => return Err(Err::SharedError(err)),
        };
        

        buffer.next((NUM_POINTS_SLOT, points.len()));
        buffer.next((NUM_FACES_SLOT, faces.len()));
        

        for face in faces {
            buffer.next((index_size, face[0]));
            buffer.next((index_size, face[1]));
            buffer.next((index_size, face[2]));
        }
        Ok(())
    }
}

pub struct Config;

impl ConfigType for Config {
    fn default() -> Self {
        Config
    }
}

pub enum Err {
    SharedError(crate::shared::connectivity::sequential::Err),
}
