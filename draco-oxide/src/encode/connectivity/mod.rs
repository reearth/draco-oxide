pub mod config;
pub(crate) mod edgebreaker;
pub(crate) mod sequential;

use std::fmt::Debug;

use crate::encode::connectivity::edgebreaker::{DefaultTraversal, ValenceTraversal};
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::connectivity::edgebreaker::EdgebreakerKind;
use draco_oxide_core::mesh::ds::AttributeDS;
use draco_oxide_core::types::{ConfigType, CornerIdx};

/// Entry point for encoding connectivity. Encodes the mesh connectivity with
/// the method selected in the configuration and returns the corner order of
/// the edgebreaker traversal (empty for sequential encoding).
pub fn encode_connectivity<'faces, W>(
    adss: &mut [AttributeDS<'faces>],
    writer: &mut W,
    cfg: &super::Config,
) -> Result<Vec<CornerIdx>, Err>
where
    W: ByteWriter,
{
    encode_connectivity_datatype_unpacked(adss, writer, cfg.connectivity.clone())
}

/// Dispatches connectivity encoding to the edgebreaker or sequential encoder
/// according to the given connectivity configuration.
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

            result
        }
        Config::Sequential(cfg) => {
            // Sequential attributes are stored per point, so the point space is
            // what the face indices address and what sizes them.
            let num_points = adss[0].global_ds().num_points();
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

/// Interface implemented by the connectivity encoders. Consumes the encoder,
/// writes the encoded connectivity to the writer, and returns the corner
/// order of the traversal.
pub trait ConnectivityEncoder {
    type Err;
    type Config;
    fn encode_connectivity<W>(self, writer: &mut W) -> Result<Vec<CornerIdx>, Self::Err>
    where
        W: ByteWriter;
}

/// Errors from connectivity encoding.
#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    /// Edgebreaker encoding failed.
    #[error("Edgebreaker encoding error: {0}")]
    EdgebreakerError(#[from] edgebreaker::Err),
    /// The position attribute has an unsupported component type.
    #[error("Position attribute must be of type f32 or f64")]
    PositionAttributeTypeError,
    /// Sequential encoding failed.
    #[error("Sequential encoding error: {0}")]
    SequentialError(#[from] sequential::Err),
    /// The mesh has more connectivity attributes than the encoder supports.
    #[error("Too many connectivity attributes")]
    TooManyConnectivityAttributes,
}

/// Selection of the connectivity encoding method, carrying the configuration
/// of the selected method. Exported as `ConnectivityConfig`.
#[remain::sorted]
#[derive(Clone, Debug)]
pub enum Config {
    /// Edgebreaker connectivity encoding.
    Edgebreaker(edgebreaker::Config),
    /// Sequential connectivity encoding, which stores face indices directly
    /// without compressing the connectivity.
    Sequential(sequential::Config),
}

impl ConfigType for Config {
    fn default() -> Self {
        Self::Edgebreaker(edgebreaker::Config::default())
    }
}

impl Config {
    /// The wire-level connectivity method this config selects, as written into
    /// the Draco header and used to branch attribute sequencing.
    pub fn encoder_method(&self) -> draco_oxide_core::codec::header::EncoderMethod {
        use draco_oxide_core::codec::header::EncoderMethod;
        match self {
            Config::Edgebreaker(_) => EncoderMethod::Edgebreaker,
            Config::Sequential(_) => EncoderMethod::Sequential,
        }
    }
}
