mod sequential;
mod spirale_reversi;
use crate::core::bit_coder::ReaderErr;
use crate::core::shared::{FaceIdx, VertexIdx}; 
use crate::debug_expect;
use crate::decode::header::Header;
use crate::prelude::ByteReader;
use crate::shared::connectivity::EdgebreakerDecoder;
use crate::shared::header::EncoderMethod;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Sequential decoding error: {0}")]
    SequentialError(#[from] sequential::Err),
    
    #[error("Spirale Reversi decoding error: {0}")]
    SpiraleReversiError(#[from] spirale_reversi::Err),

    #[error("Not enough data in stream")]
    NotEnoughData(#[from] ReaderErr),
}

pub fn decode_connectivity_att<R>(reader: &mut R, header: Header) -> Result<Vec<[FaceIdx;3]>, Err>
    where R: ByteReader,
{
    let connectivity = match header.encoding_method {
        EncoderMethod::Edgebreaker => {
            debug_expect!("Start of edgebreaker connectivity", reader);
            let mut decoder = spirale_reversi::SpiraleReversi::new();
            decoder.decode_connectivity(reader)?
        },
        EncoderMethod::Sequential => {
            debug_expect!("Start of sequential connectivity", reader);
            let mut decoder = sequential::Sequential;
            decoder.decode_connectivity(reader)?
        }
    };

    Ok(connectivity)
}


pub trait ConnectivityDecoder {
    type Err;
    fn decode_connectivity<R>(&mut self, reader: &mut R) -> Result<Vec<[VertexIdx; 3]>, Self::Err>
        where R: ByteReader;
}