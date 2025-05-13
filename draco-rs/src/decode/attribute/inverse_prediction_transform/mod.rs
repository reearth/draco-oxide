use crate::core::shared::Vector; 
use crate::debug_expect;
use crate::shared::attribute::Portable;

use super::portabilization::{Deportabilization, DeportabilizationImpl};

pub(crate) mod difference;
pub(crate) mod oct_difference;
pub(crate) mod oct_orthogonal;
pub(crate) mod oct_reflection;
pub(crate) mod orthogonal;

pub enum InversePredictionTransform<Data> 
    where Data: Vector + Portable
{
    NoTransform(NoInversePredictionTransform<Data>),
    Difference(difference::DifferenceInverseTransform<Data>),
    OctahedralDifference(oct_difference::OctahedronDifferenceInverseTransform<Data>),
    OctahedralReflection(oct_reflection::OctahedronReflectionInverseTransform<Data>),
    OctahedralOrthogonal(oct_orthogonal::OctahedronOrthogonalInverseTransform<Data>),
    Orthogonal(orthogonal::OrthogonalInverseTransform<Data>),
}

impl<Data> InversePredictionTransform<Data> 
    where Data: Vector + Portable
{
    pub(crate) fn new<F>(
        stream_in: &mut F,
    ) -> Result<Self, Err> 
        where F: FnMut(u8)->u64
    {
        debug_expect!("Start of Prediction Transform Metadata", stream_in);
        let ty: InversePredictionTransformType = InversePredictionTransformType::from_id(stream_in)
            .map_err(|id| Err::InvalidInversePredictionTransformId(id) )?;
        let out = match ty {
            InversePredictionTransformType::NoTransform => {
                InversePredictionTransform::NoTransform(NoInversePredictionTransform::new(stream_in)?)
            }
            InversePredictionTransformType::Difference => {
                InversePredictionTransform::Difference(difference::DifferenceInverseTransform::new(stream_in)?)
            }
            InversePredictionTransformType::OctahedralDifference => {
                InversePredictionTransform::OctahedralDifference(oct_difference::OctahedronDifferenceInverseTransform::new(stream_in)?)
            }
            InversePredictionTransformType::OctahedralReflection => {
                InversePredictionTransform::OctahedralReflection(oct_reflection::OctahedronReflectionInverseTransform::new(stream_in)?)
            }
            InversePredictionTransformType::OctahedralOrthogonal => {
                InversePredictionTransform::OctahedralOrthogonal(oct_orthogonal::OctahedronOrthogonalInverseTransform::new(stream_in)?)
            }
            InversePredictionTransformType::Orthogonal => {
                InversePredictionTransform::Orthogonal(orthogonal::OrthogonalInverseTransform::new(stream_in)?)
            }
        };
        debug_expect!("End of Prediction Transform Metadata", stream_in);
        Ok(out)
    }

    pub(crate) fn inverse<F>(
        &mut self,
        pred: Data,
        stream_in: &mut F,
    ) -> Data 
        where F: FnMut(u8)->u64
    {
        match self {
            InversePredictionTransform::NoTransform(x) => x.inverse(pred, stream_in),
            InversePredictionTransform::Difference(x) => x.inverse(pred, stream_in),
            InversePredictionTransform::OctahedralDifference(x) => x.inverse(pred, stream_in),
            InversePredictionTransform::OctahedralReflection(x) => x.inverse(pred, stream_in),
            InversePredictionTransform::OctahedralOrthogonal(x) => x.inverse(pred, stream_in),
            InversePredictionTransform::Orthogonal(x) => x.inverse(pred, stream_in),
        }
    }
}

#[remain::sorted]
#[derive(Debug, Clone, Copy)]
pub enum InversePredictionTransformType {
    Difference,
    NoTransform,
    OctahedralDifference,
    OctahedralOrthogonal,
    OctahedralReflection,
    Orthogonal,
}

impl InversePredictionTransformType {
    pub(crate) fn from_id<F>(stream_in: &mut F) -> Result<Self, usize> 
        where F: FnMut(u8)->u64
    {
        let id = stream_in(4) as usize;
        let out = match id {
            0 => InversePredictionTransformType::NoTransform,
            1 => InversePredictionTransformType::Difference,
            2 => InversePredictionTransformType::OctahedralDifference,
            3 => InversePredictionTransformType::OctahedralReflection,
            4 => InversePredictionTransformType::OctahedralOrthogonal,
            5 => InversePredictionTransformType::Orthogonal,
            _ => return Err(id),
        };
        Ok(out)
    }
}

pub(crate) trait InversePredictionTransformImpl: Sized {
    type Data;
    type Correction;
    type Metadata;

    const ID: usize;

    fn new<F>(stream_in: &mut F) -> Result<Self, Err>
        where F: FnMut(u8)->u64;
    
    fn inverse<F>(
        &self,
        pred: Self::Data,
        stream_in: &mut F,
    ) -> Self::Data 
        where F: FnMut(u8)->u64;
}

pub struct NoInversePredictionTransform<Data> 
    where Data: Vector + Portable
{
    deportabilization: Deportabilization<Data>,
}

impl<Data> InversePredictionTransformImpl for NoInversePredictionTransform<Data> 
    where Data: Vector + Portable
{
    type Data = Data;
    type Correction = ();
    type Metadata = ();

    const ID: usize = 0;

    fn new<F>(stream_in: &mut F) -> Result<Self, Err>
        where F: FnMut(u8)->u64
    {
        let deportabilization = Deportabilization::new(stream_in)
            .map_err(|err| Err::PortabilizationError(err))?;
        Ok(
            Self {deportabilization}
        )
    }

    fn inverse<F>(&self, _pred: Self::Data, stream_in: &mut F) -> Self::Data 
        where F: FnMut(u8)->u64
    {
        self.deportabilization.deportabilize_next(stream_in)
    }
}


#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Invalid Inverse Prediction Transform ID: {0}")]
    InvalidInversePredictionTransformId(usize),
    #[error("")]
    PortabilizationError(#[from] super::portabilization::Err),
}