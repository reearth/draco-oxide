#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    
}

pub fn encode_metadata<F>(
    _mesh: &crate::core::mesh::Mesh,
    _writer: &mut F,
) -> Result<(), Err>     
    where F: FnMut((u8, u64)),
{
    Ok(())
}