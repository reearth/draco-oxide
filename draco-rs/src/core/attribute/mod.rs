use serde::Serialize;

use crate::core::shared::Vector;
use crate::prelude::{ByteReader, ByteWriter};
use super::{buffer, shared::DataValue};


#[derive(Debug, thiserror::Error)]
pub enum Err {
	/// Invalid attribute domain id
	#[error("Invalid attribute domain id: {0}")]
	InvalidAttributeDomainId(u8),
	/// Reader error
	#[error("Reader error: {0}")]
	ReaderError(#[from] crate::core::bit_coder::ReaderErr),
}

#[derive(Debug, Clone, Serialize)]
pub struct Attribute {
	/// attribute id
	id: AttributeId,

	/// attribute buffer
	buffer: buffer::attribute::AttributeBuffer,

	/// attribute type
	att_type: AttributeType,

	/// attribute domain
	domain: AttributeDomain,
	
	/// the reference of the parent, if any
	parents: Vec<AttributeId>,

	/// 
	index_map: Option<Vec<usize>>,
}

impl Attribute {
	pub(crate) fn from<Data>(id: AttributeId, data: Vec<Data>, att_type: AttributeType, domain: AttributeDomain, parents: Vec<AttributeId>) -> Self 
		where 
			Data: Vector,
	{
		let buffer = buffer::attribute::AttributeBuffer::from(data);
		Self {
			id,
			buffer,
			parents,
			att_type,
			domain,
			index_map: None,
		}
	}

	pub fn get<Data>(&self, idx: usize) -> Data 
		where 
			Data: Vector,
			Data::Component: DataValue
	{
		self.buffer.get(idx)
	}

	pub fn get_component_type(&self) -> ComponentDataType {
		self.buffer.get_component_type()
	}

	#[inline]
	pub fn get_id(&self) -> AttributeId {
		self.id
	}

	#[inline]
	pub fn get_num_components(&self) -> usize {
		self.buffer.get_num_components()
	}

	#[inline]
	pub fn get_attribute_type(&self) -> AttributeType {
		self.att_type
	}

	#[inline]
	pub fn get_domain(&self) -> AttributeDomain {
		self.domain
	}

	#[inline]
	pub fn get_parents(&self) -> &Vec<AttributeId> {
		self.parents.as_ref()
	}

	/// The number of values of the attribute.
	#[inline(always)]
	pub fn len(&self) -> usize {
		self.buffer.len()
	}

	#[inline]
	pub fn get_att_idx(&self, idx: usize) -> usize {
		if let Some(ref index_map) = self.index_map {
			index_map[idx]
		} else {
			// otherwise, we use identity mapping
			idx
		}
	}

	/// returns the data values as a slice of values casted to the given type.
	#[inline]
	pub fn as_slice<Data>(&self) -> &[Data] {
		assert_eq!(
			self.buffer.get_num_components() * self.buffer.get_component_type().size(),
			std::mem::size_of::<Data>(),
		);
		unsafe {
			self.buffer.as_slice::<Data>()
		}
	}

	/// returns the data values as a mutable slice of values casted to the given type.
	#[inline]
	pub fn as_slice_mut<Data>(&mut self) -> &[Data] {
		assert_eq!(
			self.buffer.get_num_components() * self.buffer.get_component_type().size(),
			std::mem::size_of::<Data>(),
		);
		unsafe {
			self.buffer.as_slice_mut::<Data>()
		}
	}

	/// returns the data values as a slice of values casted to the given type.
	/// # Safety:
	/// This function assumes that the buffer's data is properly aligned and matches the type `Data`.
	#[inline]
	pub unsafe fn as_slice_unchecked<Data>(&self) -> &[Data]
	{
		// Safety: upheld
		self.buffer.as_slice::<Data>()
	}

	/// returns the data values as a mutable slice of values casted to the given type.
	/// # Safety:
	/// This function assumes that the buffer's data is properly aligned and matches the type `Data`.
    #[inline]
	pub unsafe fn as_slice_unchecked_mut<Data>(&mut self) -> &mut [Data]
	{
		// Safety: upheld
		self.buffer.as_slice_mut::<Data>()
	}

	/// permutes the data in the buffer according to the given indices, i.e. 
	/// i-th element in the buffer will be moved to indices[i]-th position.
	pub fn permute(&mut self, indices: &[usize]) {
		assert!(
			indices.len() == self.len(),
			"Indices length must match the buffer length: indices.len() = {}, self.len() = {}",
			indices.len(),
			self.len()
		);
		assert!(
			indices.iter().all(|&i| i < self.len()),
			"All indices must be within the buffer length: indices = {:?}, self.len() = {}",
			indices,
			self.len()
		);
		unsafe {
			self.buffer.permute_unchecked(indices);
		}
	}

