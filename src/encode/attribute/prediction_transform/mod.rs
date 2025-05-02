pub mod difference;
pub(crate) mod geom;
pub mod orthogonal;
pub mod oct_orthogonal;
pub mod oct_reflection;
pub mod oct_difference;

use core::fmt;
use std::cmp;

use crate::{core::shared::{ConfigType, Vector}, shared::attribute::Portable};

use super::{attribute_encoder::GroupConfig, portabilization::Portabilization, WritableFormat};

pub enum PredictionTransform<Data> 
	where Data: Vector + Portable
{
	NoTransform(NoPredictionTransform<Data>),
	Difference(difference::Difference<Data>),
	OctahedralDifference(oct_difference::OctahedronDifferenceTransform<Data>),
	OctahedralReflection(oct_reflection::OctahedronReflectionTransform<Data>),
	OctahedralOrthogonal(oct_orthogonal::OctahedronOrthogonalTransform<Data>),
	Orthogonal(orthogonal::OrthogonalTransform<Data>),
}

impl<Data> PredictionTransform<Data> 
	where Data: Vector + Portable
{
	pub(crate) fn map_with_tentative_metadata(&mut self, orig: Data, pred: Data) {
		match self {
			PredictionTransform::NoTransform(_) => unreachable!(),
			PredictionTransform::Difference(x) => x.map_with_tentative_metadata(orig, pred),
			PredictionTransform::OctahedralDifference(x) => x.map_with_tentative_metadata(orig, pred),
			PredictionTransform::OctahedralReflection(x) => x.map_with_tentative_metadata(orig, pred),
			PredictionTransform::OctahedralOrthogonal(x) => x.map_with_tentative_metadata(orig, pred),
			PredictionTransform::Orthogonal(x) => x.map_with_tentative_metadata(orig, pred),
		}
	}

	pub(crate) fn squeeze(&mut self) -> (WritableFormat, WritableFormat) {
		match self {
			PredictionTransform::NoTransform(_) => unreachable!(),
			PredictionTransform::Difference(x) => x.squeeze(),
			PredictionTransform::OctahedralDifference(x) => x.squeeze(),
			PredictionTransform::OctahedralReflection(x) => x.squeeze(),
			PredictionTransform::OctahedralOrthogonal(x) => x.squeeze(),
			PredictionTransform::Orthogonal(x) => x.squeeze(),
		}
	}

	pub(crate) fn squeeze_and_write<F>(&mut self, writer: &mut F) -> WritableFormat
		where F: FnMut((u8, u64))
	{
		match self {
			PredictionTransform::NoTransform(_) => unreachable!(),
			PredictionTransform::Difference(x) => x.squeeze_and_write(writer),
			PredictionTransform::OctahedralDifference(x) => x.squeeze_and_write(writer),
			PredictionTransform::OctahedralReflection(x) => x.squeeze_and_write(writer),
			PredictionTransform::OctahedralOrthogonal(x) => x.squeeze_and_write(writer),
			PredictionTransform::Orthogonal(x) => x.squeeze_and_write(writer),
		}
	}
}

pub(crate) trait PredictionTransformImpl {
	const ID: usize = 0;

	type Data: Vector;
	type Correction: Vector + Copy; // ToDo: examine if Copy is needed and remove it if not
	type Metadata;
	
	/// transforms the data (the correction value) with the given metadata.
	fn map(orig: Self::Data, pred: Self::Data, metadata: Self::Metadata) -> Self::Correction;

	/// transforms the data (the correction value) with the tentative metadata value.
	/// The tentative metadata can be determined by the function without any restriction,
	/// but it needs to be returned. The output of the transform might get later on 
	/// fixed by the metadata universal to the attribute after all the transforms are
	/// done once for each attribute value.
	fn map_with_tentative_metadata(&mut self, orig: Self::Data, pred: Self::Data);
	
	/// squeezes the transform results having computed the entire attribute and
	/// gives up the final data.
	/// This includes cutting off the unnecessary data from both tentative metadata
	/// and the transformed data, or doing some trade-off's between the tentative
	/// metadata and the transformed data to decide the global metadata that will 
	/// be encoded to buffer.
	fn squeeze(&mut self) -> (WritableFormat, WritableFormat) {
		self.squeeze_impl();
		let (port_metadata, data) = self.portabilize();
		let mut metadata= self.get_final_metadata_writable_form();
		metadata.append(&port_metadata);
		(metadata, data)
	}

	fn squeeze_and_write<F>(&mut self, writer: &mut F) -> WritableFormat 
		where F: FnMut((u8, u64))
	{
		self.squeeze_impl();
		self.get_final_metadata_writable_form().write(writer);
		self.portabilize_and_write_metadata(writer)
	}

	fn squeeze_impl(&mut self);

	fn portabilize(&mut self) -> (WritableFormat, WritableFormat);

	fn portabilize_and_write_metadata<F>(&mut self, writer: &mut F) -> WritableFormat
		where F: FnMut((u8, u64));

	fn get_final_metadata(&self) -> &FinalMetadata<Self::Metadata>;

	fn get_final_metadata_writable_form(&self) -> WritableFormat;
}


#[derive(Clone)]
/// The final metadata is either local or global. Local metadata
/// is stored for each attribute value, while global metadata is stored
/// once for the entire attribute.
pub(crate) enum FinalMetadata<T> {
	Local(Vec<T>),
	Global(T)
}

impl<T> From<FinalMetadata<T>> for WritableFormat 
	where WritableFormat: From<T>
{
	fn from(data: FinalMetadata<T>) -> Self {
		let mut out = WritableFormat::new();
		match data {
			FinalMetadata::Local(x) => {
				out.push((1,0));			
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

/// Trait limiting the selections of the encoding methods for vertex coordinates.
trait TransformForVertexCoords: PredictionTransformImpl {}

/// Trait limiting the selections of the encoding methods for texture coordinates.
trait TransformForTexCoords: PredictionTransformImpl {}

/// Trait limiting the selections of the encoding methods for normals.
trait TansformForNormals: PredictionTransformImpl {}

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


pub struct NoPredictionTransform<Data> 
	where Data: Vector
{
	_marker: std::marker::PhantomData<Data>,
	portabilization: Portabilization<<Self as PredictionTransformImpl>::Correction>,
}

impl<Data> NoPredictionTransform<Data> 
	where Data: Vector + Portable
{
	pub fn new(cfg: GroupConfig) -> Self {
		let portabilization = Portabilization::new(cfg.portabilization);
		Self {
			_marker: std::marker::PhantomData,
			portabilization,
		}
	}

	pub fn new_with_portabilization(portabilization: Portabilization<<Self as PredictionTransformImpl>::Correction>) -> Self {
		Self {
			_marker: std::marker::PhantomData,
			portabilization,
		}
	}
}

impl<Data> PredictionTransformImpl for NoPredictionTransform<Data> 
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

	fn squeeze_impl(&mut self) {
		unreachable!()
	}

	fn portabilize(&mut self) -> (WritableFormat, WritableFormat) {
		unreachable!()
	}

	fn portabilize_and_write_metadata<F>(&mut self, _writer: &mut F) -> WritableFormat 
		where F: FnMut((u8, u64))
	{
		unreachable!()
	}

	fn get_final_metadata(&self) -> &FinalMetadata<Self::Metadata> {
		unreachable!()
	}

	fn get_final_metadata_writable_form(&self) -> WritableFormat {
		unreachable!()
	}
}