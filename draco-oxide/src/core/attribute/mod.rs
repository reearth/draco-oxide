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
	#[error("Invalid DataTypeId: {0}")]
	InvalidDataTypeId(u8),
}

/// Represents an attribute in a mesh. An attribute can be an array of values representing potisions
/// of vertices, or it can be an array of values representing normals of vertices or corners, or faces.
/// Note that the struct does not have the static type information, so the attribute value can be a 
/// vector of any type of any dimension, as long as it implements the `Vector` trait. The information about 
/// the type of the attribute, component type, and the number of components is stored in dynamically.
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

	/// an optional mapping from vertex index to the attribute value index in attribute.
	vertex_to_att_val_map: Option<Vec<usize>>,

	/// name of the attribute, if any
	name: Option<String>,
}

impl Attribute {
	pub fn new<Data, const N: usize>(data: Vec<Data>, att_type: AttributeType, domain: AttributeDomain, parents: Vec<AttributeId>) -> Self 
		where 
			Data: Vector<N>,
	{
		let id = AttributeId::new(0); // TODO: generate unique id
		let buffer = buffer::attribute::AttributeBuffer::from_vec(data);
		let mut out = Self {
			id,
			buffer,
			parents,
			att_type,
			domain,
			vertex_to_att_val_map: None,
			name: None,
		};
		out.remove_duplicate_values::<Data, N>();
		out
	}

	pub fn new_empty(id: AttributeId, att_type: AttributeType, domain: AttributeDomain, component_type: ComponentDataType, num_components: usize) -> Self {
		let buffer = buffer::attribute::AttributeBuffer::new(
			component_type,
			num_components,
		);
		Self {
			id,
			buffer,
			parents: Vec::new(),
			att_type,
			domain,
			vertex_to_att_val_map: None,
			name: None,
		}
	}

	pub(crate) fn from<Data, const N: usize>(id: AttributeId, data: Vec<Data>, att_type: AttributeType, domain: AttributeDomain, parents: Vec<AttributeId>) -> Self 
		where 
			Data: Vector<N>,
	{
		let buffer = buffer::attribute::AttributeBuffer::from_vec(data);
		let mut out = Self {
			id,
			buffer,
			parents,
			att_type,
			domain,
			vertex_to_att_val_map: None,
			name: None,
		};
		out.remove_duplicate_values::<Data, N>();
		out
	}
	
	pub(crate) fn from_without_removing_duplicates<Data, const N: usize>(id: AttributeId, data: Vec<Data>, att_type: AttributeType, domain: AttributeDomain, parents: Vec<AttributeId>) -> Self 
		where 
			Data: Vector<N>,
	{
		let buffer = buffer::attribute::AttributeBuffer::from_vec(data);
		let out = Self {
			id,
			buffer,
			parents,
			att_type,
			domain,
			vertex_to_att_val_map: None,
			name: None,
		};
		out
	}

	pub fn get<Data, const N: usize>(&self, v_idx: usize) -> Data 
		where 
			Data: Vector<N>,
			Data::Component: DataValue
	{
		self.buffer.get(self.get_att_idx(v_idx))
	}

	pub fn get_unique_val<Data, const N: usize>(&self, val_idx: usize) -> Data 
		where 
			Data: Vector<N>,
			Data::Component: DataValue
	{
		self.buffer.get(val_idx)
	}

	pub fn get_component_type(&self) -> ComponentDataType {
		self.buffer.get_component_type()
	}

	#[inline]
	#[allow(unused)]
	pub(crate) fn set_component_type(&mut self, component_type: ComponentDataType) {
		self.buffer.set_component_type(component_type);
	}

	#[inline]
	#[allow(unused)]
	pub(crate) fn set_num_components(&mut self, num_components: usize) {
		self.buffer.set_num_components(num_components);
	}

	pub(crate) fn get_data_as_bytes(&self) -> &[u8] {
		self.buffer.as_slice_u8()
	}

	#[inline]
	#[allow(unused)]
	pub(crate) fn get_as_bytes(&self, i: usize) -> &[u8] {
		&self.buffer.as_slice_u8()[
			i * self.buffer.get_num_components() * self.buffer.get_component_type().size()..
			(i + 1) * self.buffer.get_num_components() * self.buffer.get_component_type().size()
		]
	}

