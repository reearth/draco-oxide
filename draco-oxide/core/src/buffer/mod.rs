use crate::safety_assert;
pub mod attribute;

use std::{alloc, fmt, ptr};

pub trait OrderConfig {
    const IS_MSB_FIRST: bool;
}

#[derive(Debug)]
pub struct MsbFirst;

#[derive(Debug)]
pub struct LsbFirst;

impl OrderConfig for MsbFirst {
    const IS_MSB_FIRST: bool = true;
}
impl OrderConfig for LsbFirst {
    const IS_MSB_FIRST: bool = false;
}

struct RawBuffer {
    data: ptr::NonNull<u8>,

    /// the size of the allocation in bytes.
    /// The number of bits that can be stored in the buffer is 'cap' * 8.
    cap: usize,

    /// alignment of the allocation; part of the layout it is freed with.
    align: usize,
}

impl RawBuffer {
    /// constructs a new buffer with the given capacity.
    /// 'cap' must be given in bytes.
    fn with_capacity(cap: usize) -> Self {
        if cap == 0 {
            return Self {
                data: ptr::NonNull::dangling(),
                cap: 0,
                align: 1,
            };
        }
        let layout = alloc::Layout::array::<u8>(cap).unwrap();
        // Safety: 'layout' has a non-zero size.
        let data = unsafe { alloc::alloc(layout) };
        Self {
            data: ptr::NonNull::new(data).unwrap_or_else(|| alloc::handle_alloc_error(layout)),
            cap,
            align: 1,
        }
    }

    fn layout(&self) -> alloc::Layout {
        alloc::Layout::from_size_align(self.cap, self.align).unwrap()
    }

    /// expands the buffer to 'new_cap'.
    /// Safety: 'new_cap' must be less than 'usize::Max' and greater than zero.
    unsafe fn expand(&mut self, new_cap: usize) {
        safety_assert!(new_cap < usize::MAX, "'new_cap' is too large");
        safety_assert!(new_cap > 0, "'new_cap' must be positive");
        let new_layout = alloc::Layout::from_size_align(new_cap, self.align).unwrap();
        let new_data = if self.cap == 0 {
            alloc::alloc(new_layout)
        } else {
            alloc::realloc(self.data.as_ptr(), self.layout(), new_cap)
        };
        self.data =
            ptr::NonNull::new(new_data).unwrap_or_else(|| alloc::handle_alloc_error(new_layout));
        self.cap = new_cap;
    }

    /// doubles the capacity of the buffer.
    fn double(&mut self) {
        let new_cap = (self.cap * 2).max(1);
        assert!(new_cap < usize::MAX, "'new_cap' is too large");
        // Safety: Just checked that 'new_cap' is positive and less than 'usize::Max'.
        unsafe {
            self.expand(new_cap);
        }
    }

    fn as_ptr(&self) -> *mut u8 {
        self.data.as_ptr()
    }

    /// Takes over the allocation of 'v'. The buffer records the vector's full
    /// capacity and element alignment so the allocation is freed with the
    /// layout it was made with.
    fn from_vec<Data>(mut v: Vec<Data>) -> Self {
        let cap = v.capacity() * std::mem::size_of::<Data>();
        let data = v.as_mut_ptr() as *mut u8;
        std::mem::forget(v);
        Self {
            data: ptr::NonNull::new(data).unwrap(),
            cap,
            align: std::mem::align_of::<Data>(),
        }
    }

    /// Releases ownership of the allocation without freeing it.
    fn into_raw(self) -> *mut u8 {
        let data = self.data.as_ptr();
        std::mem::forget(self);
        data
    }
}

impl Drop for RawBuffer {
    fn drop(&mut self) {
        if self.cap != 0 {
            // Safety: 'data' was allocated by the global allocator with 'self.layout()'.
            unsafe { alloc::dealloc(self.data.as_ptr(), self.layout()) };
        }
    }
}

impl fmt::Debug for RawBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for n in 0..self.cap {
            write!(f, "{:02x} ", unsafe { *self.data.as_ptr().add(n) })?;
        }
        Ok(())
    }
}
