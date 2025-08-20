pub mod config;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use std::fmt::Debug;

use crate::core::bit_coder::ByteWriter;
use crate::core::shared::{ConfigType, PointIdx};
use crate::encode::connectivity::edgebreaker::{DefaultTraversal, ValenceTraversal};
use crate::prelude::{Attribute, AttributeType};
use crate::shared::connectivity::edgebreaker::EdgebreakerKind;

#[cfg(feature = "evaluation")]
use crate::eval;

/// entry point for encoding connectivity.
pub fn encode_connectivity<'faces, W>(
    faces: &'faces[[PointIdx; 3]],
    atts: &mut [Attribute],
    writer: &mut W,
    #[allow(unused)] // This parameter is unused in the current implementation, as we only support default configuration.
    cfg: &super::Config,
) -> Result<ConnectivityEncoderOutput<'faces>, Err>
    where W: ByteWriter
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("connectivity info", writer);

    let result = encode_connectivity_datatype_unpacked(faces, atts, writer, Config::default());

    #[cfg(feature = "evaluation")]
    eval::scope_end(writer);
    result
}

pub fn encode_connectivity_datatype_unpacked<'faces, W>(
    faces: &'faces[[PointIdx; 3]],
    atts: &mut [Attribute],
    writer: &mut W,
    cfg: Config,
) -> Result<ConnectivityEncoderOutput<'faces>, Err>
    where W: ByteWriter,
{
    let result = match cfg {
        Config::Edgebreaker(cfg) => {
            #[cfg(feature = "evaluation")]
            eval::scope_begin("edgebreaker", writer);
            
            let result = match cfg.traversal {
                EdgebreakerKind::Standard => {
                    let encoder = edgebreaker::Edgebreaker::<DefaultTraversal>::new(cfg, atts, faces)?;
                    encoder.encode_connectivity(&faces, writer)
                },
                EdgebreakerKind::Predictive => {
                    unimplemented!("Predictive edgebreaker encoding is not implemented yet");
                },
                EdgebreakerKind::Valence => {
                    let encoder = edgebreaker::Edgebreaker::<ValenceTraversal>::new(cfg, atts, faces)?;
                    encoder.encode_connectivity(&faces, writer)
                },
            };
            
            #[cfg(feature = "evaluation")]
            eval::scope_end(writer);

            result.map(|o| ConnectivityEncoderOutput::Edgebreaker(o))?
        },
        Config::Sequential(cfg) => {
            #[cfg(feature = "evaluation")]
            eval::scope_begin("sequential", writer);

            let num_points = atts.iter()
                .find(|att| att.get_attribute_type() == AttributeType::Position)
                .unwrap()
                .len();
            let encoder = sequential::Sequential::new(cfg, num_points);
            let result = encoder.encode_connectivity(faces, writer)?;

            #[cfg(feature = "evaluation")]
            eval::scope_end(writer);
            
            ConnectivityEncoderOutput::Sequential(result)
        }
    };
    Ok(result)
}

pub trait ConnectivityEncoder {
    type Err;
    type Config;
    type Output;
    fn encode_connectivity<W>(
        self, 
        faces: &[[PointIdx; 3]],
        buffer: &mut W
    ) -> Result<Self::Output, Self::Err>
        where W: ByteWriter;
}

pub(crate) enum ConnectivityEncoderOutput<'faces> {
    Edgebreaker(edgebreaker::Output<'faces>),
    Sequential(()),
}

#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Edgebreaker encoding error: {0}")]
    EdgebreakerError(#[from] edgebreaker::Err),
    #[error("Position attribute must be of type f32 or f64")]
    PositionAttributeTypeError,
    #[error("Sequential encoding error: {0}")]
    SequentialError(#[from] sequential::Err),
    #[error("Too many connectivity attributes")]
    TooManyConnectivityAttributes,
}

#[remain::sorted]
#[derive(Clone, Debug)]
pub enum Config {
    Edgebreaker(edgebreaker::Config),
    #[allow(unused)] // we currently support only edgebreaker
    Sequential(sequential::Config),
}

impl ConfigType for Config {
    fn default()-> Self {
        Self::Edgebreaker(edgebreaker::Config::default())
    }
}