	pub(crate) fn set_vertex_to_att_val_map(&mut self, vertex_to_att_val_map: Option<Vec<usize>>) {
		self.vertex_to_att_val_map = vertex_to_att_val_map;
	}

	pub(crate) fn take_vertex_to_att_val_map(self) -> Option<Vec<usize>> {
		self.vertex_to_att_val_map
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
		if let Some(f) = &self.vertex_to_att_val_map {
			f.len()
		} else {
			self.buffer.len()
		}
	}

	#[inline(always)]
	pub fn num_unique_values(&self) -> usize {
		self.buffer.len()
	}

	#[inline]
	pub fn get_att_idx(&self, idx: usize) -> usize {
		assert!(
			idx < self.len(),
			"Index out of bounds: idx = {}, len = {}",
			idx,
			self.len()
		);
		if let Some(ref vertex_to_att_val_map) = self.vertex_to_att_val_map {
			vertex_to_att_val_map[idx]
		} else {
			// otherwise, we use identity mapping
			idx
		}
	}

	#[inline]
	pub fn set_name(&mut self, name: String) {
		self.name = Some(name);
	}

	#[inline]
	pub fn get_name(&self) -> Option<&String> {
		self.name.as_ref()
	}

	/// returns the data values as a slice of values casted to the given type.
	#[inline]
	pub fn unique_vals_as_slice<Data>(&self) -> &[Data] {
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
	pub fn unique_vals_as_slice_mut<Data>(&mut self) -> &mut [Data] {
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
	pub unsafe fn unique_vals_as_slice_unchecked<Data>(&self) -> &[Data]
	{
		// Safety: upheld
		self.buffer.as_slice::<Data>()
	}

	/// returns the data values as a mutable slice of values casted to the given type.
	/// # Safety:
	/// This function assumes that the buffer's data is properly aligned and matches the type `Data`.
    #[inline]
	pub unsafe fn unique_vals_as_slice_unchecked_mut<Data>(&mut self) -> &mut [Data]
	{
		// Safety: upheld
		self.buffer.as_slice_mut::<Data>()
	}

	/// permutes the data in the buffer according to the given indices, i.e. 
	/// `i`-th element in the buffer will be moved to `indices[i]`-th position.
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
	/// `i`-th element in the buffer will be moved to `indices[i]`-th position.
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

	pub fn take_values<Data, const N: usize>(self) -> Vec<Data>
		where Data: Vector<N>,
	{
		assert_eq!(
			self.get_num_components(), N,
		);
		assert_eq!(
			self.get_component_type(), Data::Component::get_dyn(),
		);
		
		unsafe {
			self.buffer.into_vec_unchecked::<Data, N>()
		}
	}


	pub fn into_parts<Data, const N: usize>(mut self) -> (Vec<Data>, Option<Vec<usize>>, Self)
		where Data: Vector<N>,
	{
		let num_components = self.get_num_components();
		let component_type = self.get_component_type();
		assert_eq!(
			num_components, N,
		);
		assert_eq!(
			component_type, Data::Component::get_dyn(),
		);
		let mut new_buffer = buffer::attribute::AttributeBuffer::from_vec(
			Vec::<Data>::new()
		);
		std::mem::swap(&mut self.buffer, &mut new_buffer);
		let data = unsafe {
			new_buffer.into_vec_unchecked::<Data, N>()
		};

		let mut vertex_to_att_val_map = None;
		std::mem::swap(&mut vertex_to_att_val_map, &mut self.vertex_to_att_val_map);

		(data, vertex_to_att_val_map, self)
	}

	pub fn set_values<Data, const N: usize>(&mut self, data: Vec<Data>)
		where Data: Vector<N>,
	{
		assert_eq!(
			self.get_num_components(), N,
		);
		assert_eq!(
			self.get_component_type(), Data::Component::get_dyn(),
		);
		assert_eq!( self.len(), 0 );
		self.buffer = buffer::attribute::AttributeBuffer::from_vec(data);
	}

	pub fn remove_duplicate_values<Data, const N: usize>(&mut self) 
		where Data: Vector<N>,
	{
		let mut duplicate_indeces = Vec::new();
		// start with identity mapping
		let mut vertex_to_att_val_map = (0..self.len()).collect::<Vec<_>>();
		for (i, val) in self.unique_vals_as_slice::<Data>().iter().enumerate() {
			if i == self.len() - 1 {
				// last element, no need to check for duplicates
				break;
			}
			if duplicate_indeces.contains(&i) {
				// already processed this value
				continue;
			}
			let mut local_duplicate_indeces = Vec::new();
			for (j, other_val) in self.unique_vals_as_slice::<Data>()[i+1..].iter().enumerate() {
				if val == other_val {
					local_duplicate_indeces.push(i+1 + j);
				}
			}
			if local_duplicate_indeces.is_empty() {
				continue;
			}
			
			for &duplicate_idx in local_duplicate_indeces.iter() {
				// update the mapping
				vertex_to_att_val_map[duplicate_idx] = i;
			}
			duplicate_indeces.extend(local_duplicate_indeces);
		}
		let mut curr_max = 0;
		for i in 0..vertex_to_att_val_map.len() {
			let val_idx = vertex_to_att_val_map[i];
			if val_idx == curr_max+1 {
				// no gap
				curr_max += 1;
			} else if val_idx > curr_max+1 {
				// gap found, update the index
				curr_max += 1;
				for j in i..vertex_to_att_val_map.len() {
					if vertex_to_att_val_map[j] == val_idx {
						vertex_to_att_val_map[j] = curr_max;
					}
				}
			}
		}
		if !duplicate_indeces.is_empty() {
			self.vertex_to_att_val_map = Some(vertex_to_att_val_map);
		}
		// remove the duplicates from the buffer
		duplicate_indeces.sort_unstable();
		for i in duplicate_indeces.into_iter().rev() {
			self.buffer.remove::<Data, N>(i);
		}
	}

	pub(crate) fn remove<Data, const N: usize>(&mut self, v_idx: usize) {
		assert!(v_idx < self.len(), "Value index out of bounds: {}", v_idx);
		if let Some(ref mut vertex_to_att_val_map) = self.vertex_to_att_val_map {
			// update the mapping
			if (0..vertex_to_att_val_map.len())
				.filter(|&v| v!=v_idx)
				.any(|v| vertex_to_att_val_map[v]==vertex_to_att_val_map[v_idx]) 
			{
				// if there are other vertices with the same value, we just remove the mapping
				vertex_to_att_val_map.remove(v_idx);
			} else {
				let removed_unique_val_idx = vertex_to_att_val_map.remove(v_idx);
				self.buffer.remove::<Data, N>(removed_unique_val_idx);
				// update the mapping for the remaining vertices
				for i in 0..vertex_to_att_val_map.len() {
					if vertex_to_att_val_map[i] > removed_unique_val_idx {
						vertex_to_att_val_map[i] -= 1;
					}
				}
			}
		} else {
			// no mapping, just remove the value
			self.remove_unique_val::<Data, N>(v_idx);
		}
	}

	pub(crate) fn remove_dyn(&mut self, v_idx: usize) {
		assert!(v_idx < self.len(), "Value index out of bounds: {}", v_idx);
		match self.get_component_type().size() * self.get_num_components() {
			1 => self.remove::<u8, 1>(v_idx),
			2 => self.remove::<u16, 1>(v_idx),
			4 => self.remove::<u32, 1>(v_idx),
			6 => self.remove::<u16, 3>(v_idx),
			8 => self.remove::<u64, 1>(v_idx),
			12 => self.remove::<u32, 3>(v_idx),
			16 => self.remove::<u64, 2>(v_idx),
			18 => self.remove::<u64, 3>(v_idx),
			_ => panic!("Unsupported component size: {}", self.get_component_type().size()),
		}
	}

	pub(crate) fn remove_unique_val<Data, const N: usize>(&mut self, val_idx: usize) 
	{
		assert!(val_idx < self.num_unique_values(), "Value index out of bounds: {}", val_idx);
		self.buffer.remove::<Data, N>(val_idx);
		if let Some(ref mut _vertex_to_att_val_map) = self.vertex_to_att_val_map {
			unimplemented!();
		}
	}

	pub fn remove_unique_val_dyn(&mut self, val_idx: usize) 
	{
		assert!(val_idx < self.num_unique_values(), "Value index out of bounds: {}", val_idx);
		match self.get_component_type().size() * self.get_num_components() {
			1 => self.buffer.remove::<u8, 1>(val_idx),
			2 => self.buffer.remove::<u16, 1>(val_idx),
			4 => self.buffer.remove::<u32, 1>(val_idx),
			6 => self.buffer.remove::<u16, 3>(val_idx),
			8 => self.buffer.remove::<u64, 1>(val_idx),
			12 => self.buffer.remove::<u32, 3>(val_idx),
			16 => self.buffer.remove::<u64, 2>(val_idx),
			18 => self.buffer.remove::<u64, 3>(val_idx),
			_ => panic!("Unsupported component size: {}", self.get_component_type().size()),
		}
	}
}


#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum ComponentDataType {
	I8,
	U8,
	I16,
	U16,
	I32,
	U32,
	I64,
	U64,
	F32,
	F64,
	Invalid,
}

impl ComponentDataType {
	/// returns the size of the data type in bytes e.g. 4 for F32
	#[inline]
	pub fn size(self) -> usize {
        match self {
            ComponentDataType::F32 => 4,
            ComponentDataType::F64 => 8,
            ComponentDataType::U8 => 1,
            ComponentDataType::U16 => 2,
            ComponentDataType::U32 => 4,
            ComponentDataType::U64 => 8,
			ComponentDataType::I8 => 1,
			ComponentDataType::I16 => 2,
			ComponentDataType::I32 => 4,
			ComponentDataType::I64 => 8,
			ComponentDataType::Invalid => 0
        }
    }

