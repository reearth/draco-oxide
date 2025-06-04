mod sequential;
mod spirale_reversi;
use crate::core::attribute::{Attribute, AttributeId}; 
use crate::core::bit_coder::ReaderErr;
use crate::core::shared::VertexIdx; 
use crate::debug_expect;
use crate::prelude::ByteReader;
use crate::shared::connectivity::{EdgebreakerDecoder, Encoder};

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Sequential decoding error: {0}")]
    SequentialError(#[from] sequential::Err),
    
    #[error("Spirale Reversi decoding error: {0}")]
    SpiraleReversiError(#[from] spirale_reversi::Err),

    #[error("Not enough data in stream")]
    NotEnoughData(#[from] ReaderErr),
}

pub fn decode_connectivity_atts<R>(reader: &mut R) -> Result<Vec<Attribute>, Err>
    where R: ByteReader,
{
    // ToDo: get rans coder

    let num_atts = reader.read_u8().unwrap() as usize;

    let mut atts = Vec::with_capacity(num_atts);
    for i in 0..num_atts {
        let connectivity = match Encoder::from_id(reader.read_u8().unwrap() as u64) {
            Encoder::Edgebreaker => {
                debug_expect!("Start of edgebreaker connectivity", reader);
                match EdgebreakerDecoder::from_id(reader.read_u8().unwrap() as u64) {
                    EdgebreakerDecoder::SpiraleReversi => {
                        let mut decoder = spirale_reversi::SpiraleReversi::new();
                        decoder.decode_connectivity(reader)?
                    }
                }
            },
            Encoder::Sequential => {
                debug_expect!("Start of sequential connectivity", reader);
                let mut decoder = sequential::Sequential;
                decoder.decode_connectivity(reader)?
            }
        };
        let attribute = Attribute::from_faces(AttributeId::new(i ), connectivity, Vec::new());
        atts.push(attribute);
    }

    Ok(atts)
}


pub trait ConnectivityDecoder {
    fn decode_connectivity<R>(&mut self, reader: &mut R) -> Result<Vec<[VertexIdx; 3]>, Err>
        where R: ByteReader;
}