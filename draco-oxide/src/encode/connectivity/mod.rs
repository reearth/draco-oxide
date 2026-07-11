pub mod config;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use std::fmt::Debug;

use crate::encode::connectivity::edgebreaker::{DefaultTraversal, ValenceTraversal};
use draco_oxide_core::attribute::AttributeType;
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::connectivity::edgebreaker::EdgebreakerKind;
use draco_oxide_core::mesh::ds::AttributeDS;
use draco_oxide_core::types::{ConfigType, CornerIdx};

#[cfg(feature = "evaluation")]
use crate::eval;

/// entry point for encoding connectivity.
pub fn encode_connectivity<'faces, W>(
    adss: &mut [AttributeDS<'faces>],
    writer: &mut W,
    #[allow(unused)]
    // This parameter is unused in the current implementation, as we only support default configuration.
    cfg: &super::Config,
) -> Result<Vec<CornerIdx>, Err>
where
    W: ByteWriter,
{
    #[cfg(feature = "evaluation")]
    eval::scope_begin("connectivity info", writer);

    let result = encode_connectivity_datatype_unpacked(adss, writer, Config::default());

    #[cfg(feature = "evaluation")]
    eval::scope_end(writer);
    result
}

pub fn encode_connectivity_datatype_unpacked<'faces, W>(
    adss: &mut [AttributeDS<'faces>],
    writer: &mut W,
    cfg: Config,
) -> Result<Vec<CornerIdx>, Err>
where
    W: ByteWriter,
{
    let corners_of_edgebreaker = match cfg {
        Config::Edgebreaker(cfg) => {
            #[cfg(feature = "evaluation")]
            eval::scope_begin("edgebreaker", writer);

            let result = match cfg.traversal {
                EdgebreakerKind::Standard => {
                    let encoder =
                        edgebreaker::Edgebreaker::new(cfg, adss, |_| DefaultTraversal::new())?;
                    encoder.encode_connectivity(writer)?
                }
                EdgebreakerKind::Predictive => {
                    unimplemented!("Predictive edgebreaker encoding is not implemented yet");
                }
                EdgebreakerKind::Valence => {
                    let encoder = edgebreaker::Edgebreaker::new(cfg, adss, ValenceTraversal::new)?;
                    encoder.encode_connectivity(writer)?
                }
            };

            #[cfg(feature = "evaluation")]
            eval::scope_end(writer);

            result
        }
        Config::Sequential(cfg) => {
            #[cfg(feature = "evaluation")]
            eval::scope_begin("sequential", writer);

            let num_points = adss
                .iter()
                .find(|ads| ads.att_data().get_attribute_type() == AttributeType::Position)
                .unwrap()
                .att_data()
                .len();
            let faces = (0..adss[0].global_ds().num_faces())
                .map(|i| {
                    let c = CornerIdx::from(3 * i);
                    [
                        adss[0].global_ds().point_idx(c),
                        adss[0].global_ds().point_idx(c.next()),
                        adss[0].global_ds().point_idx(c.next().next()),
                    ]
                })
                .collect::<Vec<_>>();
            let encoder = sequential::Sequential::new(&faces, cfg, num_points);
            // Sequential encoding does not produce an edgebreaker traversal ordering.
            encoder.encode_connectivity(writer)?
        }
    };
    Ok(corners_of_edgebreaker)
}

pub trait ConnectivityEncoder {
    type Err;
    type Config;
    fn encode_connectivity<W>(self, writer: &mut W) -> Result<Vec<CornerIdx>, Self::Err>
    where
        W: ByteWriter;
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
    fn default() -> Self {
        Self::Edgebreaker(edgebreaker::Config::default())
    }
}
