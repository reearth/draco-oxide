use crate::{core::shared::Vector, shared::attribute::Portable};

use super::PredictionTransform;


pub struct Difference<Data> {
    out: Vec<Data>,
}

impl<Data> Difference<Data> {
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
        }
    }
}

impl<Data> PredictionTransform for Difference<Data> 
    where Data: Vector
{
    const ID: usize = 1;

    type Data = Data;
    type Correction = Data;
    type Metadata = Data;

    fn map(orig: Self::Data, pred: Self::Data, metadata: Self::Metadata) {
        // Implement the mapping logic here
    }

    fn map_with_tentative_metadata(&mut self, orig: Self::Data, pred: Self::Data) {
        // Implement the mapping with tentative metadata logic here
    }

    fn inverse(&mut self, pred: Self::Data, crr: Self::Correction, metadata: Self::Metadata) {
        // Implement the inverse logic here
    }

    fn squeeze(&mut self) -> Vec<Self::Correction> {
        // Implement the squeezing logic here
        vec![]
    }
}