	/// permutes the data in the buffer according to the given indices, i.e. 
	/// i-th element in the buffer will be moved to indices[i]-th position.
	/// # Safety:
	/// This function assumes that the indices are valid, i.e. they are within the bounds of the buffer.
	pub fn permute_unchecked(&mut self, indices: &[usize]) {
		debug_assert!(
			indices.len() == self.len(),
			"Indices length must match the buffer length: indices.len() = {}, self.len() = {}",
			indices.len(),
			self.len()
		);
		debug_assert!(
			indices.iter().all(|&i| i < self.len()),
			"All indices must be within the buffer length: indices = {:?}, self.len() = {}",
			indices,
			self.len()
		);
		unsafe {
			self.buffer.permute_unchecked(indices);
		}
	}

	/// swaps the elements at indices `i` and `j` in the buffer.
	pub fn swap(&mut self, i: usize, j: usize) {
		assert!(
			i < self.len() && j < self.len(),
			"Indices out of bounds: i = {}, j = {}, len = {}",
			i, j, self.len()
		);
		unsafe {
			self.buffer.swap_unchecked(i, j);
		}
	}
}


#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ComponentDataType {
	F32,
	F64,
	U8,
	U16,
	U32,
	U64,
}

impl ComponentDataType {
	/// returns the size of the data type in bytes e.g. 4 for F32
	pub fn size(self) -> usize {
        match self {
            ComponentDataType::F32 => 4,
            ComponentDataType::F64 => 8,
            ComponentDataType::U8 => 1,
            ComponentDataType::U16 => 2,
            ComponentDataType::U32 => 4,
            ComponentDataType::U64 => 8,
        }
    }
	/// returns unique id for the data type.
	pub fn get_id(self) -> usize {
        match self {
            ComponentDataType::F32 => 0,
            ComponentDataType::F64 => 1,
            ComponentDataType::U8 => 2,
            ComponentDataType::U16 => 3,
            ComponentDataType::U32 => 4,
            ComponentDataType::U64 => 5,
        }
    }

