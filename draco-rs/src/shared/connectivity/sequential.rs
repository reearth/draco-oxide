pub(crate) const NUM_POINTS_SLOT: u8 = 32;
pub(crate) const NUM_FACES_SLOT: u8 = 32;
pub(crate) const ENCODER_METHOD_SLOT: u8 = 8;

const TWO_POW_21: usize = 1 << 21;

#[inline]
pub(crate) fn index_size_from_vertex_count(vertex_count: usize) -> Result<usize, Err> {
    match vertex_count {
        0..0x100 => Ok(8),
        0x100..0x10000 => Ok(16),
        0x10000..TWO_POW_21 => Ok(21),
        TWO_POW_21..0x1000000 => Ok(32),
        _ => Err(Err::TooManyVertices),
    }
}


#[derive(Debug)]
pub enum Err {
    TooManyVertices
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    Compressed,
    DirectIndices
}

impl Method {
    pub fn from_id(id: u8) -> Self {
        match id {
            0 => Self::Compressed,
            1 => Self::DirectIndices,
            _ => panic!("Unknown method id: {}", id),
        }
    }
    pub fn get_id(&self) -> u8 {
        match self {
            Self::Compressed => 0,
            Self::DirectIndices => 1,
        }
    }
}