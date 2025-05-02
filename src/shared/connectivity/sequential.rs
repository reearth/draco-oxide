pub(crate) const INDEX_SIZE_SLOT_SELECTOR: [usize;3] = [8,16,32];
pub(crate)const INDEX_SIZE_SLOT: u8 = 2;
pub(crate) const NUM_POINTS_SLOT: u8 = 32;
pub(crate) const NUM_FACES_SLOT: u8 = 32;

pub(crate) fn index_size_from_vertex_count(vertex_count: usize) -> Result<usize, Err> {
    match vertex_count {
        0..0x100 => Ok(8),
        0x100..0x10000 => Ok(16),
        0x10000..0x1000000 => Ok(32),
        _ => Err(Err::TooManyVertices),
    }
}


#[derive(Debug)]
pub enum Err {
    TooManyVertices
}