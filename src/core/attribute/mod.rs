use crate::core::shared::Vector;

use super::{buffer, shared::DataValue};
pub struct Attribute {
	buffer: buffer::attribute::AttributeBuffer,
	
	/// id of the parents, if any
	parent_id: Option<usize>,
	
	/// attribute type
	att_type: AttributeType,
}

impl Attribute {
	pub fn from<Data>(data: Vec<Data>, att_type: AttributeType) -> Self 
		where 
			Data: Vector,
	{
		let buffer = buffer::attribute::AttributeBuffer::from(data);
		Self {
			buffer,
			parent_id: None,
			att_type
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

	pub fn get_num_components(&self) -> usize {
		self.buffer.get_num_components()
	}

	pub fn get_attribute_type(&self) -> AttributeType {
		self.att_type
	}

	#[inline(always)]
	pub fn len(&self) -> usize {
		self.buffer.len()
	}

	#[inline]
	/// returns the data values as a slice of casted values to the given type.
	/// # Safety:
    /// This function assumes that the buffer's data is properly aligned and matches the type `Data`.
	pub unsafe fn as_slice_unchecked<Data>(&self) -> &[Data] 
		where 
			Data: Vector,
			Data::Component: DataValue
	{
		// Safety: upheld
		self.buffer.as_slice::<Data>()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
		println!("size of NdVector<3,f32> = {}", std::mem::size_of::<NdVector<3,f32>>());
		let att = super::Attribute::from(data.clone(), super::AttributeType::Position);
		assert_eq!(att.len(), data.len());
		assert_eq!(att.get::<NdVector<3,f32>>(0), data[0], "{:b}!={:b}", att.get::<NdVector<3,f32>>(0).get(0).to_bits(), data[0].get(0).to_bits());
		assert_eq!(att.get_component_type(), super::ComponentDataType::F32);
		assert_eq!(att.get_num_components(), 3);
		assert_eq!(att.get_attribute_type(), super::AttributeType::Position);
		assert_eq!(unsafe{ att.as_slice_unchecked::<NdVector<3,f32>>() }, &data[..]);
	}
}