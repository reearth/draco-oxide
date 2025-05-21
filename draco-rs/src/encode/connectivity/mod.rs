pub mod config;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use std::fmt::Debug;

use crate::core::shared::{
    ConfigType,
    VertexIdx,
};
use crate::core::attribute::AttributeType;
use crate::core::mesh::Mesh;
use crate::prelude::Attribute;
use crate::shared::connectivity::{EdgebreakerDecoder, Encoder, NUM_CONNECTIVITY_ATTRIBUTES_SLOT};

#[cfg(feature = "evaluation")]
use crate::eval;

/// entry point for encoding connectivity.
pub fn encode_connectivity<F>(
    mesh:&mut Mesh,
    writer: &mut F,
    cfg: Config,
) -> Result<(), Err>
    where F: FnMut((u8, u64))
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("connectivity info", writer);

    // create the array of tuples (connectivity attribute (parent), position attribute (chilc) index)
    let conn_child_atts_indices = mesh.get_attributes()
        .iter()
        .enumerate()
        .filter(|(_, att)| att.get_attribute_type() == AttributeType::Connectivity)
        .map(|(idx, conn_att)| {
            // look for attributes that has 'conn_att' as parent
            let children = mesh.get_attributes()
                .iter()
                .enumerate()
                .filter(|(_, att)| att.get_parents().contains(&conn_att.get_id()))
                .map(|(child_idx, _)| child_idx)
                .collect::<Vec<_>>();
            let mut out = vec![idx];
            out.extend(children);
            out
        })
        .collect::<Vec<_>>();

    // write the number of connectivity attributes
    if conn_child_atts_indices.len() >= 1 << NUM_CONNECTIVITY_ATTRIBUTES_SLOT {
        return Err(Err::TooManyConnectivityAttributes);
    }
    writer((NUM_CONNECTIVITY_ATTRIBUTES_SLOT, conn_child_atts_indices.len() as u64));

    for idx in conn_child_atts_indices {
        let mut atts = mesh.get_attributes_mut_by_indices(&idx);
        let (conn_att, children) = atts.split_at_mut(1);
        
        // Safety: we know that the first attribute is a connectivity attribute,
        // and the connectivity attribute is a 3D array of VertexIdx
        let faces = unsafe{ 
            conn_att[0].as_slice_unchecked_mut::<[VertexIdx; 3]>()
        };

        encode_connectivity_datatype_unpacked(faces, children, writer, Config::default())?;

    }

    #[cfg(feature = "evaluation")]
    eval::scope_end(writer);
    Ok(())
}

pub fn encode_connectivity_datatype_unpacked<F>(
    faces: &mut [[VertexIdx; 3]],
    children: &mut[&mut Attribute],
    writer: &mut F,
    cfg: Config,
) -> Result<(), Err>
where
    F: FnMut((u8, u64)),
{
    match cfg {
        Config::Edgebreaker(cfg) => {
            #[cfg(feature = "evaluation")]
            eval::scope_begin("edgebreaker", writer);
            
            // write the encoder id
            writer((1,Encoder::Edgebreaker.id()));
            // write the edgebreaker decoder id
            writer((3, EdgebreakerDecoder::SpiraleReversi.id()));
            let mut encoder = edgebreaker::Edgebreaker::new(cfg);
            let result = encoder.encode_connectivity(faces, children, writer);
            
            #[cfg(feature = "evaluation")]
            eval::scope_end(writer);

            if let Err(err) = result {
                return Err(Err::EdgebreakerError(err));
            }
        },
        Config::Sequential(cfg) => {
            #[cfg(feature = "evaluation")]
            eval::scope_begin("sequential", writer);

            // write the encoder id
            writer((1,Encoder::Sequential.id()));
            let mut encoder = sequential::Sequential::new(cfg);
            let result = encoder.encode_connectivity(faces, children, writer);
            
            #[cfg(feature = "evaluation")]
            eval::scope_end(writer);
            
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
    fn encode_connectivity<F>(
        &mut self, 
        faces: &mut [[VertexIdx; 3]],
        points: &mut[&mut Attribute], 
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