pub mod difference;
mod geom;
pub mod orthogonal;
pub mod oct_orthogonal;
pub mod oct_reflection;
pub mod oct_difference;

use crate::core::shared::{ConfigType, DataValue, NdVector, Vector};

pub(crate) trait PredictionTransform {
	const ID: usize = 0;

	type Data: Vector;
	type Correction: Vector + Copy; // examine if Copy is needed and remove it if not
	type Metadata;
	
	/// transforms the data (the correction value) with the given metadata.
	fn map(orig: Self::Data, pred: Self::Data, metadata: Self::Metadata) -> Self::Correction;

	/// transforms the data (the correction value) with the tentative metadata value.
	/// The tentative metadata can be determined by the function without any restriction,
	/// but it needs to be returned. The output of the transform might get later on 
	/// fixed by the metadata universal to the attribute after all the transforms are
	/// done once for each attribute value.
	fn map_with_tentative_metadata(&mut self, orig: Self::Data, pred: Self::Data);

	/// The inverse transform revertes 'map()'.
	fn inverse(&mut self, pred: Self::Data, crr: Self::Correction, metadata: Self::Metadata) -> Self::Data;
	
	/// squeezes the transform results having computed the entire attribute and
	/// gives up the final data.
	/// This includes cutting off the unnecessary data from both tentative metadata
	/// and the transformed data, or doing some trade-off's between the tentative
	/// metadata and the transformed data to decide the global metadata that will 
	/// be encoded to buffer.
	fn squeeze(&mut self) -> (FinalMetadata<Self::Metadata>, Vec<Self::Correction>);
}

pub(crate) enum FinalMetadata<T> {
	Local(Vec<T>),
	Global(T)
}

/// Trait limiting the selections of the encoding methods for vertex coordinates.
trait TransformForVertexCoords: PredictionTransform {}

/// Trait limiting the selections of the encoding methods for texture coordinates.
trait TransformForTexCoords: PredictionTransform {}

/// Trait limiting the selections of the encoding methods for normals.
trait TansformForNormals: PredictionTransform {}

#[derive(Clone, Copy)]
pub enum PredictionTransformType {
	Difference,
	OctahedralDifference,
	OctahedralReflection,
	OctahedralOrthogonal,
	Orthogonal,
	NoTransform,
}

#[derive(Clone, Copy)]
pub struct Config {
	pub prediction_transform: PredictionTransformType,
}

impl ConfigType for Config {
	fn default()-> Self {
		Config {
			prediction_transform: PredictionTransformType::Difference,
		}
	}
}


pub struct NoPredictionTransform<Data> {
	_marker: std::marker::PhantomData<Data>,
}

impl<Data> PredictionTransform for NoPredictionTransform<Data> 
	where 
		Data: Vector,
{
	const ID: usize = 0;
	type Data = Data;
	type Correction = Data;
	type Metadata = ();
	fn map(_orig: Self::Data, _pred: Self::Data, _metadata: Self::Metadata) -> Self::Correction{
		unreachable!()
	}
	fn map_with_tentative_metadata(&mut self, _orig: Self::Data, _pred: Self::Data) {
		unreachable!()
	}
	fn inverse(&mut self, _pred: Self::Data, _crr: Self::Correction, _metadata: Self::Metadata) -> Self::Data {
		unreachable!()
	}
	fn squeeze(&mut self) -> (FinalMetadata<Self::Metadata>, Vec<Self::Correction>) {
		unreachable!()
	}
}