pub(crate) mod edgebreaker;
pub(crate) mod sequential;

#[remain::sorted]
pub(crate) enum Encoder {
    Edgebreaker,
    Sequential
}

impl Encoder {
    /// returns the id of the encoder.
    pub(crate) fn id(&self) -> u64 {
        match self {
            Encoder::Edgebreaker => 0,
            Encoder::Sequential => 1,
        }
    }

    /// returns the encoder from the id.
    pub(crate) fn from_id(id: u64) -> Self {
        match id {
            0 => Encoder::Edgebreaker,
            1 => Encoder::Sequential,
            _ => panic!("Unknown encoder id: {}", id),
        }
    }
}


#[remain::sorted]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EdgebreakerDecoder {
    SpiraleReversi
}

impl EdgebreakerDecoder {
    /// returns the id of the encoder.
    pub(crate) fn id(&self) -> u64 {
        match self {
            EdgebreakerDecoder::SpiraleReversi => 0,
        }
    }

    /// returns the decoder from the id.
    pub(crate) fn from_id(id: u64) -> Self {
        match id {
            0 => EdgebreakerDecoder::SpiraleReversi,
            _ => panic!("Unknown edgebreaker decoder id: {}", id),
        }
    }
}

pub(crate) const NUM_CONNECTIVITY_ATTRIBUTES_SLOT: u8 = 12;