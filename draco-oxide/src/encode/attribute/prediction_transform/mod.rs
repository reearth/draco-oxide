pub mod difference;
pub mod wrapped_difference;
pub(crate) mod geom;
pub mod orthogonal;
pub mod oct_orthogonal;
pub mod oct_reflection;

#[cfg(feature = "evaluation")]
use crate::eval;
use crate::prelude::{ByteWriter, NdVector};

use crate::core::shared::{ConfigType, Vector};

#[enum_dispatch::enum_dispatch(PredictionTransformImpl<N>)]
pub enum PredictionTransform<const N: usize> 
{
	Difference(difference::Difference<N>),
	WrappedDifference(wrapped_difference::WrappedDifference<N>),
	NoTransform(NoPredictionTransform<N>),
	OctahedralOrthogonal(oct_orthogonal::OctahedronOrthogonalTransform<N>),
	OctahedralReflection(oct_reflection::OctahedronReflectionTransform<N>),
	Orthogonal(orthogonal::OrthogonalTransform<N>),
}

impl<const N: usize> PredictionTransform<N> 
{
	pub(crate) fn new(cfg: Config) -> Self {
		let ty = cfg.ty;
		match ty {
			PredictionTransformType::NoTransform => PredictionTransform::NoTransform(NoPredictionTransform::new(cfg)),
			PredictionTransformType::Difference => PredictionTransform::Difference(difference::Difference::new(cfg)),
			PredictionTransformType::WrappedDifference => PredictionTransform::WrappedDifference(wrapped_difference::WrappedDifference::new(cfg)),
			PredictionTransformType::OctahedralReflection => PredictionTransform::OctahedralReflection(oct_reflection::OctahedronReflectionTransform::new(cfg)),
			PredictionTransformType::OctahedralOrthogonal => PredictionTransform::OctahedralOrthogonal(oct_orthogonal::OctahedronOrthogonalTransform::new(cfg)),
			PredictionTransformType::Orthogonal => PredictionTransform::Orthogonal(orthogonal::OrthogonalTransform::new(cfg)),
		}
	}
	#[allow(unused)] // TODO: Remove this whenever possible.
	#[inline]
	pub(crate) fn get_type(&self) -> PredictionTransformType {
		match self {
			PredictionTransform::NoTransform(_) => PredictionTransformType::NoTransform,
			PredictionTransform::Difference(_) => PredictionTransformType::Difference,
			PredictionTransform::WrappedDifference(_) => PredictionTransformType::WrappedDifference,
			PredictionTransform::OctahedralReflection(_) => PredictionTransformType::OctahedralReflection,
			PredictionTransform::OctahedralOrthogonal(_) => PredictionTransformType::OctahedralOrthogonal,
			PredictionTransform::Orthogonal(_) => PredictionTransformType::Orthogonal,
		}
	}
}


#[enum_dispatch::enum_dispatch]
pub(crate) trait PredictionTransformImpl<const N: usize> {
	/// transforms the data (the correction value) with the tentative metadata value.
	/// The tentative metadata can be determined by the function without any restriction,
	/// but it needs to be returned. The output of the transform might get later on 
	/// fixed by the metadata universal to the attribute after all the transforms are
	/// done once for each attribute value.
	fn map_with_tentative_metadata(&mut self, orig: NdVector<N, i32>, pred: NdVector<N, i32>)
		where NdVector<N, i32>: Vector<N, Component = i32>;
	
	/// squeezes the transform results having computed the entire attribute and
	/// gives up the final data.
	/// This includes cutting off the unnecessary data from both tentative metadata
	/// and the transformed data, or doing some trade-off's between the tentative
	/// metadata and the transformed data to decide the global metadata that will 
	/// be encoded to buffer.
	fn squeeze<W>(self, writer: &mut W) -> Vec<NdVector<N, i32>>
		where 
			W: ByteWriter,
			NdVector<N, i32>: Vector<N, Component = i32>;
}



#[derive(Clone, Copy, Debug)]
pub enum PredictionTransformType {
	NoTransform,
	Difference,
	WrappedDifference,
	OctahedralOrthogonal,
	#[allow(unused)] // TODO: This variant is not used yet, as we only support the default configuration. Remove this when we implement the octahedral orthogonal transform.
	OctahedralReflection,
	#[allow(unused)] // TODO: This variant is not used yet, as we only support the default configuration. Remove this when we implement the orthogonal transform.
	Orthogonal,
}

impl PredictionTransformType {
	/// gets the prediction transform type from its id.
	#[inline]
	pub(crate) fn get_id(&self) -> u8 {
		match self {
			PredictionTransformType::NoTransform => 0xFF, // -1 in i8
			PredictionTransformType::Difference => 0,
			PredictionTransformType::WrappedDifference => 1,
			PredictionTransformType::OctahedralReflection => 2,
			PredictionTransformType::OctahedralOrthogonal => 3,

			PredictionTransformType::Orthogonal => 4,
		}
	}

	/// Writes the prediction transform type to a byte stream.
	#[inline]
	pub(crate) fn write_to<W>(self, writer: &mut W) 
		where W: ByteWriter
	{
		let id = self.get_id();
		writer.write_u8(id);
	}
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
	pub ty: PredictionTransformType,
	#[allow(unused)]
	pub portabilization: super::portabilization::Config,
}

impl ConfigType for Config {
	fn default()-> Self {
		Config {
			ty: PredictionTransformType::Difference,
			portabilization: <super::portabilization::Config as ConfigType>::default(),
		}
	}
}


pub struct NoPredictionTransform<const N: usize> 
{
	out: Vec<NdVector<N, i32>>,
}

impl<const N: usize> NoPredictionTransform<N> 
{
	pub fn new(_cfg: Config) -> Self {
		Self {
			out: Vec::new(),
		}
	}
}

impl<const N: usize> PredictionTransformImpl<N> for NoPredictionTransform<N> 
{
	fn map_with_tentative_metadata(&mut self, orig: NdVector<N,i32>, _pred: NdVector<N,i32>) {
		self.out.push(orig);
	}

	fn squeeze<W>(self, _writer: &mut W) -> Vec<NdVector<N, i32>>
		where W: ByteWriter
	{
		#[cfg(feature = "evaluation")]
        {
            eval::array_scope_begin("transformed data", _writer);
            for &x in self.out.iter() {
                eval::write_arr_elem(x.into(), _writer);
            }
            eval::array_scope_end(_writer);
        }

		self.out
	}
}