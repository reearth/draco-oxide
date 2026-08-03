use super::PredictionTransformImpl;
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::attribute::geom::invert_diamond;
use draco_oxide_core::types::{NdVector, Vector};

pub struct OctahedronOrthogonalTransform<const N: usize> {
    out: Vec<NdVector<N, i32>>,
    center: i32,
}

impl<const N: usize> OctahedronOrthogonalTransform<N> {
    pub fn new(cfg: super::Config) -> Self {
        Self {
            out: Vec::new(),
            center: cfg.portabilization.oct_center(),
        }
    }
}

impl<const N: usize> PredictionTransformImpl<N> for OctahedronOrthogonalTransform<N> {
    fn map_with_tentative_metadata(
        &mut self,
        mut orig: NdVector<N, i32>,
        mut pred: NdVector<N, i32>,
    ) where
        NdVector<N, i32>: Vector<N, Component = i32>,
    {
        assert!(N == 2);
        let one = self.center;
        let max_quantized = 2 * one + 1;

        *pred.get_mut(0) -= one;
        *pred.get_mut(1) -= one;
        *orig.get_mut(0) -= one;
        *orig.get_mut(1) -= one;
        if pred.get(0).abs() + pred.get(1).abs() > one {
            let mut p = NdVector::<2, i32>::from([*pred.get(0), *pred.get(1)]);
            invert_diamond(&mut p, one);
            *pred.get_mut(0) = *p.get(0);
            *pred.get_mut(1) = *p.get(1);
            let mut o = NdVector::<2, i32>::from([*orig.get(0), *orig.get(1)]);
            invert_diamond(&mut o, one);
            *orig.get_mut(0) = *o.get(0);
            *orig.get_mut(1) = *o.get(1);
        }

        // Rotate until both of pred's coordinates are negative, taking orig along.
        if pred != NdVector::<N, i32>::zero() {
            while *pred.get(0) >= 0 || *pred.get(1) > 0 {
                let tmp = *pred.get(0);
                *pred.get_mut(0) = -pred.get(1);
                *pred.get_mut(1) = tmp;

                let tmp = *orig.get(0);
                *orig.get_mut(0) = -orig.get(1);
                *orig.get_mut(1) = tmp;
            }
        }

        let mut corr = orig - pred;
        for i in 0..N {
            if *corr.get(i) < 0 {
                *corr.get_mut(i) += max_quantized;
            }
        }
        self.out.push(corr);
    }

    fn squeeze<W>(self, writer: &mut W) -> Vec<NdVector<N, i32>>
    where
        W: ByteWriter,
    {
        writer.write_u32((2 * self.center + 1) as u32);
        writer.write_u32(self.center as u32);
        self.out
    }
}
