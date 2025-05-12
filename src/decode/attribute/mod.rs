pub(crate) mod attribute_decoder;
pub(crate) mod inverse_prediction_transform;
pub(crate) mod portabilization;

use thiserror::Error;
use crate::prelude::{Attribute, ConfigType};


#[derive(Debug, Error)]
pub enum Err {
    #[error("Attribute error: {0}")]
    AttributeError(#[from] attribute_decoder::Err),
    #[error("Prediction inverse transform error: {0}")]
    PredictionInverseTransformError(String),
}

#[derive(Debug, Clone)]
pub struct Config {
    decoder_cfgs: Vec<attribute_decoder::Config>,
}

impl ConfigType for Config {
    fn default() -> Self {
        Self {
            decoder_cfgs: vec![attribute_decoder::Config::default()],
        }
    }
}

pub fn decode_attributes<F>(
    stream_in: &mut F,
    cfg: Config,
    mut decoded_attributes: Vec<Attribute>,
) -> Result<Vec<Attribute>, Err>
    where F: FnMut(u8) -> u64,
{
    // Read the number of attributes
    let num_attributes = stream_in(16);

    let mut cfg = cfg.decoder_cfgs.into_iter();
    for _ in 0..num_attributes {
        let decoder = attribute_decoder::AttributeDecoder::new_and_init(
            cfg.next().unwrap(),
            stream_in,
            &decoded_attributes,
        )?;
        let att = decoder.decode().map_err(|err| {
            Err::AttributeError(err)
        })?;
        decoded_attributes.push(att);
    }
    Ok(decoded_attributes)
}