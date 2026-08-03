const TWO_POW_21: usize = 1 << 21;
const TWO_POW_32: usize = 1 << 32;

/// The width face indices are stored in for a point space of `point_count`.
/// `21` selects varint storage; the others are that many fixed bits.
#[inline]
pub fn index_size_from_vertex_count(point_count: usize) -> Result<usize, Err> {
    match point_count {
        0..0x100 => Ok(8),
        0x100..0x10000 => Ok(16),
        0x10000..TWO_POW_21 => Ok(21),
        TWO_POW_21..TWO_POW_32 => Ok(32),
        _ => Err(Err::TooManyVertices),
    }
}

#[derive(Debug)]
pub enum Err {
    TooManyVertices,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    #[allow(unused)]
    Compressed,
    DirectIndices,
}

impl Method {
    #[allow(unused)]
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
