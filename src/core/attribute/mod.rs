use crate::core::shared::Vector;

use super::{buffer, shared::DataValue};
pub struct Attribute {
	buffer: buffer::attribute::AttributeBuffer,
	
	/// id of the parents, if any
	parent_id: Option<usize>,
	
	/// attribute type
	type_: AttributeType,
}

impl Attribute {
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

	pub fn get_num_components(&self) -> usize {
		self.buffer.get_num_components()
	}

	#[inline(always)]
	pub fn len(&self) -> usize {
		self.buffer.len()
	}
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentDataType {
	F32,
	F64,
	U8,
	U16,
	U32,
	U64,
}

impl ComponentDataType {
	/// returns the size of the data type e.g. 32 for F32
	pub fn size(self) -> usize {
        match self {
            ComponentDataType::F32 => 32,
            ComponentDataType::F64 => 64,
            ComponentDataType::U8 => 8,
            ComponentDataType::U16 => 16,
            ComponentDataType::U32 => 32,
            ComponentDataType::U64 => 64,
        }
    }
	/// returns unique id for the data type.
	pub fn id(self) -> usize {
        match self {
            ComponentDataType::F32 => 0,
            ComponentDataType::F64 => 1,
            ComponentDataType::U8 => 2,
            ComponentDataType::U16 => 3,
            ComponentDataType::U32 => 4,
            ComponentDataType::U64 => 5,
        }
    }
}

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