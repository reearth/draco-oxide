
#[derive(thiserror::Error, Debug)]
pub enum Err {

}

pub fn decode_header<F>(_stream_in: &mut F) -> Result<(), Err>
where
    F: FnMut(u8) -> u64,
{
    Ok(())
}