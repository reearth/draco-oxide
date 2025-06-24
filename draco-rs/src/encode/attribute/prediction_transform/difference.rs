use crate::core::shared::{DataValue, Vector};
use crate::prelude::{ByteWriter, NdVector};
use crate::utils::to_positive_i32_vec;
use super::PredictionTransformImpl;

#[cfg(feature = "evaluation")]
use crate::eval;

pub struct Difference<const N: usize> 
{
    cfg: super::Config,
    out: Vec<NdVector<N,i32>>,
}

impl<const N: usize> Difference<N> 
{
    pub fn new(cfg: super::Config) -> Self {
        Self {
            cfg,
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

    fn squeeze<W>(self, writer: &mut W) -> Vec<NdVector<N, i32>>
        where W: ByteWriter
    {
        #[cfg(feature = "evaluation")]
        {
            eval::write_json_pair("transform  type", "Difference".into(), writer);
            eval::array_scope_begin("transformed data", writer);
            for &x in self.out.iter() {
                eval::write_arr_elem(x.into(), writer);
            }
            eval::array_scope_end(writer);
        }
        self.out
    }
}

