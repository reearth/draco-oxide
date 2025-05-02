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

    fn push_value_unchecked<Data>(&mut self, value: Data) 
        where Data: DataValue
    {
        let size = mem::size_of::<Data>();

        debug_assert!(!self.last.is_null());
        debug_assert_eq!(self.last, unsafe{ self.data.as_ptr().add(self.len * size) });
        debug_assert!(self.len * size <= isize::MAX as usize);

        unsafe {
            ptr::write(self.last as *mut Data, value);
        }

        self.last = unsafe { self.last.add(size) };
    }

	#[inline(always)]
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


pub(crate) struct AttributeBufferFamily {
    pub(crate) num_components: usize,
    pub(crate) component_type: ComponentDataType,
    pub(crate) atts: Vec<AttributeBuffer>,

}
/// Classifies the attributes into distinct families based on their component type and number of components.
pub fn classify(atts: Vec<AttributeBuffer>) -> Vec<AttributeBufferFamily> {
    let mut families: Vec<AttributeBufferFamily> = Vec::new();

    for att in atts {
        let maybe_handle = families.iter_mut().find(|f| 
                f.component_type == att.get_component_type() && f.num_components == att.get_num_components()
            );
        match maybe_handle {
            Some(handle) => handle.atts.push(att),
            None => {
                families.push(
                    AttributeBufferFamily { 
                        num_components: att.get_num_components(),
                        component_type: att.get_component_type(),
                        atts: vec![att],
                    }
                );
            }
        };
    }

    families
}