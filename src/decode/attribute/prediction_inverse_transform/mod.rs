pub(crate) mod difference;
pub(crate) mod oct_difference;
pub(crate) mod oct_orthogonal;
pub(crate) mod oct_reflection;
pub(crate) mod orthogonal;

pub(crate) trait PredictionInverseTransformImpl {
    type Data;
    type Correction;
    type Metadata;

    const ID: usize;

    fn init(&mut self, metadata: Self::Metadata);
    fn inverse(&mut self, pred: Self::Data, crr: Self::Correction) -> Self::Data;
}

