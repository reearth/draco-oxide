use crate::core::shared::VertexIdx;
use crate::core::shared::ConfigType;
use crate::encode::connectivity::ConnectivityEncoder;
use crate::prelude::Attribute;
use crate::shared::connectivity::sequential::index_size_from_vertex_count;
use crate::shared::connectivity::sequential::{
    NUM_POINTS_SLOT,
    NUM_FACES_SLOT
};

pub(crate) struct Sequential;

impl Sequential {
    pub fn new(_config: Config) -> Self {
        Self
    }
}

impl ConnectivityEncoder for Sequential {
    type Err = Err;
    type Config = Config;

    fn encode_connectivity<F>(
        &mut self, 
        faces: &mut [[VertexIdx; 3]],
        points: &mut[&mut Attribute], 
        writer: &mut F
    ) -> Result<(), Err> 
        where  F: FnMut((u8, u64)),
    {
        let index_size = match index_size_from_vertex_count(points.len()) {
            Ok(index_size) => index_size as u8,
            Err(err) => return Err(Err::SharedError(err)),
        };
        

        writer((NUM_POINTS_SLOT, points.len() as u64));
        writer((NUM_FACES_SLOT, faces.len() as u64));
        

        for face in faces {
            writer((index_size, face[0] as u64));
            writer((index_size, face[1] as u64));
            writer((index_size, face[2] as u64));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Config;

impl ConfigType for Config {
    fn default() -> Self {
        Config
    }
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Invalid vertex count")]
    SharedError(crate::shared::connectivity::sequential::Err),
}
