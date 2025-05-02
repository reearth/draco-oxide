pub mod prediction_scheme;
pub mod portabilization;

pub trait Portable {
    fn to_bits(&self) -> Vec<(u8, u64)>;
}
