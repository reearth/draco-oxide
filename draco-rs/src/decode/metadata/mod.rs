use crate::core::mesh::metadata::Metadata;


#[derive(thiserror::Error, Debug)]
pub enum Err {

}

pub fn decode_metadata<F>(_stream_in: &mut F) -> Result<Metadata, Err>
    where F: FnMut(u8) -> u64,
{
    Ok(Metadata::new())
}