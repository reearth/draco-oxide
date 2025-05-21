pub mod difference;
pub(crate) mod geom;
pub mod orthogonal;
pub mod oct_orthogonal;
pub mod oct_reflection;
pub mod oct_difference;
use crate::encode::attribute::portabilization::PortabilizationImpl;

#[cfg(feature = "evaluation")]
use crate::eval;

use std::{
	cmp,
	fmt
};

use crate::core::shared::{ConfigType, Vector};
use crate::shared::attribute::Portable;

use super::portabilization::{self, Portabilization};
use crate::encode::attribute::WritableFormat;

#[remain::sorted]
#[enum_dispatch::enum_dispatch(PredictionTransformImpl<Data>)]
pub enum PredictionTransform<Data> 
	where Data: Vector + Portable
{
	Difference(difference::Difference<Data>),
	NoTransform(NoPredictionTransform<Data>),
	OctahedralDifference(oct_difference::OctahedronDifferenceTransform<Data>),
	OctahedralOrthogonal(oct_orthogonal::OctahedronOrthogonalTransform<Data>),
	OctahedralReflection(oct_reflection::OctahedronReflectionTransform<Data>),
	Orthogonal(orthogonal::OrthogonalTransform<Data>),
}

impl<Data> PredictionTransform<Data> 
	where Data: Vector + Portable
{
	pub(crate) fn new(cfg: Config) -> Self {
		let ty = cfg.ty;
		match ty {
			PredictionTransformType::NoTransform => PredictionTransform::NoTransform(NoPredictionTransform::new(cfg)),
			PredictionTransformType::Difference => PredictionTransform::Difference(difference::Difference::new(cfg)),
			PredictionTransformType::OctahedralDifference => PredictionTransform::OctahedralDifference(oct_difference::OctahedronDifferenceTransform::new(cfg)),
			PredictionTransformType::OctahedralReflection => PredictionTransform::OctahedralReflection(oct_reflection::OctahedronReflectionTransform::new(cfg)),
			PredictionTransformType::OctahedralOrthogonal => PredictionTransform::OctahedralOrthogonal(oct_orthogonal::OctahedronOrthogonalTransform::new(cfg)),
			PredictionTransformType::Orthogonal => PredictionTransform::Orthogonal(orthogonal::OrthogonalTransform::new(cfg)),
		}
	}
	pub(crate) fn get_type(&self) -> PredictionTransformType {
		match self {
			PredictionTransform::NoTransform(_) => PredictionTransformType::NoTransform,
			PredictionTransform::Difference(_) => PredictionTransformType::Difference,
			PredictionTransform::OctahedralDifference(_) => PredictionTransformType::OctahedralDifference,
			PredictionTransform::OctahedralReflection(_) => PredictionTransformType::OctahedralReflection,
			PredictionTransform::OctahedralOrthogonal(_) => PredictionTransformType::OctahedralOrthogonal,
			PredictionTransform::Orthogonal(_) => PredictionTransformType::Orthogonal,
		}
	}
}


#[enum_dispatch::enum_dispatch]
pub(crate) trait PredictionTransformImpl<Data> {
	/// transforms the data (the correction value) with the tentative metadata value.
	/// The tentative metadata can be determined by the function without any restriction,
	/// but it needs to be returned. The output of the transform might get later on 
	/// fixed by the metadata universal to the attribute after all the transforms are
	/// done once for each attribute value.
	fn map_with_tentative_metadata(&mut self, orig: Data, pred: Data);
	
	/// squeezes the transform results having computed the entire attribute and
	/// gives up the final data.
	/// This includes cutting off the unnecessary data from both tentative metadata
	/// and the transformed data, or doing some trade-off's between the tentative
	/// metadata and the transformed data to decide the global metadata that will 
	/// be encoded to buffer.
	fn squeeze<F>(&mut self, writer: &mut F) 
		where F: FnMut((u8, u64));

	fn out<F>(self, writer: &mut F) -> std::vec::IntoIter<WritableFormat>
		where F: FnMut((u8, u64));
}


