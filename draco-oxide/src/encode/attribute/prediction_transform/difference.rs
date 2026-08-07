use super::PredictionTransformImpl;
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::types::NdVector;
use draco_oxide_core::types::Vector;
use draco_oxide_core::utils::to_positive_i32_vec;

pub struct Difference<const N: usize> {
    out: Vec<NdVector<N, i32>>,
}

impl<const N: usize> Difference<N> {
    pub fn new(_cfg: super::Config) -> Self {
        Self { out: Vec::new() }
    }
}

impl<const N: usize> PredictionTransformImpl<N> for Difference<N> {
    fn map_with_tentative_metadata(&mut self, orig: NdVector<N, i32>, pred: NdVector<N, i32>)
    where
        NdVector<N, i32>: Vector<N, Component = i32>,
    {
        let corr = orig - pred;
        let corr = to_positive_i32_vec(corr);

        self.out.push(corr);
    }

    fn squeeze<W>(self, _writer: &mut W) -> Vec<NdVector<N, i32>>
    where
        W: ByteWriter,
    {
        self.out
    }
}
