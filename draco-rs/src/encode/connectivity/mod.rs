pub mod config;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use core::panic;
use std::fmt::Debug;

use crate::core::bit_coder::ByteWriter;
use crate::core::shared::{
    ConfigType,
    VertexIdx,
};
use crate::core::attribute::AttributeType;
use crate::core::mesh::Mesh;
use crate::debug_write;
use crate::prelude::Attribute;
use crate::shared::connectivity::{EdgebreakerDecoder, Encoder, NUM_CONNECTIVITY_ATTRIBUTES_SLOT};

#[cfg(feature = "evaluation")]
use crate::eval;

/// entry point for encoding connectivity.
pub fn encode_connectivity<W>(
    mesh:&mut Mesh,
    writer: &mut W,
    cfg: Config,
) -> Result<(), Err>
    where W: ByteWriter
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("connectivity info", writer);

    // create the array of tuples (connectivity attribute (parent), position attribute (child) index)
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
    writer.write_u8(conn_child_atts_indices.len() as u8);

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

pub fn encode_connectivity_datatype_unpacked<W>(
    faces: &mut [[VertexIdx; 3]],
    children: &mut[&mut Attribute],
    writer: &mut W,
    cfg: Config,
) -> Result<(), Err>
    where W: ByteWriter,
{
    match cfg {
        Config::Edgebreaker(cfg) => {
            #[cfg(feature = "evaluation")]
            eval::scope_begin("edgebreaker", writer);
            
            // write the encoder id
            writer.write_u8(Encoder::Edgebreaker.get_id() as u8);
            debug_write!("Start of edgebreaker connectivity", writer);
            // write the edgebreaker decoder id
            writer.write_u8(EdgebreakerDecoder::SpiraleReversi.id() as u8);
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
            writer.write_u8(Encoder::Sequential.get_id() as u8);
            let mut encoder = sequential::Sequential::new(cfg);
            debug_write!("Start of sequential connectivity", writer);
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
    fn encode_connectivity<W>(
        &mut self, 
        faces: &mut [[VertexIdx; 3]],
        points: &mut[&mut Attribute], 
        buffer: &mut W
    ) -> Result<(), Self::Err>
        where W: ByteWriter;
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