#[derive(Clone)]
/// The final metadata is either local or global. Local metadata
/// is stored for each attribute value, while global metadata is stored
/// once for the entire attribute.
pub(crate) enum FinalMetadata<T> {
	Local(Vec<T>),
	Global(T)
}

impl<T> FinalMetadata<T> 
	where T: Portable,
{
	pub(crate) fn read_from_bits<F>(stream_in: &mut F) -> Self 
		where F: FnMut(u8)->u64
	{
		if stream_in(1)==0 {
			let len = stream_in(64) as usize;
			let mut out = Vec::with_capacity(len);
			for _ in 0..len {
				let v = T::read_from_bits(stream_in);
				out.push(v);
			}
			FinalMetadata::Local(out)
		} else {
			let out = T::read_from_bits(stream_in);
			FinalMetadata::Global(out)
		}
	}
}

impl<T> From<FinalMetadata<T>> for WritableFormat 
	where WritableFormat: From<T>
{
	fn from(data: FinalMetadata<T>) -> Self {
		let mut out = WritableFormat::new();
		match data {
			FinalMetadata::Local(x) => {
				out.push((1,0));
				out.push((64, x.len() as u64));
				for v in x {
					out.append(&mut <WritableFormat as From<T>>::from(v));
				}
			},
			FinalMetadata::Global(x) => {
				out.push((1,1));
				out.append(&mut <WritableFormat as From<T>>::from(x));
			},
		}
		out
	}
}

impl<T> fmt::Debug for FinalMetadata<T> 
	where T: fmt::Debug
{
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			FinalMetadata::Local(x) => write!(f, "Local({:?})", x),
			FinalMetadata::Global(x) => write!(f, "Global({:?})", x),
		}
	}
}

impl<T> cmp::PartialEq for FinalMetadata<T> 
	where T: cmp::PartialEq
{
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(FinalMetadata::Local(x), FinalMetadata::Local(y)) => x == y,
			(FinalMetadata::Global(x), FinalMetadata::Global(y)) => x == y,
			_ => false,
		}
	}
}



#[remain::sorted]
#[derive(Clone, Copy, Debug)]
pub enum PredictionTransformType {
	Difference,
	NoTransform,
	OctahedralDifference,
	OctahedralOrthogonal,
	OctahedralReflection,
	Orthogonal,
}

impl PredictionTransformType {
	pub(crate) fn get_id(&self) -> u8 {
		match self {
			PredictionTransformType::NoTransform => 0,
			PredictionTransformType::Difference => 1,
			PredictionTransformType::OctahedralDifference => 2,
			PredictionTransformType::OctahedralReflection => 3,
			PredictionTransformType::OctahedralOrthogonal => 4,
			PredictionTransformType::Orthogonal => 5,
		}
	}
}

#[derive(Clone, Debug)]
pub struct Config {
	pub ty: PredictionTransformType,
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


pub struct NoPredictionTransform<Data> 
	where Data: Vector + Portable
{
	cfg: portabilization::Config,
	out: Vec<Data>,
	_marker: std::marker::PhantomData<Data>,
}

impl<Data> NoPredictionTransform<Data> 
	where Data: Vector + Portable
{
	pub fn new(cfg: Config) -> Self {
		Self {
			cfg: cfg.portabilization,
			out: Vec::new(),
			_marker: std::marker::PhantomData,
		}
	}
}

impl<Data> PredictionTransformImpl<Data> for NoPredictionTransform<Data> 
	where 
		Data: Vector + Portable,
{


	fn map_with_tentative_metadata(&mut self, orig: Data, _pred: Data) {
		self.out.push(orig);
	}

	fn out<F>(self, writer: &mut F) -> std::vec::IntoIter<WritableFormat> 
		where F: FnMut((u8, u64))
	{
		Portabilization::new(
		self.out,
			self.cfg,
			writer
		).portabilize()
	}

	fn squeeze<F>(&mut self, writer: &mut F) 
		where F: FnMut((u8,u64))  
	{
		#[cfg(feature = "evaluation")]
        {
            eval::array_scope_begin("transformed data", writer);
            for &x in self.out.iter() {
                eval::write_arr_elem(x.into(), writer);
            }
            eval::array_scope_end(writer);
        }
	}
}