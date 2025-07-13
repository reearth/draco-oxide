use crate::core::bit_coder::ReaderErr;
use crate::{core::shared::Vector, prelude::ByteReader}; 
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
    pub(crate) fn new<R>(
        reader: &mut R,
    ) -> Result<Self, Err> 
        where R: ByteReader
    {
        debug_expect!("Start of Prediction Transform Metadata", reader);
        let ty: InversePredictionTransformType = InversePredictionTransformType::read_from(reader)
            .map_err(|id| Err::InvalidInversePredictionTransformId(id) )?;
        let out = match ty {
            InversePredictionTransformType::NoTransform => {
                InversePredictionTransform::NoTransform(NoInversePredictionTransform::new(reader)?)
            }
            InversePredictionTransformType::Difference => {
                InversePredictionTransform::Difference(difference::DifferenceInverseTransform::new(reader)?)
            }
            InversePredictionTransformType::OctahedralDifference => {
                InversePredictionTransform::OctahedralDifference(oct_difference::OctahedronDifferenceInverseTransform::new(reader)?)
            }
            InversePredictionTransformType::OctahedralReflection => {
                InversePredictionTransform::OctahedralReflection(oct_reflection::OctahedronReflectionInverseTransform::new(reader)?)
            }
            InversePredictionTransformType::OctahedralOrthogonal => {
                InversePredictionTransform::OctahedralOrthogonal(oct_orthogonal::OctahedronOrthogonalInverseTransform::new(reader)?)
            }
            InversePredictionTransformType::Orthogonal => {
                InversePredictionTransform::Orthogonal(orthogonal::OrthogonalInverseTransform::new(reader)?)
            }
        };
        debug_expect!("End of Prediction Transform Metadata", reader);
        Ok(out)
    }

    pub(crate) fn inverse<R>(
        &mut self,
        pred: Data,
        reader: &mut R,
    ) -> Data 
        where R: ByteReader
    {
        match self {
            InversePredictionTransform::NoTransform(x) => x.inverse(pred, reader),
            InversePredictionTransform::Difference(x) => x.inverse(pred, reader),
            InversePredictionTransform::OctahedralDifference(x) => x.inverse(pred, reader),
            InversePredictionTransform::OctahedralReflection(x) => x.inverse(pred, reader),
            InversePredictionTransform::OctahedralOrthogonal(x) => x.inverse(pred, reader),
            InversePredictionTransform::Orthogonal(x) => x.inverse(pred, reader),
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
    pub(crate) fn read_from<R>(reader: &mut R) -> Result<Self, usize> 
        where R: ByteReader
    {
        let id = reader.read_u8().unwrap() as usize; // TODO: handle error properly
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

    fn new<R>(reader: &mut R) -> Result<Self, Err>
        where R: ByteReader;
    
    fn inverse<R>(
        &self,
        pred: Self::Data,
        reader: &mut R,
    ) -> Self::Data 
        where R: ByteReader;
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

    fn new<R>(reader: &mut R) -> Result<Self, Err>
        where R: ByteReader
    {
        let deportabilization = Deportabilization::new(reader)
            .map_err(|err| Err::PortabilizationError(err))?;
        Ok(
            Self {deportabilization}
        )
    }

    fn inverse<R>(&self, _pred: Self::Data, reader: &mut R) -> Self::Data 
        where R: ByteReader
    {
        self.deportabilization.deportabilize_next(reader)
    }
}


#[remain::sorted]
#[derive(thiserror::Error, Debug)]
pub enum Err {
    #[error("Invalid Inverse Prediction Transform ID: {0}")]
    InvalidInversePredictionTransformId(usize),
    #[error("Not enough data to read")]
    NotEnoughData(#[from] ReaderErr),
    #[error("")]
    PortabilizationError(#[from] super::portabilization::Err),
}