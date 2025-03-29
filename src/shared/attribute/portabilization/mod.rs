mod quantization;
use crate::core::shared::Vector;

pub(crate) trait Portabilization {
    type Data: Vector;
    type Output: Vector<Component = u64>;
    fn skim(&mut self, att_val: &Self::Data) ;
    fn portabilize(&mut self, att_val: Self::Data) -> Self::Output;
}