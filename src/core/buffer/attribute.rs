use std::{ptr, mem};
use crate::core::shared::DataValue;
use crate::core::attribute::ComponentDataType;
use crate::core::shared::Vector;

use super::RawBuffer;

pub(crate) struct AttributeBuffer {
    /// Contains the data of the attribute.
    data: RawBuffer,

    /// The number of values of the attribute.
    len: usize,

    /// pointer of the last element.
    last: *mut u8,

    component_type: ComponentDataType,

    num_components: usize,
}

impl AttributeBuffer {
    pub fn new(component_type: ComponentDataType, num_components: usize) -> Self {
        Self {
            data: RawBuffer::new(),
            len: 0,
            last: ptr::null_mut(),
            component_type,
            num_components
        }
    }

    fn as_ptr(&self) -> *mut u8 {
        self.data.as_ptr()
    }

    pub(crate) fn get<Data>(&self, idx: usize) -> Data 
        where 
            Data: Vector,
            Data::Component: DataValue
    {
        assert!(idx < self.len, "Index out of bounds: The index {} is out of bounds for the attribute buffer with length {}", idx, self.len);
        let size = mem::size_of::<Data>();
        let ptr = unsafe{ self.as_ptr().add(size * idx) };
        unsafe{ ptr::read(ptr as *const Data) }
    }

    pub(crate) fn get_component_type(&self) -> ComponentDataType {
        self.component_type
    }

    pub(crate) fn get_num_components(&self) -> usize {
        self.num_components
    }

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

    /// pushes a value into the buffer without checking the type and number.
    /// # Safety
    /// This function is unsafe because it does not check the type and number of components of the data.
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

    fn push_value<Data>(&mut self, value: Data) 
        where Data: DataValue
    {
        let size = mem::size_of::<Data>();

        if self.last.is_null() {
            self.last = self.data.as_ptr();
        } else {
            self.last = unsafe { self.last.add(size) };
        }

        unsafe {
            ptr::write(self.last as *mut Data, value);
        }
    }
}