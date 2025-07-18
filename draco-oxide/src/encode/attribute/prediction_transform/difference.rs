use crate::core::shared::Vector;
use crate::prelude::{ByteWriter, NdVector};
use crate::utils::to_positive_i32_vec;
use super::PredictionTransformImpl;

#[cfg(feature = "evaluation")]
use crate::eval;

pub struct Difference<const N: usize> 
{
    out: Vec<NdVector<N,i32>>,
}

impl<const N: usize> Difference<N> 
{
    pub fn new(_cfg: super::Config) -> Self {
        Self {
            out: Vec::new(),
        }
    }
}

impl<const N: usize> PredictionTransformImpl<N> for Difference<N> 
{

    fn map_with_tentative_metadata(&mut self, orig: NdVector<N,i32>, pred: NdVector<N,i32>) 
        where 
            NdVector<N,i32>: Vector<N, Component = i32>,
    {
        let corr = orig - pred;
        let corr = to_positive_i32_vec(corr);

        self.out.push(corr);
    }

    fn squeeze<W>(self, _writer: &mut W) -> Vec<NdVector<N, i32>>
        where W: ByteWriter
    {
        #[cfg(feature = "evaluation")]
        {
            eval::write_json_pair("transform  type", "Difference".into(), _writer);
            eval::array_scope_begin("transformed data", _writer);
            for &x in self.out.iter() {
                eval::write_arr_elem(x.into(), _writer);
            }
            eval::array_scope_end(_writer);
        }
        self.out
    }
}