	#[inline]
	pub fn is_float(self) -> bool {
		matches!(self, ComponentDataType::F32 | ComponentDataType::F64)
	}
	
	/// returns unique id for the data type.
	#[inline]
	pub fn get_id(self) -> u8 {
        match self {
            ComponentDataType::U8 => 1,
			ComponentDataType::I8 => 2,
            ComponentDataType::U16 => 3,
			ComponentDataType::I16 => 4,
            ComponentDataType::U32 => 5,
			ComponentDataType::I32 => 6,
            ComponentDataType::U64 => 7,
			ComponentDataType::I64 => 8,
            ComponentDataType::F32 => 9,
            ComponentDataType::F64 => 10,
			ComponentDataType::Invalid => u8::MAX, // Invalid type
        }
	}

	/// returns the data type as a string.
	#[inline]
	pub fn write_to<W: ByteWriter>(self, writer: &mut W) {
		writer.write_u8(self.get_id());
	}

	/// returns the data type from the given id.
	#[inline]
	pub fn from_id(id: usize) -> Result<Self, ()> {
		match id {
			1 => Ok(ComponentDataType::I8),
			2 => Ok(ComponentDataType::U8),
			3 => Ok(ComponentDataType::I16),
			4 => Ok(ComponentDataType::U16),
			5 => Ok(ComponentDataType::I32),
			6 => Ok(ComponentDataType::U32),
			7 => Ok(ComponentDataType::I64),
			8 => Ok(ComponentDataType::U64),
			9 => Ok(ComponentDataType::F32),
			10 => Ok(ComponentDataType::F64),
			_ => Err(()),
		}
	}

