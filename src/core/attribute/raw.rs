use crate::core::buffer;
use crate::core::shared::{DataValue, Vector};
use crate::core::attribute::AttributeType;
use crate::core::attribute::ComponentDataType;

pub struct Attribute {
	buffer: buffer::attribute::AttributeBuffer,
	
	/// id of the parents, if any
	parent_ids: Vec<usize>,
	
	/// attribute type
	att_type: AttributeType,
}

impl Attribute {
    pub fn from<Data>(data: Vec<Data>, att_type: AttributeType) -> Self 
        where Data: Vector,
    {
        let buffer = buffer::attribute::AttributeBuffer::from(data);
        Self {
            buffer,
            parent_ids: Vec::new(),
            att_type
        }
    }

    pub fn from_faces(data: Vec<[usize; 3]>) -> Self {
        let buffer = buffer::attribute::AttributeBuffer::from(data);
        Self {
            buffer,
            parent_ids: Vec::new(),
            att_type: AttributeType::Connectivity,
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

    pub fn get_parent_ids(&self) -> &[usize] {
        &self.parent_ids
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }
	#[inline]
	/// returns the data values as a slice of values casted to the given type.
	/// # Safety:
    /// This function assumes that the buffer's data is properly aligned and matches the type `Data`.
	pub unsafe fn as_slice_unchecked<Data>(&self) -> &[Data]
	{
		// Safety: upheld
		self.buffer.as_slice::<Data>()
	}

    #[inline]
	/// returns the data values as a mutable slice of values casted to the given type.
	/// # Safety:
    /// This function assumes that the buffer's data is properly aligned and matches the type `Data`.
	pub unsafe fn as_slice_unchecked_mut<Data>(&mut self) -> &mut [Data]
	{
		// Safety: upheld
		self.buffer.as_slice_mut::<Data>()
	}

    pub(crate) fn from_with_parent_ids(att: super::Attribute, ids: Vec<usize>) -> Self {
        Self {
            buffer: att.buffer,
            parent_ids: ids,
            att_type: att.att_type,
        }
    }

    pub(crate) fn from_parentless(att: super::Attribute) -> Self {
        Self {
            buffer: att.buffer,
            parent_ids: Vec::new(),
            att_type: att.att_type,
        }
    }
}