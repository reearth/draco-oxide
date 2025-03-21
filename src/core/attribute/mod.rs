use super::buffer;
pub struct Attribute {
	buffer: buffer::Buffer,
	
	/// id of the parent, if any
	parent_id: Option<usize>,
	
	/// component_data_type
	component_data_type: ComponentDataType,
	
	/// attribute type
	type_: AttributeType,
	
	/// number of components e.g. 3 for 3D vector
	num_components: usize,
	
	/// number of values e.g. number of points for the position attributes of 
	/// a point cloud.
	len: usize,
	
	/// the byte stride for one attribute value given by 
	/// 'self.component_data_type.size()' * 'self.num_components'
	stride: usize,
}

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