	/// Reads the data type from the reader.
	#[inline]
	pub fn read_from<R: ByteReader>(reader: &mut R) -> Result<Self, Err> {
		let id = reader.read_u8()?;
		Self::from_id(id as usize).map_err(|_| Err::InvalidDataTypeId(id))
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum AttributeType {
	Position,
	Normal,
	Color,
	TextureCoordinate,
	Custom,
	Tangent,
	Material,
	Joint,
	Weight,
	Invalid,
}

impl AttributeType {
	pub fn get_minimum_dependency(&self) -> Vec<Self> {
		match self {
			Self::Position => Vec::new(),
			Self::Normal => Vec::new(),
			Self::Color => Vec::new(),
			Self::TextureCoordinate => vec![Self::Position],
			Self::Tangent => Vec::new(),
			Self::Material => Vec::new(),
			Self::Joint => Vec::new(),
			Self::Weight => Vec::new(),
			Self::Custom => Vec::new(),
			Self::Invalid => Vec::new(),
		}
	}

	/// Returns the id of the attribute type.
	#[inline]
	pub(crate) fn get_id(&self) -> u8 {
		match self {
			Self::Position => 0,
			Self::Normal => 1,
			Self::Color => 2,
			Self::TextureCoordinate => 3,
			Self::Custom => 4,
			Self::Tangent => 5,
			Self::Material => 6,
			Self::Joint => 7,
			Self::Weight => 8,
			Self::Invalid => u8::MAX, // Invalid type
		}
	}

	/// Returns the id of the attribute type.
	#[inline]
	pub fn write_to<W: ByteWriter>(&self, writer: &mut W) {
		writer.write_u8(self.get_id());
	}

	/// Reads the attribute type from the reader.
	#[inline]
	pub(crate) fn from_id(id: u8) -> Result<Self, Err> {
		match id {
			0 => Ok(Self::Position),
			1 => Ok(Self::Normal),
			2 => Ok(Self::Color),
			3 => Ok(Self::TextureCoordinate),
			4 => Ok(Self::Custom),
			5 => Ok(Self::Tangent),
			6 => Ok(Self::Material),
			7 => Ok(Self::Joint),
			8 => Ok(Self::Weight),
			_ => Err(Err::InvalidDataTypeId(id)),
		}
	}

	/// Reads the attribute type from the reader.
	#[inline]
	pub fn read_from<R: ByteReader>(reader: &mut R) -> Result<Self, Err> {
		let id = reader.read_u8()?;
		Self::from_id(id)
	}
}

/// The domain of the attribute, i.e. whether it is defined on the position or corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum AttributeDomain {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct AttributeId(usize);

impl AttributeId {
    pub fn new(id: usize) -> Self {
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
		assert_eq!(att.get::<NdVector<3,f32>, 3>(0), data[0], "{:b}!={:b}", att.get::<NdVector<3,f32>,3>(0).get(0).to_bits(), data[0].get(0).to_bits());
		assert_eq!(att.get_component_type(), super::ComponentDataType::F32);
		assert_eq!(att.get_num_components(), 3);
		assert_eq!(att.get_attribute_type(), super::AttributeType::Position);
	}

	#[test]
	fn test_attribute_remap() {
	    let positions = vec![
            NdVector::from([0.0f32, 0.0, 0.0]),  // vertex 0 (unique)
            NdVector::from([1.0f32, 0.0, 0.0]),  // vertex 1 (unique)
            NdVector::from([0.5f32, 1.0, 0.0]),  // vertex 2 (unique)
            NdVector::from([0.0f32, 0.0, 0.0]),  // vertex 3 (duplicate of vertex 0)
            NdVector::from([1.0f32, 0.0, 0.0]),  // vertex 4 (duplicate of vertex 1)
            NdVector::from([2.0f32, 0.0, 0.0]),  // vertex 5 (unique)
        ];
        
        let att = Attribute::new(
            positions,
            AttributeType::Position,
            AttributeDomain::Position,
            vec![],
        );

		assert_eq!(
			att.vertex_to_att_val_map.unwrap(),
			vec![0, 1, 2, 0, 1, 3],
		)
	}

	#[test]
	fn test_remove() {
		let positions = vec![
			NdVector::from([0.0f32, 0.0, 0.0]),  // vertex 0 (unique)
			NdVector::from([1.0f32, 0.0, 0.0]),  // vertex 1 (unique)
			NdVector::from([2.0f32, 0.0, 0.0]),  // vertex 2 (unique)
			NdVector::from([3.0f32, 0.0, 0.0]),  // vertex 3 (unique)
			NdVector::from([2.0f32, 0.0, 0.0]),  // vertex 4 (duplicate of vertex 2)
			NdVector::from([5.0f32, 0.0, 0.0]),  // vertex 5 (unique)
		];

		let mut att = Attribute::new(
			positions,
			AttributeType::Position,
			AttributeDomain::Position,
			vec![],
		);

		assert_eq!(att.len(), 6);
		assert_eq!(att.num_unique_values(), 5);
		assert_eq!(att.vertex_to_att_val_map, Some(vec![0, 1, 2, 3, 2, 4]));
		att.remove::<NdVector<3,f32>, 3>(2); // remove vertex 2
		assert_eq!(att.len(), 5);
		assert_eq!(att.num_unique_values(), 5);
		assert_eq!(att.vertex_to_att_val_map, Some(vec![0, 1, 3, 2, 4]));
		att.remove::<NdVector<3,f32>, 3>(1); // remove vertex 1
		assert_eq!(att.len(), 4);
		assert_eq!(att.num_unique_values(), 4);
		assert_eq!(att.vertex_to_att_val_map, Some(vec![0, 2, 1, 3]));
	}
}