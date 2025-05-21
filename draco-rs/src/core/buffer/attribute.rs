use std::{ptr, mem};
use serde::ser::SerializeSeq;
use serde::Serialize;

use crate::core::shared::DataValue;
use crate::core::attribute::ComponentDataType;
use crate::core::shared::Vector;

use super::RawBuffer;

pub(crate) struct AttributeBuffer {
    /// Contains the data of the attribute.
    data: RawBuffer,

    /// The number of values of the attribute.
    len: usize,

    /// pointer to the last element.
    #[allow(unused)]
    last: *mut u8,

    /// component type of the attribute.
    component_type: ComponentDataType,

    num_components: usize,
}


impl AttributeBuffer {
    fn as_ptr(&self) -> *mut u8 {
        self.data.as_ptr()
    }


    pub(crate) fn get<Data>(&self, idx: usize) -> Data 
        where 
            Data: Vector,
            Data::Component: DataValue
    {
        assert!(
            size_of::<Data>() == self.component_type.size() * self.num_components, 
            "Cannot read from buffer: Trying to read {}, but the buffer stores the elements of type {} with {} components", 
            size_of::<Data>(), self.component_type.size(), self.num_components
        );
        assert!(idx < self.len, "Index out of bounds: The index {} is out of bounds for the attribute buffer with length {}", idx, self.len);
        // just checked the condition
        unsafe{ self.get_unchecked::<Data>(idx) }
    }

    /// # Safety:
    /// Two checks are ignored in this function:
    /// (1) 'std::mem::size_of::<Data>()==component.size() * num_components', and
    /// (2) idx < self.len
    pub(crate) unsafe fn get_unchecked<Data>(&self, idx: usize) -> Data 
        where 
            Data: Vector,
            Data::Component: DataValue
    {
        debug_assert!(
            size_of::<Data>() == self.component_type.size() * self.num_components, 
            "Cannot read from buffer: Trying to read {}, but the buffer stores the elements of type {} with {} components", 
            size_of::<Data>(), self.component_type.size(), self.num_components
        );
        debug_assert!(idx < self.len, "Index out of bounds: The index {} is out of bounds for the attribute buffer with length {}", idx, self.len);
        let size = mem::size_of::<Data>();
        let ptr = unsafe{ self.as_ptr().add(size * idx) };
        // Safety: upheld
        ptr::read(ptr as *const Data)
    }

    pub(crate) fn get_component_type(&self) -> ComponentDataType {
        self.component_type
    }

    pub(crate) fn get_num_components(&self) -> usize {
        self.num_components
    }

    #[allow(unused)]
    pub(crate) fn push<Data>(&mut self, data: Data) 
        where 
            Data: Vector,
            Data::Component: DataValue
    {
        assert_eq!(
            Data::Component::get_dyn(), self.component_type, 
            "Data type mismatch: Cannot push data of type {:?} into attribute buffer of type {:?}", 
            Data::Component::get_dyn(), self.component_type
        );
        assert!(
            Data::NUM_COMPONENTS == self.num_components,
            "Number of components mismatch: Cannot push data with {} components into attribute buffer with {} components",
            Data::NUM_COMPONENTS, self.num_components
        );
        unsafe {
            self.push_type_unchecked(data);
        }
    }

    /// pushes a value into the buffer without checking the type and the number of components.
    /// # Safety
    /// This function is unsafe because it does not check the type and the number of components of the data.
    pub(crate) unsafe fn push_type_unchecked<Data>(&mut self, data: Data) 
        where 
            Data: Vector,
            Data::Component: DataValue
    {
        debug_assert_eq!(
            Data::Component::get_dyn(), self.component_type, 
            "Unsafe Condition Failed: Data type mismatch: Cannot push data of type {:?} into attribute buffer of type {:?}", 
            Data::Component::get_dyn(), self.component_type
        );
        debug_assert!(
            Data::NUM_COMPONENTS == self.num_components,
            "Unsafe Condition Failed: Number of components mismatch: Cannot push data with {} components into attribute buffer with {} components",
            Data::NUM_COMPONENTS, self.num_components
        );
    

        self.len += 1;
        if self.len * size_of::<Data>() > self.data.cap {
            self.data.double();
        }

        ptr::write(self.last as *mut Data, data);
    }

