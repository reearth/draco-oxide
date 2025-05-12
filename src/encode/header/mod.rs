#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    
}

pub fn encode_header<F>(writer: &mut F) -> Result<(), Err>
where
    F: FnMut((u8, u64)),
{
    Ok(())
}