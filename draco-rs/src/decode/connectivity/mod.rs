mod sequential;
mod spirale_reversi;
use crate::core::attribute::{Attribute, AttributeId}; 
use crate::core::shared::VertexIdx; 
use crate::shared::connectivity::{EdgebreakerDecoder, Encoder, NUM_CONNECTIVITY_ATTRIBUTES_SLOT};

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Sequential decoding error: {0}")]
    SequentialError(#[from] sequential::Err),
    
    #[error("Spirale Reversi decoding error: {0}")]
    SpiraleReversiError(#[from] spirale_reversi::Err),
}

pub fn decode_connectivity_atts<F>(reader: &mut F) -> Result<Vec<Attribute>, Err>
    where F: FnMut(u8) -> u64,
{
    // ToDo: get rans coder

    let num_atts = reader(NUM_CONNECTIVITY_ATTRIBUTES_SLOT) as usize;

    let mut atts = Vec::with_capacity(num_atts);
    for i in 0..num_atts {
        let connectivity = match Encoder::from_id(reader(1 as u8)) {
            Encoder::Edgebreaker => {
                match EdgebreakerDecoder::from_id(reader(3 as u8)) {
                    EdgebreakerDecoder::SpiraleReversi => {
                        let mut decoder = spirale_reversi::SpiraleReversi::new();
                        decoder.decode_connectivity(reader)?
                    }
                }
            },
            Encoder::Sequential => {
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
    fn decode_connectivity<F>(&mut self, reader: &mut F) -> Result<Vec<[VertexIdx; 3]>, Err>
        where F: FnMut(u8) -> u64; 
}