	#[inline(always)]
    /// Returns the number of values of the attribute.
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    /// Returns a slice of all the values in the buffer casted to the static type `Data`.
    /// # Safety
    /// This function assumes that the buffer's data is properly aligned and matches the type `Data`.
    pub(crate) unsafe fn as_slice<Data>(&self) -> &[Data] {
        debug_assert!(
            mem::size_of::<Data>() == self.component_type.size() * self.num_components,
            "Cannot create slice: Trying to cast to {}, but the buffer stores elements of type {}D vector of {:?}, which has size {}",
            mem::size_of::<Data>(),
            self.num_components,
            self.component_type,
            self.component_type.size(),
        );

        
        std::slice::from_raw_parts(
            self.as_ptr() as *const Data,
            self.len,
        )
    }

    #[inline]
    /// Returns the mutable slice of all the values in the buffer casted to the static type `Data`.
    /// # Safety
    /// This function assumes that the buffer's data is properly aligned and matches the type `Data`.
    pub(crate) unsafe fn as_slice_mut<Data>(&mut self) -> &mut [Data] {
        debug_assert!(
            mem::size_of::<Data>() == self.component_type.size() * self.num_components,
            "Cannot create slice: Trying to cast to {}, but the buffer stores elements of type {}D vector of {:?}, which has size {}",
            mem::size_of::<Data>(),
            self.num_components,
            self.component_type,
            self.component_type.size(),
        );

        
        std::slice::from_raw_parts_mut(
            self.as_ptr() as *mut Data,
            self.len,
        )
    }

    #[allow(unused)]
    pub unsafe fn into_vec<Data>(self) -> Vec<Data> 
        where Data: Vector,
    {
        assert_eq!(
            Data::Component::get_dyn(), self.component_type, 
            "Data type mismatch: Cannot push data of type {:?} into attribute buffer of type {:?}", 
            Data::Component::get_dyn(), self.component_type
        );
        assert!(
            Data::NUM_COMPONENTS == self.num_components,
            "Number of components mismatch: Cannot push data with {} components into attribute buffer with {} components",
            Data::NUM_COMPONENTS, self.num_components
        );

        self.into_vec_unchecked()
    }

    pub unsafe fn into_vec_unchecked<Data>(self) -> Vec<Data> 
        where Data: Vector,
    {
        debug_assert_eq!(
            Data::Component::get_dyn(), self.component_type, 
            "Data type mismatch: Cannot push data of type {:?} into attribute buffer of type {:?}", 
            Data::Component::get_dyn(), self.component_type
        );
        debug_assert!(
            Data::NUM_COMPONENTS == self.num_components,
            "Number of components mismatch: Cannot push data with {} components into attribute buffer with {} components",
            Data::NUM_COMPONENTS, self.num_components
        );

        unsafe {
            let slice = self.as_slice::<Data>();
            Vec::from_raw_parts(slice.as_ptr() as *mut Data, self.len, self.len)
        }
    }

    #[inline]
    /// Returns a slice of all the values in the buffer casted to the static type `u8`.
    unsafe fn as_slice_u8(&self) -> &[u8] {
        std::slice::from_raw_parts(
            self.as_ptr(),
            self.len * self.num_components * self.component_type.size(),
        )
    }

