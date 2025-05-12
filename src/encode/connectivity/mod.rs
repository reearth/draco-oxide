pub mod config;
pub mod err;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use std::fmt::Debug;

use crate::core::shared::{
    ConfigType,
    VertexIdx, 
    NdVector,
};
use crate::core::attribute::{
    AttributeType,
    ComponentDataType
};
use crate::core::mesh::Mesh;
use crate::shared::connectivity::{EdgebreakerDecoder, Encoder, NUM_CONNECTIVITY_ATTRIBUTES_SLOT};

/// entry point for encoding connectivity.
pub fn encode_connectivity<F>(
    mesh:&mut Mesh,
    writer: &mut F,
    cfg: Config,
) -> Result<(), Err>
    where F: FnMut((u8, u64))
{
    // create the array of tuples (connectivity attribute (parent), position attribute (chilc) index)
    let conn_pos_atts_indices = mesh.get_attributes_mut()
        .iter_mut()
        .enumerate()
        .filter(|(_, att)| att.get_attribute_type() == AttributeType::Position)
        .map(|(idx, att)| 
            {
                debug_assert!(att.get_parents().len() <= 1, "Internal error: position attribute has more than one parent");
                // ToDo: Consider to remove the following condition.
                assert!(att.get_parents().len() == 1, "Currently we do not support connectivity attributes without a child.");
                (att.get_parents()[0], idx)
            }
        )
        .collect::<Vec<_>>();

    // write the number of connectivity attributes
    if conn_pos_atts_indices.len() >= 1 << NUM_CONNECTIVITY_ATTRIBUTES_SLOT {
        return Err(Err::TooManyConnectivityAttributes);
    }
    writer((NUM_CONNECTIVITY_ATTRIBUTES_SLOT, conn_pos_atts_indices.len() as u64));

    for (conn_att_idx, pos_att_idx) in conn_pos_atts_indices {
        let conn_att_idx = conn_att_idx.as_usize();
        let (conn_att, pos_att) = {
            let (first, last) = mesh.get_attributes_mut().split_at_mut(pos_att_idx);
            (&mut first[conn_att_idx], &mut last[0])
        };

        debug_assert!(
            conn_att.get_num_components() == 3, 
            "Position attributes must have 3 components"
        );
        let faces = unsafe{ 
            conn_att.as_slice_unchecked_mut::<[VertexIdx; 3]>()
        };
            
        match pos_att.get_component_type() {
            ComponentDataType::F32 => {
                // Safety: Checked that the number of components is 3 and the type is f32
                let points = unsafe{ 
                    pos_att.as_slice_unchecked_mut::<NdVector<3,f32>>()
                };
                encode_connectivity_datatype_unpacked(faces, points, writer, cfg.clone())?
            }
            ComponentDataType::F64 => {
                // Safety: Checked that the number of components is 3 and the type is f64
                let points = unsafe{ 
                    pos_att.as_slice_unchecked_mut::<NdVector<3, f64>>()
                };
                encode_connectivity_datatype_unpacked(faces, points, writer, cfg.clone())?
            }
            _ => return Err(Err::PositionAttributeTypeError),
        };
    }

    Ok(())
}

pub fn encode_connectivity_datatype_unpacked<CoordValType, F>(
    faces: &mut [[VertexIdx; 3]],
    points: &mut [NdVector<3, CoordValType>],
    writer: &mut F,
    cfg: Config,
) -> Result<(), Err>
where
    CoordValType: Copy + Debug,
    F: FnMut((u8, u64)),
{
    match cfg {
        Config::Edgebreaker(cfg) => {
            // write the encoder id
            writer((1,Encoder::Edgebreaker.id()));
            // write the edgebreaker decoder id
            writer((3, EdgebreakerDecoder::SpiraleReversi.id()));
            let mut encoder = edgebreaker::Edgebreaker::new(cfg);
            let result = encoder.encode_connectivity(faces, points, writer);
            if let Err(err) = result {
                return Err(Err::EdgebreakerError(err));
            }
        },
        Config::Sequential(cfg) => {
            // write the encoder id
            writer((1,Encoder::Sequential.id()));
            let mut encoder = sequential::Sequential::new(cfg);
            let result = encoder.encode_connectivity(faces, points, writer);
            if let Err(err) = result {
                return Err(Err::SequentialError(err));
            }
        }
    };

    Ok(())
}

pub trait ConnectivityEncoder {
    type Err;
    type Config;
    fn encode_connectivity<CoordValType: Copy + Debug, F>(
        &mut self, 
        faces: &mut [[VertexIdx; 3]],
        points: &mut [NdVector<3, CoordValType>], 
        buffer: &mut F
    ) -> Result<(), Self::Err>
        where
            F: FnMut((u8, u64));
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Edgebreaker encoding error")]
    EdgebreakerError(edgebreaker::Err),
    #[error("Position attribute must be of type f32 or f64")]
    PositionAttributeTypeError,
    #[error("Sequential encoding error")]
    SequentialError(sequential::Err),
    #[error("Too many connectivity attributes")]
    TooManyConnectivityAttributes,
}

#[remain::sorted]
#[derive(Clone)]
pub enum Config {
    Edgebreaker(edgebreaker::Config),
    Sequential(sequential::Config),
}

impl ConfigType for Config {
    fn default()-> Self {
        Self::Edgebreaker(edgebreaker::Config::default())
    }
}