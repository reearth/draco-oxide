use crate::core::shared::{DataValue, Vector};
use crate::prelude::{ByteWriter, NdVector};
use crate::shared::attribute::Portable;
use crate::utils::to_positive_i32_vec;
use super::PredictionTransformImpl;

#[cfg(feature = "evaluation")]
use crate::eval;

pub struct WrappedDifference<const N: usize> 
{
    cfg: super::Config,
    preds: Vec<NdVector<N,i32>>,
    origs: Vec<NdVector<N,i32>>,
    max: i32,
    min: i32,
}

impl<const N: usize> WrappedDifference<N> 
{
    pub fn new(cfg: super::Config) -> Self {
        Self {
            cfg,
            preds: Vec::new(),
            origs: Vec::new(),
            max: i32::MIN,
            min: i32::MAX,
        }
    }
}

impl<const N: usize> PredictionTransformImpl<N> for WrappedDifference<N> 
    where NdVector<N,i32>: Vector<N, Component = i32>
{

    fn map_with_tentative_metadata(&mut self, orig: NdVector<N,i32>, pred: NdVector<N,i32>) 
        where 
            NdVector<N,i32>: Vector<N, Component = i32>,
    {
        // Update min and max values for the wrapped difference
        for i in 0..N {
            let orig_val = *orig.get(i);
            if orig_val > self.max {
                self.max = orig_val;
            }
            if orig_val < self.min {
                self.min = orig_val;
            }
        }
        self.origs.push(orig);
        self.preds.push(pred);
    }

    fn squeeze<W>(self, writer: &mut W) -> Vec<NdVector<N, i32>>
        where W: ByteWriter
    {
        #[cfg(feature = "evaluation")]
        {
            eval::write_json_pair("transform type", "WrappedDifference".into(), writer);
            eval::array_scope_begin("prediction data", writer);
            for &x in self.preds.iter() {
                eval::write_arr_elem(x.into(), writer);
            }
            eval::array_scope_end(writer);
        }
        let  diff = self.max - self.min;
        let max_diff = 1 + diff;
        let mut max_corr = max_diff / 2;
        let min_corr = -max_corr;
        if (max_diff & 1) == 0 {
            max_corr -= 1;
        }

        // compute the wrapped difference
        let mut out = Vec::with_capacity(self.origs.len());
        for (orig, mut pred) in self.origs.into_iter().zip(self.preds.into_iter()) {
            let mut corr = NdVector::zero();
            for i in 0..N {
                // clamp the prediction values
                *pred.get_mut(i) = *pred.get(i).clamp(&self.min, &self.max);
                // then compute the wrapped difference
                let val = *orig.get(i) - *pred.get(i);
                if val > max_corr {
                    *corr.get_mut(i) = val - max_diff;
                } else if val < min_corr {
                    *corr.get_mut(i) = val + max_diff;
                } else {
                    *corr.get_mut(i) = val;
                }
            }
            out.push(to_positive_i32_vec(corr));
        }

        // write metadata
        self.min.write_to(writer);
        self.max.write_to(writer);

        out
    }
}