    /// #Safety
    /// This function assumes that the permutation is welll-defined in the sense that
    /// (1) it has the same length as the buffer,
    /// (2) its elements are distinct.
    pub fn permute(&mut self, permutation: &[usize]) {
        debug_assert_eq!(self.len, permutation.len(), "Permutation length does not match the buffer length");
        debug_assert!(
            {
                let mut p = permutation.to_vec();
                p.sort();
                p.into_iter().enumerate().all(|(i, x)| i == x)
            },
            "Permutation is not a valid permutation: This violates the safety contract of the function, so need to get resolved immediately",
        );
        let mut tmp_att = self.clone();

        let elem_size = self.num_components * self.component_type.size();
        for (i, &new_idx) in permutation.iter().enumerate() {
            // Copy the value at self[i] to tmp_att[new_idx]
            // We need to copy the raw bytes for each element.
            let src = unsafe { self.as_ptr().add(i * elem_size) };
            let dst = unsafe { tmp_att.as_ptr().add(new_idx * elem_size) };
            unsafe {
                std::ptr::copy_nonoverlapping(src, dst, elem_size);
            }
        }
        mem::swap(self, &mut tmp_att);
    }
}


impl Serialize for AttributeBuffer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
    {
        let mut s = serializer.serialize_seq(Some(self.len))?;
        match self.component_type {
            ComponentDataType::U8 => {
                match self.num_components {
                    1 => s.serialize_element(unsafe{ self.as_slice::<[u8;1]>() }),
                    2 => s.serialize_element(unsafe{ self.as_slice::<[u8;2]>() }),
                    3 => s.serialize_element(unsafe{ self.as_slice::<[u8;3]>() }),
                    4 => s.serialize_element(unsafe{ self.as_slice::<[u8;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::U16 => {
                match self.num_components {
                    1 => s.serialize_element(unsafe{ self.as_slice::<[u16;1]>() }),
                    2 => s.serialize_element(unsafe{ self.as_slice::<[u16;2]>() }),
                    3 => s.serialize_element(unsafe{ self.as_slice::<[u16;3]>() }),
                    4 => s.serialize_element(unsafe{ self.as_slice::<[u16;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::U32 => {
                match self.num_components {
                    1 => s.serialize_element(unsafe{ self.as_slice::<[u32;1]>() }),
                    2 => s.serialize_element(unsafe{ self.as_slice::<[u32;2]>() }),
                    3 => s.serialize_element(unsafe{ self.as_slice::<[u32;3]>() }),
                    4 => s.serialize_element(unsafe{ self.as_slice::<[u32;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::U64 => {
                match self.num_components {
                    1 => s.serialize_element(unsafe{ self.as_slice::<[u64;1]>() }),
                    2 => s.serialize_element(unsafe{ self.as_slice::<[u64;2]>() }),
                    3 => s.serialize_element(unsafe{ self.as_slice::<[u64;3]>() }),
                    4 => s.serialize_element(unsafe{ self.as_slice::<[u64;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::F32 => {
                match self.num_components {
                    1 => s.serialize_element(unsafe{ self.as_slice::<[f32;1]>() }),
                    2 => s.serialize_element(unsafe{ self.as_slice::<[f32;2]>() }),
                    3 => s.serialize_element(unsafe{ self.as_slice::<[f32;3]>() }),
                    4 => s.serialize_element(unsafe{ self.as_slice::<[f32;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::F64 => {
                match self.num_components {
                    1 => s.serialize_element(unsafe{ self.as_slice::<[f64;1]>() }),
                    2 => s.serialize_element(unsafe{ self.as_slice::<[f64;2]>() }),
                    3 => s.serialize_element(unsafe{ self.as_slice::<[f64;3]>() }),
                    4 => s.serialize_element(unsafe{ self.as_slice::<[f64;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
        }?;
        s.end()
    }
}


impl<Data> From<Vec<Data>> for AttributeBuffer 
    where 
        Data: Vector,
        Data::Component: DataValue
{
    fn from(data: Vec<Data>) -> Self {
        let component_type = Data::Component::get_dyn();
        let num_components = Data::NUM_COMPONENTS;
        let len = data.len();
        let buffer = RawBuffer::from_vec(data);
        let last = unsafe {
            buffer.as_ptr().add(len * mem::size_of::<Data>())
        };

        Self {
            data: buffer,
            len,
            last,
            component_type,
            num_components,
        }
    }
}

impl From<Vec<[usize; 3]>> for AttributeBuffer {
    fn from(data: Vec<[usize;3]>) -> Self {
        // The size of usize is platform dependent, so we need to check it at runtime.
        let component_type = match mem::size_of::<usize>() {
            2 => ComponentDataType::U16,
            4 => ComponentDataType::U32,
            8 => ComponentDataType::U64,
            _ => panic!("Unsupported size for usize: {}", mem::size_of::<usize>()),
            
        };

        let num_components = 3;
        let len = data.len();
        let buffer = RawBuffer::from_vec(data);
        let last = unsafe {
            buffer.as_ptr().add(len * mem::size_of::<[usize; 3]>())
        };

        Self {
            data: buffer,
            len,
            last,
            component_type,
            num_components,
        }
    }
}

impl Clone for AttributeBuffer {
    fn clone(&self) -> Self {
        let data = unsafe { self.as_slice_u8().to_vec() };
        let component_type = self.component_type;
        let num_components = self.num_components;
        let len = self.len;
        let buffer = RawBuffer::from_vec(data);
        let last = unsafe {
            buffer.as_ptr().add(len * mem::size_of::<u8>())
        };

        Self {
            data: buffer,
            len,
            last,
            component_type,
            num_components,
        }
    }
}

impl std::fmt::Debug for AttributeBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = match self.component_type {
            ComponentDataType::U8 => {
                match self.num_components {
                    1 => format!("{:?}", unsafe{ self.as_slice::<[u8;1]>() }),
                    2 => format!("{:?}", unsafe{ self.as_slice::<[u8;2]>() }),
                    3 => format!("{:?}", unsafe{ self.as_slice::<[u8;3]>() }),
                    4 => format!("{:?}", unsafe{ self.as_slice::<[u8;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::U16 => {
                match self.num_components {
                    1 => format!("{:?}", unsafe{ self.as_slice::<[u16;1]>() }),
                    2 => format!("{:?}", unsafe{ self.as_slice::<[u16;2]>() }),
                    3 => format!("{:?}", unsafe{ self.as_slice::<[u16;3]>() }),
                    4 => format!("{:?}", unsafe{ self.as_slice::<[u16;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::U32 => {
                match self.num_components {
                    1 => format!("{:?}", unsafe{ self.as_slice::<[u32;1]>() }),
                    2 => format!("{:?}", unsafe{ self.as_slice::<[u32;2]>() }),
                    3 => format!("{:?}", unsafe{ self.as_slice::<[u32;3]>() }),
                    4 => format!("{:?}", unsafe{ self.as_slice::<[u32;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::U64 => {
                match self.num_components {
                    1 => format!("{:?}", unsafe{ self.as_slice::<[u64;1]>() }),
                    2 => format!("{:?}", unsafe{ self.as_slice::<[u64;2]>() }),
                    3 => format!("{:?}", unsafe{ self.as_slice::<[u64;3]>() }),
                    4 => format!("{:?}", unsafe{ self.as_slice::<[u64;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::F32 => {
                match self.num_components {
                    1 => format!("{:?}", unsafe{ self.as_slice::<[f32;1]>() }),
                    2 => format!("{:?}", unsafe{ self.as_slice::<[f32;2]>() }),
                    3 => format!("{:?}", unsafe{ self.as_slice::<[f32;3]>() }),
                    4 => format!("{:?}", unsafe{ self.as_slice::<[f32;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
            ComponentDataType::F64 => {
                match self.num_components {
                    1 => format!("{:?}", unsafe{ self.as_slice::<[f64;1]>() }),
                    2 => format!("{:?}", unsafe{ self.as_slice::<[f64;2]>() }),
                    3 => format!("{:?}", unsafe{ self.as_slice::<[f64;3]>() }),
                    4 => format!("{:?}", unsafe{ self.as_slice::<[f64;4]>() }),
                    _ => panic!("Unsupported number of components: {}", self.num_components),
                }
            },
        };
        f.debug_struct("AttributeBuffer")
            .field("len", &self.len)
            .field("component_type", &self.component_type)
            .field("num_components", &self.num_components)
            .field("data", &data)
            .finish()
    }
}

impl std::cmp::PartialEq for AttributeBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && 
        self.component_type == other.component_type && 
        self.num_components == other.num_components &&
        (0..self.len()*self.num_components*self.component_type.size()).all(|i| {
            let self_ptr = unsafe { self.as_ptr().add(i) };
            let other_ptr = unsafe { other.as_ptr().add(i) };
            unsafe { ptr::read(self_ptr) == ptr::read(other_ptr) }
        })
    }
}

pub(crate) struct MaybeInitAttributeBuffer {
    /// Contains the data of the attribute.
    data: RawBuffer,

    /// The length of allocation.
    len: usize,

    /// pointer of the last element.
    last: *mut u8,

    component_type: ComponentDataType,

    num_components: usize,

    /// Debugging purpose only; this will not be used in the release mode.
    initialized_elements: Vec<bool>,
}

impl MaybeInitAttributeBuffer {
    /// Creates a new attribute buffer with the given component type and number of components.
    /// This allocates memory for the buffer, but does not initialize it.
    pub fn new(len: usize, component_type: ComponentDataType, num_components: usize) -> Self {
        let data = RawBuffer::with_capacity(len*component_type.size()*num_components);
        let last = unsafe { data.as_ptr().add(len*component_type.size()*num_components) };
        let mut initialized_elements = Vec::with_capacity(len);
        #[cfg(debug_assertions)] {
            initialized_elements.resize(len, false);
        }
        Self {
            data,
            len,
            last,
            component_type,
            num_components,
            initialized_elements,
        }
    }

    /// Returns a slice of all the values in the buffer casted to the static type `Data`.
	/// Safety: Callers must know exactly which part of resulting slice is valid. \
	/// Dereferencing the uninitialized part of the slice is undefined behavior.
	/// Moreover, 'num_components * component_type.size()' must equal 'std::mem::size_of::<Data>()'.
    pub fn as_slice_unchecked<Data>(&self) -> &[Data] 
        where 
            Data: Vector,
            Data::Component: DataValue
    {
        debug_assert_eq!(
            mem::size_of::<Data>(), self.component_type.size() * self.num_components, 
            "Cannot create slice: Trying to cast to {}, but the buffer stores elements of type {}D vector of {:?}, which has size {}",
            mem::size_of::<Data>(), self.num_components, self.component_type, self.component_type.size(),
        );
        // Safety: upheld.
        unsafe {
            std::slice::from_raw_parts(
                self.data.as_ptr() as *const Data,
                self.len,
            )
        }
    }


    #[allow(unused)]
    #[inline]
    pub fn write<Data>(&mut self, idx: usize, data: Data) 
        where 
            Data: Vector,
            Data::Component: DataValue
    {
        assert_eq!(
            Data::Component::get_dyn(), self.component_type, 
            "Data type mismatch: Cannot push data of type {:?} into attribute buffer of type {:?}", 
            Data::Component::get_dyn(), self.component_type
        );
        assert!(
            Data::NUM_COMPONENTS == self.num_components,
            "Number of components mismatch: Cannot push data with {} components into attribute buffer with {} components",
            Data::NUM_COMPONENTS, self.num_components
        );
        assert!(idx < self.len, "Index out of bounds: The index {} is out of bounds for the attribute buffer with length {}", idx, self.len);

        self.write_type_unchecked(idx, data);
    }

    /// Safety: The caller must ensure that the type of the data matches the type of the buffer.
    /// Furthermore, the index must be within the bounds of the buffer.
    #[inline]
    pub fn write_type_unchecked<Data>(&mut self, idx: usize, data: Data) 
        where 
            Data: Vector,
            Data::Component: DataValue
    {
        debug_assert_eq!(
            Data::Component::get_dyn(), self.component_type, 
            "Data type mismatch: Cannot push data of type {:?} into attribute buffer of type {:?}", 
            Data::Component::get_dyn(), self.component_type
        );
        debug_assert!(
            Data::NUM_COMPONENTS == self.num_components,
            "Number of components mismatch: Cannot push data with {} components into attribute buffer with {} components",
            Data::NUM_COMPONENTS, self.num_components
        );

        debug_assert!(idx < self.len, "Index out of bounds: The index {} is out of bounds for the attribute buffer with length {}", idx, self.len);

        #[cfg(debug_assertions)] {
            self.initialized_elements[idx] = true;
        }

        unsafe {
            (self.data.as_ptr() as *mut Data).add(idx).write(data);
        }
    }

    /// Returns the number of values of the attribute.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns the component type of the attribute.
    #[inline(always)]
    pub fn get_component_type(&self) -> ComponentDataType {
        self.component_type
    }

    /// Returns the number of components.
    #[inline(always)]
    pub fn get_num_components(&self) -> usize {
        self.num_components
    }
}

impl From<MaybeInitAttributeBuffer> for AttributeBuffer {
    fn from(maybe_init: MaybeInitAttributeBuffer) -> Self {
        debug_assert!(
            maybe_init.initialized_elements.iter().all(|&x| x),
            "Not all elements are initialized: Out of {} elements, uninitialized are {:?}",
            maybe_init.len,
            maybe_init.initialized_elements.iter().filter(|&&x| !x)
        );

        Self {
            data: maybe_init.data,
            len: maybe_init.len,
            last: maybe_init.last,
            component_type: maybe_init.component_type,
            num_components: maybe_init.num_components,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::NdVector;

    use super::*;
    #[test]
    fn clone() {
        let data = vec![
            NdVector::from([1.0f32, 2.0, 3.0]), 
            NdVector::from([4.0f32, 5.0, 6.0]), 
            NdVector::from([7.0f32, 8.0, 9.0])
        ];

        let att = AttributeBuffer::from(data);

        let att_clone = att.clone();
        assert_eq!(att, att_clone, "The clone is not equal to the original");
    }

    #[test]
    fn maybe_init() {
        let mut buffer = MaybeInitAttributeBuffer::new(5, ComponentDataType::F32, 3);
        let data = (0..5).map(|i| NdVector::from([i as f32, i as f32, i as f32])).collect::<Vec<_>>();
        let mut idx = 1;
        for _ in 0..5 {
            idx = (idx*2)%5; // 2 is a generator of $ \Z / 5 \Z $
            buffer.write(idx, data[idx]);
        }
        buffer.write(0, data[0]);
        
        let att = AttributeBuffer::from(buffer);
        // check if the data is correct
        let answer = AttributeBuffer::from(data);
        assert_eq!(att, answer, "The attribute buffer is not equal to the original");
    }


    #[test]
    fn test_permute() {
        let data = vec![
            NdVector::from([1f32, 2.0, 3.0]), 
            NdVector::from([4f32, 5.0, 6.0]), 
            NdVector::from([7f32, 8.0, 9.0])
        ];
        let mut att = AttributeBuffer::from(data);
        let permutation = vec![2, 1, 0];
        att.permute(&permutation);
        let expected_data = vec![
            NdVector::from([7f32, 8.0, 9.0]), 
            NdVector::from([4f32, 5.0, 6.0]), 
            NdVector::from([1f32, 2.0, 3.0])
        ];
        let expected_att = AttributeBuffer::from(expected_data);
        assert_eq!(att, expected_att);
    }
}