	/// returns the data type from the given id.
	pub fn from_id(id: usize) -> Result<Self, ()> {
		match id {
			0 => Ok(ComponentDataType::F32),
			1 => Ok(ComponentDataType::F64),
			2 => Ok(ComponentDataType::U8),
			3 => Ok(ComponentDataType::U16),
			4 => Ok(ComponentDataType::U32),
			5 => Ok(ComponentDataType::U64),
			_ => Err(()),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AttributeType {
	Position,
	Normal,
	Color,
	TextureCoordinate,
	Tangent,
	Material,
	Joint,
	Weight,
	Connectivity,
	Custom
}

impl AttributeType {
	pub fn get_minimum_dependency(&self) -> Vec<Self> {
		match self {
			Self::Position => Vec::new(),
			Self::Normal => Vec::new(),
			Self::Color => Vec::new(),
			Self::TextureCoordinate => vec![Self::Position, Self::Connectivity],
			Self::Tangent => Vec::new(),
			Self::Material => Vec::new(),
			Self::Joint => Vec::new(),
			Self::Weight => Vec::new(),
			Self::Connectivity => Vec::new(),
			Self::Custom => Vec::new(),
		}
	}

	pub(crate) fn get_id(&self) -> usize {
		match self {
			Self::Position => 0,
			Self::Normal => 1,
			Self::Color => 2,
			Self::TextureCoordinate => 3,
			Self::Tangent => 4,
			Self::Material => 5,
			Self::Joint => 6,
			Self::Weight => 7,
			Self::Connectivity => 8,
			Self::Custom => 9,
		}
	}

	pub(crate) fn from_id(id: usize) -> Self {
		match id {
			0 => Self::Position,
			1 => Self::Normal,
			2 => Self::Color,
			3 => Self::TextureCoordinate,
			4 => Self::Tangent,
			5 => Self::Material,
			6 => Self::Joint,
			7 => Self::Weight,
			8 => Self::Connectivity,
			9 => Self::Custom,
			_ => panic!("Invalid attribute type id"),
		}
	}
}

/// The domain of the attribute, i.e. whether it is defined on the position or corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum AttributeDomain {
	/// The attribute is defined on the position attribute, i.e. i'th element in the attribute is attached to the i'th position in the mesh.
	Position,
	/// The attribute is defined on the corner attribute, i.e. i'th element in the attribute is attached to the i'th corner in the mesh.
	Corner,
}

impl AttributeDomain {
	/// Writes the id of the attribute domain to the writer.
	pub fn write_to<W: ByteWriter>(&self, writer: &mut W) {
		match self {
			Self::Position => writer.write_u8(0),
			Self::Corner => writer.write_u8(1),
		}
	}

	/// Reads the attribute domain from the reader.
	pub fn read_from<R: ByteReader>(reader: &mut R) -> Result<Self, Err> {
		let id = reader.read_u8()?;
		match id {
			0 => Ok(Self::Position),
			1 => Ok(Self::Corner),
			_ => Err(Err::InvalidAttributeDomainId(id)),
		}
	}
}

pub(crate) struct MaybeInitAttribute {
	/// attribute id
	id: AttributeId,

	/// attribute buffer
	buffer: buffer::attribute::MaybeInitAttributeBuffer,

	/// attribute type
	att_type: AttributeType,

	/// attribute domain
	domain: AttributeDomain,
	
	/// the reference of the parent, if any
	parents: Vec<AttributeId>,
}

impl MaybeInitAttribute {
	#[inline]
	pub fn new(
		id: AttributeId, 
		att_type: AttributeType,
		domain: AttributeDomain,
		len: usize, 
		component_type: ComponentDataType,
		num_components: usize,
		parents: Vec<AttributeId>) -> Self {
		let buffer = buffer::attribute::MaybeInitAttributeBuffer::new(
			len,
			component_type,
			num_components,
		);
		Self {
			id,
			buffer,
			parents,
			att_type,
			domain,
		}
	}

	/// Returns the slice of the data in the buffer.
	/// Safety: Callers must know exactly which part of resulting slice is valid. \
	/// Dereferencing the uninitialized part of the slice is undefined behavior.
	/// Moreover, 'num_components * component_type.size()' must equal 'std::mem::size_of::<Data>()'.
	#[inline]
	pub unsafe fn as_slice_unchecked<Data>(&self) -> &[Data]
		where Data: Vector,
	{
		// Safety: upheld
		self.buffer.as_slice_unchecked::<Data>()
	}

	/// Writes the data to the buffer at the specified index.
	#[inline]
	pub fn write<Data>(&mut self, idx:usize, data: Data)
		where Data: Vector,
	{
		self.buffer.write(idx, data);
	}

	/// Writes the data to the buffer at the specified index without checking type and bounds.
	/// # Safety:
	/// (1) The type of the 'data' (i.e. 'Data') must match the initializtion.
	/// (2) The index must be within the bounds of the buffer.
	#[inline]
	#[allow(unused)]
	pub unsafe fn write_type_unchecked<Data>(&mut self, idx:usize, data: Data)
		where Data: Vector,
	{
		self.buffer.write_type_unchecked(idx, data);
	}

	#[inline]
	pub fn get_component_type(&self) -> ComponentDataType {
		self.buffer.get_component_type()
	}

	#[inline]
	pub fn get_num_components(&self) -> usize {
		self.buffer.get_num_components()
	}

	#[inline]
	pub fn len(&self) -> usize {
		self.buffer.len()
	}
}


impl From<MaybeInitAttribute> for Attribute {
	fn from(maybe_init: MaybeInitAttribute) -> Self {
		let buffer = maybe_init.buffer.into();
		Self {
			id: maybe_init.id,
			buffer,
			parents: maybe_init.parents,
			att_type: maybe_init.att_type,
			domain: maybe_init.domain,
			index_map: None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct AttributeId(usize);

impl AttributeId {
    pub(crate) fn new(id: usize) -> Self {
        Self(id)
    }

    /// Returns the id of the attribute.
    pub fn as_usize(&self) -> usize {
        self.0
    }
}


#[cfg(test)]
mod tests {
    use crate::core::shared::NdVector;
    use super::*;


	#[test]
	fn test_attribute() {
		let data = vec![
			NdVector::from([1.0f32, 2.0, 3.0]), 
			NdVector::from([4.0f32, 5.0, 6.0]), 
			NdVector::from([7.0f32, 8.0, 9.0])
		];
		let att = super::Attribute::from(AttributeId::new(0), data.clone(), super::AttributeType::Position, super::AttributeDomain::Position, Vec::new());
		assert_eq!(att.len(), data.len());
		assert_eq!(att.get::<NdVector<3,f32>>(0), data[0], "{:b}!={:b}", att.get::<NdVector<3,f32>>(0).get(0).to_bits(), data[0].get(0).to_bits());
		assert_eq!(att.get_component_type(), super::ComponentDataType::F32);
		assert_eq!(att.get_num_components(), 3);
		assert_eq!(att.get_attribute_type(), super::AttributeType::Position);
	}
}